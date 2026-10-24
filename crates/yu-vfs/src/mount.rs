//! 叠加挂载栈。后挂者覆盖先挂者(architecture §1)。
//!
//! 引擎实测的挂载顺序: 基础包 + `update1..9` 依次覆盖。覆盖按**归一路径**
//! 精确匹配;上层存在文件时,下层同路径的目录整体被遮蔽。
//!
//! 接线在 `yu-cli/src/project.rs`(`Project::update_packs` 枚举增量包,
//! `open_source` / `mount` 按序 push;批次 B5)。

use crate::{missing, not_dir, Codec, DataSource, Entry, LookupKey};
use std::collections::BTreeMap;
use yu_log::Diag;

/// 有序数据源栈。索引 0 = 最底层,末位 = 最高层。
pub struct Mount {
    codec: Codec,
    sources: Vec<Box<dyn DataSource>>,
}

impl Mount {
    pub fn new(codec: Codec) -> Self {
        Self {
            codec,
            sources: Vec::new(),
        }
    }

    /// 追加一个数据源(后来者覆盖先来者)。
    pub fn push(&mut self, source: Box<dyn DataSource>) {
        self.sources.push(source);
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn sources(&self) -> &[Box<dyn DataSource>] {
        &self.sources
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// 原始路径入口: 归一一次后交给挂载栈。
    pub fn open_path(&self, raw: &[u8]) -> Result<Vec<u8>, Diag> {
        let key = LookupKey::new(raw, self.codec)?;
        DataSource::open(self, &key)
    }

    pub fn exists_path(&self, raw: &[u8]) -> bool {
        match LookupKey::new(raw, self.codec) {
            Ok(key) => DataSource::exists(self, &key),
            Err(_) => false,
        }
    }

    pub fn list_path(&self, raw: &[u8]) -> Result<Vec<Entry>, Diag> {
        let key = LookupKey::new(raw, self.codec)?;
        DataSource::list(self, &key)
    }
}

impl DataSource for Mount {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        for source in self.sources.iter().rev() {
            match source.open(path) {
                Ok(bytes) => return Ok(bytes),
                Err(diag) if diag.is_missing() => continue,
                Err(diag) => return Err(diag),
            }
        }
        Err(missing(path, self.source_name()))
    }

    fn exists(&self, path: &LookupKey) -> bool {
        self.sources.iter().rev().any(|s| s.exists(path))
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        let mut merged: BTreeMap<Vec<u8>, Entry> = BTreeMap::new();
        let mut saw_dir = false;
        for source in self.sources.iter().rev() {
            match source.list(path) {
                Ok(entries) => {
                    saw_dir = true;
                    for entry in entries {
                        merged.entry(entry.name.clone()).or_insert(entry);
                    }
                }
                Err(diag) if diag.code == yu_log::code::vfs::NOT_DIR => {
                    if !saw_dir {
                        // 最高层在这个路径上是文件,下层目录被整体遮蔽。
                        return Err(not_dir(path, self.source_name()));
                    }
                    // 上层目录遮蔽下层同路径文件: 忽略。
                }
                Err(diag) if diag.is_missing() => continue,
                Err(diag) => return Err(diag),
            }
        }
        if !saw_dir {
            return Err(missing(path, self.source_name()));
        }
        Ok(merged.into_values().collect())
    }

    fn source_name(&self) -> &str {
        "mount"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemSource;

    fn mem(files: &[(&[u8], &[u8])]) -> Box<dyn DataSource> {
        let mut source = MemSource::new(Codec::Cp932);
        for (path, bytes) in files {
            source.insert(path, bytes.to_vec()).unwrap();
        }
        Box::new(source)
    }

    #[test]
    fn later_source_overrides_earlier() {
        let mut mount = Mount::new(Codec::Cp932);
        mount.push(mem(&[(b"a.txt", b"base"), (b"only_base.txt", b"b")]));
        mount.push(mem(&[(b"a.txt", b"update"), (b"only_update.txt", b"u")]));

        assert_eq!(mount.open_path(b"a.txt").unwrap(), b"update");
        assert_eq!(mount.open_path(b"only_base.txt").unwrap(), b"b");
        assert_eq!(mount.open_path(b"only_update.txt").unwrap(), b"u");

        let root = mount.list_path(b"").unwrap();
        let names: Vec<&[u8]> = root.iter().map(|e| e.name.as_slice()).collect();
        assert_eq!(
            names,
            vec![b"a.txt".as_slice(), b"only_base.txt", b"only_update.txt"]
        );
    }

    #[test]
    fn missing_uses_recoverable_code() {
        let mut mount = Mount::new(Codec::Cp932);
        mount.push(mem(&[(b"a.txt", b"a")]));
        let diag = mount.open_path(b"nope.txt").unwrap_err();
        assert_eq!(diag.code, yu_log::code::vfs::MISSING);
        assert!(!mount.exists_path(b"nope.txt"));
    }

    #[test]
    fn upper_file_shadows_lower_directory() {
        let mut mount = Mount::new(Codec::Cp932);
        mount.push(mem(&[(b"a/b.txt", b"inner")]));
        mount.push(mem(&[(b"a", b"file")]));
        let a = LookupKey::new(b"a", Codec::Cp932).unwrap();
        let diag = mount.list(&a).unwrap_err();
        assert_eq!(diag.code, yu_log::code::vfs::NOT_DIR);
        assert_eq!(mount.open_path(b"a").unwrap(), b"file");
    }

    #[test]
    fn upper_directory_shadows_lower_file() {
        let mut mount = Mount::new(Codec::Cp932);
        mount.push(mem(&[(b"a", b"file")]));
        mount.push(mem(&[(b"a/b.txt", b"inner")]));
        let a = LookupKey::new(b"a", Codec::Cp932).unwrap();
        let entries = mount.list(&a).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, b"b.txt");
    }
}
