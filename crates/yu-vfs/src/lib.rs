//! `yu-vfs` · 数据源抽象与路径归一。
//!
//! 契约见 `docs/architecture.md` §1 / 不变式 A1、A2、A5:
//! - 散盘目录、`.ypf` 封包、内存夹具在 [`DataSource`] 之上是同一个程序;
//! - 路径归一只在 [`LookupKey`] 构造时做一次(分隔符统一 + ASCII 折叠 + 多字节整对);
//! - 数据只读,写存档走独立 `SaveDir`(不在本 crate 范围)。
//!
//! 三种实现: [`MemSource`] / [`DirSource`] / [`YpfSource`],叠加用 [`Mount`]。
//!
//! `clippy::result_large_err` 在此显式放行: 错误类型是 `yu-log` 的 [`Diag`],
//! 结构化上下文(边界/位置/因果链)是契约的一部分,见 architecture §8。
#![allow(clippy::result_large_err)]

mod codec;
mod dir;
pub mod encoding;
mod key;
mod mem;
mod mount;
pub mod ypf;

pub use codec::Codec;
pub use dir::DirSource;
pub use encoding::{detect as detect_encoding, Decision as EncodingDecision, TextEncoding};
pub use key::{normalize_component, split_components, LookupKey};
pub use mem::MemSource;
pub use mount::Mount;
pub use ypf::{YpfDialect, YpfEntryInfo, YpfHeader, YpfParseStats, YpfSource};

use std::collections::{BTreeMap, BTreeSet};
use yu_log::{code, Boundary, Diag};

/// 目录项种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Dir,
}

impl EntryKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryKind::File => "file",
            EntryKind::Dir => "dir",
        }
    }
}

/// [`DataSource::list`] 的返回项。`name` 是最后一级组件的归一小写字节。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub kind: EntryKind,
    pub size: Option<u64>,
}

/// 只读数据源。
///
/// 各实现只做**精确匹配**: 输入必须是 [`LookupKey`]。归一在边界之外完成,
/// 避免同一路径在不同层被重复解释。
pub trait DataSource: Send {
    /// 按归一键读取完整字节。
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag>;

    /// 按归一键判断存在性。
    fn exists(&self, path: &LookupKey) -> bool;

    /// 列出目录的直接子项(按名字排序)。`path` 指向文件时返回
    /// [`code::vfs::NOT_DIR`];不存在时返回 [`code::vfs::MISSING`]。
    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag>;

    /// 诊断用的数据源名称。
    fn source_name(&self) -> &str;
}

pub(crate) fn missing(path: &LookupKey, source: &str) -> Diag {
    Diag::recoverable(code::vfs::MISSING, Boundary::Data)
        .at_data(path.as_bytes().to_vec(), 0)
        .with_detail(format!("{source}: entry not found"))
}

pub(crate) fn not_dir(path: &LookupKey, source: &str) -> Diag {
    Diag::recoverable(code::vfs::NOT_DIR, Boundary::Data)
        .at_data(path.as_bytes().to_vec(), 0)
        .with_detail(format!("{source}: path is a file"))
}

pub(crate) fn collision(
    path: &LookupKey,
    source: &str,
    detail: impl Into<String>,
) -> Diag {
    Diag::fatal(code::vfs::PATH_COLLISION, Boundary::Data)
        .at_data(path.as_bytes().to_vec(), 0)
        .with_detail(format!("{source}: {}", detail.into()))
}

/// 由文件/目录集合派生的确定性目录树。三种数据源共用,保证 `list` 语义一致。
#[derive(Default)]
pub(crate) struct DirTree {
    children: BTreeMap<LookupKey, BTreeMap<Vec<u8>, Entry>>,
    dirs: BTreeSet<LookupKey>,
    files: BTreeSet<LookupKey>,
}

impl DirTree {
    pub(crate) fn new() -> Self {
        let mut tree = Self::default();
        tree.dirs.insert(LookupKey::root());
        tree
    }

    pub(crate) fn insert_dir(&mut self, path: &LookupKey) -> Result<(), Diag> {
        if path.is_root() || self.dirs.contains(path) {
            return Ok(());
        }
        if self.files.contains(path) {
            return Err(collision(
                path,
                "dir-tree",
                "same normalized path is both file and directory",
            ));
        }
        let parent = path.parent().unwrap_or_else(LookupKey::root);
        self.insert_dir(&parent)?;
        let name = path
            .file_name()
            .map(|n| n.to_vec())
            .ok_or_else(|| collision(path, "dir-tree", "directory has no name"))?;
        self.dirs.insert(path.clone());
        self.children
            .entry(parent)
            .or_default()
            .insert(name.clone(), Entry {
                name,
                kind: EntryKind::Dir,
                size: None,
            });
        Ok(())
    }

    pub(crate) fn insert_file(&mut self, path: &LookupKey, size: u64) -> Result<(), Diag> {
        if self.dirs.contains(path) && !path.is_root() {
            return Err(collision(
                path,
                "dir-tree",
                "same normalized path is both file and directory",
            ));
        }
        let mut ancestors = Vec::new();
        let mut cursor = path.parent();
        while let Some(parent) = cursor {
            ancestors.push(parent.clone());
            if parent.is_root() {
                break;
            }
            cursor = parent.parent();
        }
        for ancestor in ancestors.into_iter().rev() {
            self.insert_dir(&ancestor)?;
        }
        let parent = path.parent().unwrap_or_else(LookupKey::root);
        let name = path
            .file_name()
            .map(|n| n.to_vec())
            .ok_or_else(|| collision(path, "dir-tree", "file has no name"))?;
        self.files.insert(path.clone());
        self.children
            .entry(parent)
            .or_default()
            .insert(name.clone(), Entry {
                name,
                kind: EntryKind::File,
                size: Some(size),
            });
        Ok(())
    }

    pub(crate) fn list(&self, path: &LookupKey, source: &str) -> Result<Vec<Entry>, Diag> {
        if self.files.contains(path) {
            return Err(not_dir(path, source));
        }
        if !self.dirs.contains(path) {
            return Err(missing(path, source));
        }
        Ok(self
            .children
            .get(path)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_tree_lists_sorted_children() {
        let mut tree = DirTree::new();
        let b = LookupKey::new(b"b.txt", Codec::Cp932).unwrap();
        let a = LookupKey::new(b"a.txt", Codec::Cp932).unwrap();
        let sub = LookupKey::new(b"sub/c.bin", Codec::Cp932).unwrap();
        tree.insert_file(&b, 2).unwrap();
        tree.insert_file(&a, 1).unwrap();
        tree.insert_file(&sub, 3).unwrap();
        let root = tree.list(&LookupKey::root(), "test").unwrap();
        let names: Vec<&[u8]> = root.iter().map(|e| e.name.as_slice()).collect();
        assert_eq!(names, vec![b"a.txt".as_slice(), b"b.txt", b"sub"]);
        let inner = tree
            .list(&LookupKey::new(b"sub", Codec::Cp932).unwrap(), "test")
            .unwrap();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].size, Some(3));
    }

    #[test]
    fn dir_tree_reports_file_and_missing() {
        let mut tree = DirTree::new();
        let a = LookupKey::new(b"a.txt", Codec::Cp932).unwrap();
        tree.insert_file(&a, 1).unwrap();
        assert_eq!(
            tree.list(&a, "test").unwrap_err().code,
            code::vfs::NOT_DIR
        );
        let missing_key = LookupKey::new(b"nope", Codec::Cp932).unwrap();
        assert_eq!(
            tree.list(&missing_key, "test").unwrap_err().code,
            code::vfs::MISSING
        );
    }

    #[test]
    fn dir_tree_rejects_file_dir_collision() {
        let mut tree = DirTree::new();
        let a = LookupKey::new(b"a", Codec::Cp932).unwrap();
        let ab = LookupKey::new(b"a/b", Codec::Cp932).unwrap();
        tree.insert_file(&a, 1).unwrap();
        assert_eq!(
            tree.insert_file(&ab, 2).unwrap_err().code,
            code::vfs::PATH_COLLISION
        );
    }
}
