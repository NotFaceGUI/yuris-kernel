//! 存档域文件系统端口。
//!
//! 内核只认 [`SaveFs`]，不直接调 `std::fs`。这样「存档能力」与「平台能力」
//! 解耦：桌面与 Android 都有真实文件系统，用默认的 [`NativeSaveFs`]（行为
//! 与旧直连 `std::fs` 逐字节一致）；无文件系统的平台（浏览器 wasm）注入自己
//! 的同步实现即可，状态机一行不改。
//!
//! # 为什么是 `&Path` 而不是相对名
//!
//! 脚本用 `$es.SAVEPATH = $_SAVE_PATH`（宿主注入的**绝对路径**）拼文件名，
//! `FILEACT` 会直接传这个绝对前缀（`es_First.yst:30`、`es_SaveLoad.yst`）。
//! 端口因此接受已过 [`SaveState::confined_path`](crate::SaveState) 越界校验的
//! 路径；「路径怎么映射到存储键」是注入实现的事，不是内核的事。
//!
//! # 契约
//!
//! - 全部方法**同步**（`SAVE`/`LOAD`/`FILEACT` 都在命令时刻完成）；
//! - `list` 只列直接子项，名字按字节序排序（调用方据此建确定性槽表）；
//! - 错误类型用 `std::io::Error`，让「文件不存在」等语义与 `std::fs` 同型
//!   （`ErrorKind::NotFound` 是幂等删除与缺档判定的依据）。

use std::io;
use std::path::{Path, PathBuf};

/// 目录项（`list` 的返回项）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEntry {
    /// 文件名（不含目录）。
    pub name: String,
    /// 字节数（目录项不需要时给 0）。
    pub size: u64,
    /// 是否目录。
    pub is_dir: bool,
}

/// 存档域文件系统端口。
pub trait SaveFs: Send + Sync {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// 只读前 `max` 字节（容器版本探测；默认实现读全量再截断）。
    fn read_prefix(&self, path: &Path, max: usize) -> io::Result<Vec<u8>> {
        let bytes = self.read(path)?;
        Ok(bytes[..bytes.len().min(max)].to_vec())
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    /// 列出目录的直接子项（不存在 = `ErrorKind::NotFound`）。
    fn list(&self, dir: &Path) -> io::Result<Vec<FsEntry>>;
    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    /// 解析符号链接与 `..`。不支持/目标不存在的平台返回 `None`，
    /// 调用方退化为词法校验（与旧行为同口径）。
    fn canonicalize(&self, path: &Path) -> Option<PathBuf>;
}

/// 端口句柄（`SaveState` 需要 `Send + Sync`，`Arc<dyn Trait>` 两者都不打印）。
#[derive(Clone)]
pub struct SaveFsHandle(pub std::sync::Arc<dyn SaveFs>);

impl std::fmt::Debug for SaveFsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SaveFsHandle")
    }
}

impl std::ops::Deref for SaveFsHandle {
    type Target = dyn SaveFs;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl SaveFsHandle {
    pub fn new(fs: impl SaveFs + 'static) -> Self {
        Self(std::sync::Arc::new(fs))
    }
}

/// 平台原生实现（桌面 / Android / 任何有 `std::fs` 的目标）。
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeSaveFs;

impl SaveFs for NativeSaveFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_prefix(&self, path: &Path, max: usize) -> io::Result<Vec<u8>> {
        use std::io::Read;
        let mut head = vec![0u8; max];
        let mut file = std::fs::File::open(path)?;
        let mut filled = 0usize;
        while filled < max {
            match file.read(&mut head[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        head.truncate(filled);
        Ok(head)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<FsEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                // 非 UTF-8 名字：与旧扫描同口径跳过（槽名必须是可显示名）。
                continue;
            };
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(_) => continue,
            };
            entries.push(FsEntry {
                name,
                size: meta.len(),
                is_dir: meta.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    fn copy(&self, from: &Path, to: &Path) -> io::Result<u64> {
        std::fs::copy(from, to)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        path.canonicalize().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试根:优先 `YURIS_TEST_TMP`(沙箱),否则仓内 `target/test-tmp`
    /// —— 不用 `std::env::temp_dir()`,本机系统 TEMP 对测试进程不可写。
    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            });
        base.join(tag)
    }

    #[test]
    fn native_lists_sorted_and_reports_dirs() {
        let root = temp_root("fs-port");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("b.sd"), b"bb").unwrap();
        std::fs::write(root.join("a.sd"), b"a").unwrap();

        let fs = NativeSaveFs;
        let entries = fs.list(&root).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a.sd", "b.sd", "sub"]);
        assert_eq!(entries[0].size, 1);
        assert!(entries[2].is_dir);
        assert_eq!(fs.read(&root.join("a.sd")).unwrap(), b"a");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn native_missing_is_not_found() {
        let fs = NativeSaveFs;
        let missing = temp_root("fs-port-missing").join("x.sd");
        assert_eq!(
            fs.read(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!fs.exists(&missing));
        assert!(fs.canonicalize(&missing).is_none());
    }

    #[test]
    fn read_prefix_reads_only_the_head() {
        let root = temp_root("fs-port-prefix");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("head.sd");
        std::fs::write(&path, b"YSSD\x03\x00\x00\x00payload").unwrap();

        let fs = NativeSaveFs;
        assert_eq!(fs.read_prefix(&path, 8).unwrap(), b"YSSD\x03\x00\x00\x00");
        // 文件比请求短:返回实有字节(15),不报错。
        assert_eq!(fs.read_prefix(&path, 4096).unwrap().len(), 15);

        let _ = std::fs::remove_dir_all(&root);
    }
}
