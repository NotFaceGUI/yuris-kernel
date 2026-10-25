//! 散盘目录数据源。
//!
//! 只在构造时走一遍目录,把归一键映射到真实路径;此后 `open` 直接按映射读文件。
//! `list` 由共用的 [`DirTree`] 派生,因此与 [`crate::YpfSource`] 的结果可比。
//!
//! **非 ASCII 文件名**: 归一键是 CP932/GBK 字节,而 `std::fs` 只给 Unicode。
//! 非 ASCII 组件经 `yu-format` 的码表 encode 回配置编解码器的字节键
//! (DIV-003;architecture §7:表本体归 yu-format,本层只调用)。编码表对
//! 全表满足 `decode∘encode` 恒等,Windows 引擎按 Unicode 名访问散盘文件,
//! 折回即恢复其 CP932/GBK 字节路径语义。名字里有该编码不可映射字符的组件
//! 无法折回,被跳过并计数留痕(`unencodable_name`),调用方可据此显式拒绝。

use crate::{missing, Codec, DataSource, DirTree, Entry, LookupKey};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use yu_log::{code, Boundary, Diag};

/// 构造期被跳过的条目(不可编码名 / 符号链接)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirSkip {
    /// 相对根的路径(显示用,可能含非 UTF-8 替换符)。
    pub path: String,
    /// 跳过原因:`unencodable_name` / `non_utf8_name` / `non_normal_component`
    /// / `symlink` / `not_a_regular_file`。
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

    /// 把相对路径转成原始字节键。非 ASCII 组件经 `yu-format` 码表折回配置
    /// 编解码器的字节;任一组件编码失败(不可映射字符)时整体跳过并留痕。
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
            let encoded = if text.is_ascii() {
                text.as_bytes().to_vec()
            } else {
                match self.codec.to_format().encode(text, &[]) {
                    Ok(encoded) => encoded,
                    Err(_) => {
                        self.skipped.push(DirSkip {
                            path: rel.to_string_lossy().into_owned(),
                            reason: "unencodable_name",
                        });
                        return None;
                    }
                }
            };
            if !bytes.is_empty() {
                bytes.push(b'/');
            }
            bytes.extend_from_slice(&encoded);
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
        // 缺省落在仓库内 `target/test-tmp`:受限环境只允许写工作区,
        // 系统 TEMP 会 `PermissionDenied`。`YURIS_TEST_TMP` 仍可覆盖。
        let base = std::env::var_os("YURIS_TEST_TMP").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
        });
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = base.join(format!("yu-vfs-{tag}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("create_dir_all({}) 失败: {e}", dir.display()));
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
    fn non_ascii_names_are_folded_to_codec_bytes() {
        let root = temp_root("nonascii");
        fs::write(root.join("ascii.txt"), b"a").unwrap();
        // 仅在文件系统接受该名字时构造;Windows/Linux 都可写 UTF-8 名。
        let jp = root.join("日本語.txt");
        if fs::write(&jp, b"j").is_ok() {
            let src = DirSource::open(&root, Codec::Cp932).unwrap();
            assert!(src.exists(&LookupKey::new(b"ascii.txt", Codec::Cp932).unwrap()));
            // CP932 独立 oracle(Python cp932): 日=0x93FA 本=0x967B 語=0x8CEA。
            let raw = [0x93, 0xFA, 0x96, 0x7B, 0x8C, 0xEA, 0x2E, 0x74, 0x78, 0x74];
            let key = LookupKey::new(&raw, Codec::Cp932).unwrap();
            assert_eq!(src.open(&key).unwrap(), b"j");
            assert_eq!(src.skipped_count(), 0);
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unencodable_name_is_skipped_and_counted() {
        let root = temp_root("unencodable");
        fs::write(root.join("ascii.txt"), b"a").unwrap();
        // U+1F642 在 CP932/GBK 中均不可映射;仅当文件系统接受该名字时构造。
        let emoji = root.join("🙂.txt");
        if fs::write(&emoji, b"e").is_ok() {
            let src = DirSource::open(&root, Codec::Cp932).unwrap();
            assert!(src.exists(&LookupKey::new(b"ascii.txt", Codec::Cp932).unwrap()));
            assert_eq!(src.skipped_count(), 1);
            assert_eq!(src.skipped()[0].reason, "unencodable_name");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gbk_codec_folds_names_and_keeps_trail_bytes() {
        let root = temp_root("gbkfold");
        let jp = root.join("日本語.txt");
        if fs::write(&jp, b"j").is_ok() {
            let src = DirSource::open(&root, Codec::Gbk).unwrap();
            // GBK 独立 oracle(Python gbk): 日=0xC8D5 本=0xB1BE 語=0xD55A。
            // 語 的 trail 0x5A 是 ASCII 'Z',多字节整对语义下不得折成 0x7A。
            let raw = [0xC8, 0xD5, 0xB1, 0xBE, 0xD5, 0x5A, 0x2E, 0x74, 0x78, 0x74];
            let key = LookupKey::new(&raw, Codec::Gbk).unwrap();
            assert_eq!(key.as_bytes(), &raw, "多字节 trail 字节不得折大小写");
            assert_eq!(src.open(&key).unwrap(), b"j");
            assert_eq!(src.skipped_count(), 0);
        }
        fs::remove_dir_all(&root).ok();
    }
}
