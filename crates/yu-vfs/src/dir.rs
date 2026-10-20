//! 散盘目录数据源。
//!
//! 只在构造时走一遍目录,把归一键映射到真实路径;此后 `open` 直接按映射读文件。
//! `list` 由共用的 [`DirTree`] 派生,因此与 [`crate::YpfSource`] 的结果可比。
//!
//! **非 ASCII 文件名**: 归一键是 CP932/GBK 字节,而 `std::fs` 只给 Unicode。
//! 完整编解码表归 `yu-format`(任务书 B),本层不猜编码:非 ASCII 组件被跳过并
//! 计数留痕,调用方可据此显式拒绝启动。纯 ASCII 数据源不受影响。

use crate::{missing, Codec, DataSource, DirTree, Entry, LookupKey};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use yu_log::{code, Boundary, Diag};

/// 构造期被跳过的条目(非 ASCII 名 / 符号链接)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirSkip {
    /// 相对根的路径(显示用,可能含非 UTF-8 替换符)。
    pub path: String,
    /// 跳过原因:`non_ascii_name` / `symlink`。
    pub reason: &'static str,
}

/// 散盘目录数据源。
pub struct DirSource {
    root: PathBuf,
    codec: Codec,
    files: std::collections::BTreeMap<LookupKey, PathBuf>,
    tree: DirTree,
    skipped: Vec<DirSkip>,
}

impl DirSource {
    /// 打开一个目录作为挂载根。目录必须存在。
    pub fn open(root: impl AsRef<Path>, codec: Codec) -> Result<Self, Diag> {
        let root = root.as_ref().to_path_buf();
        let meta = fs::symlink_metadata(&root).map_err(|e| io_error(&root, e))?;
        if !meta.is_dir() {
            return Err(Diag::fatal(code::vfs::IO, Boundary::Data)
                .at_data(path_bytes(&root), 0)
                .with_detail("DirSource root is not a directory"));
        }
        let mut source = Self {
            root: root.clone(),
            codec,
            files: Default::default(),
            tree: DirTree::new(),
            skipped: Vec::new(),
        };
        source.walk(&root)?;
        Ok(source)
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 构造期跳过的条目(计数可审计,不是静默忽略)。
    pub fn skipped(&self) -> &[DirSkip] {
        &self.skipped
    }

    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }

    /// 已建立索引的普通文件数(不含跳过的条目)。
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    fn walk(&mut self, dir: &Path) -> Result<(), Diag> {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map_err(|e| io_error(dir, e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io_error(dir, e))?;
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| io_error(&path, e))?;
            let rel = match path.strip_prefix(&self.root) {
                Ok(rel) => rel,
                Err(_) => {
                    return Err(Diag::fatal(code::vfs::IO, Boundary::Data)
                        .at_data(path_bytes(&path), 0)
                        .with_detail("walked path escaped the DirSource root"));
                }
            };
            let Some(raw_key) = self.relative_key(rel) else {
                continue;
            };

            let file_type = meta.file_type();
            if file_type.is_symlink() {
                self.skipped.push(DirSkip {
                    path: rel.to_string_lossy().into_owned(),
                    reason: "symlink",
                });
                continue;
            }
            if file_type.is_dir() {
                let key = LookupKey::new(&raw_key, self.codec)?;
                self.tree.insert_dir(&key)?;
                self.walk(&path)?;
            } else if file_type.is_file() {
                let key = LookupKey::new(&raw_key, self.codec)?;
                let size = meta.len();
                self.tree.insert_file(&key, size)?;
                self.files.insert(key, path);
            } else {
                self.skipped.push(DirSkip {
                    path: rel.to_string_lossy().into_owned(),
                    reason: "not_a_regular_file",
                });
            }
        }
        Ok(())
    }

    /// 把相对路径转成原始字节键;任一组件的名字不是纯 ASCII 时返回 `None`
    /// 并留痕(编码表归 yu-format)。
    fn relative_key(&mut self, rel: &Path) -> Option<Vec<u8>> {
        let mut bytes = Vec::new();
        for component in rel.components() {
            let Component::Normal(name) = component else {
                self.skipped.push(DirSkip {
                    path: rel.to_string_lossy().into_owned(),
                    reason: "non_normal_component",
                });
                return None;
            };
            let Some(text) = name.to_str() else {
                self.skipped.push(DirSkip {
                    path: rel.to_string_lossy().into_owned(),
                    reason: "non_utf8_name",
                });
                return None;
            };
            if !text.is_ascii() {
                self.skipped.push(DirSkip {
                    path: rel.to_string_lossy().into_owned(),
                    reason: "non_ascii_name",
                });
                return None;
            }
            if !bytes.is_empty() {
                bytes.push(b'/');
            }
            bytes.extend_from_slice(text.as_bytes());
        }
        if bytes.is_empty() {
            None
        } else {
            Some(bytes)
        }
    }
}

impl DataSource for DirSource {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        let Some(real) = self.files.get(path) else {
            return Err(missing(path, self.source_name()));
        };
        fs::read(real).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                missing(path, self.source_name())
            } else {
                io_error(real, e)
            }
        })
    }

    fn exists(&self, path: &LookupKey) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        self.tree.list(path, self.source_name())
    }

    fn source_name(&self) -> &str {
        "dir"
    }
}

fn io_error(path: &Path, error: io::Error) -> Diag {
    Diag::fatal(code::vfs::IO, Boundary::Data)
        .at_data(path_bytes(path), 0)
        .with_detail(error.to_string())
}

fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().as_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(tag: &str) -> PathBuf {
        let base = std::env::var_os("YURIS_TEST_TMP")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!("yu-vfs-{tag}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn indexes_ascii_tree_and_folds_case() {
        let root = temp_root("dir");
        fs::create_dir_all(root.join("CG/Ev")).unwrap();
        fs::write(root.join("CG/Ev/A.PNG"), b"png").unwrap();
        fs::write(root.join("root.txt"), b"txt").unwrap();

        let src = DirSource::open(&root, Codec::Cp932).unwrap();
        let png = LookupKey::new(b"cg/ev/a.png", Codec::Cp932).unwrap();
        assert!(src.exists(&png));
        assert_eq!(src.open(&png).unwrap(), b"png");
        let root_list = src.list(&LookupKey::root()).unwrap();
        let names: Vec<&[u8]> = root_list.iter().map(|e| e.name.as_slice()).collect();
        assert_eq!(names, vec![b"cg".as_slice(), b"root.txt"]);
        assert_eq!(src.skipped_count(), 0);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn non_ascii_names_are_counted_not_silently_dropped() {
        let root = temp_root("nonascii");
        fs::write(root.join("ascii.txt"), b"a").unwrap();
        // 仅在文件系统接受该名字时构造;Windows/Linux 都可写 UTF-8 名。
        let jp = root.join("日本語.txt");
        if fs::write(&jp, b"j").is_ok() {
            let src = DirSource::open(&root, Codec::Cp932).unwrap();
            assert!(src.exists(&LookupKey::new(b"ascii.txt", Codec::Cp932).unwrap()));
            assert_eq!(src.skipped_count(), 1);
            assert_eq!(src.skipped()[0].reason, "non_ascii_name");
        }
        fs::remove_dir_all(&root).ok();
    }
}
