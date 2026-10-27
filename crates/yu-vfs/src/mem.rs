//! 内存数据源(测试夹具与 wasm 前端用)。

use crate::{missing, Codec, DataSource, DirTree, Entry, LookupKey};
use std::collections::BTreeMap;
use yu_log::Diag;

/// 纯内存文件树。路径在插入时归一一次。
pub struct MemSource {
    codec: Codec,
    files: BTreeMap<LookupKey, Vec<u8>>,
    tree: DirTree,
}

impl MemSource {
    pub fn new(codec: Codec) -> Self {
        Self {
            codec,
            files: BTreeMap::new(),
            tree: DirTree::new(),
        }
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// 插入一个文件;父目录自动派生。重复插入同一归一键覆盖旧内容(叠加栈语义)。
    pub fn insert(&mut self, raw_path: &[u8], bytes: Vec<u8>) -> Result<(), Diag> {
        let key = LookupKey::new(raw_path, self.codec)?;
        if key.is_root() {
            return Err(crate::collision(
                &key,
                "mem",
                "cannot insert a file at the mount root",
            ));
        }
        self.tree.insert_file(&key, bytes.len() as u64)?;
        self.files.insert(key, bytes);
        Ok(())
    }

    /// 显式登记一个空目录。`.ypf` 包无法表达空目录,故等价性测试不覆盖此形态。
    pub fn insert_dir(&mut self, raw_path: &[u8]) -> Result<(), Diag> {
        let key = LookupKey::new(raw_path, self.codec)?;
        self.tree.insert_dir(&key)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl DataSource for MemSource {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| missing(path, self.source_name()))
    }    fn exists(&self, path: &LookupKey) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        self.tree.list(path, self.source_name())
    }

    fn source_name(&self) -> &str {
        "mem"
    }
}

/// 可共享写入的内存叠层(运行时合成资产)。
///
/// 用途:`LOAD[ID]` 把存档缩略图装回图层时,内核把它编码成图像写进这个
/// 叠层;挂载栈里每个 `TextureStore` 与 VM 的资产解析都看到同一份内容。
/// 键是内容哈希,重复写入字节一致、天然幂等;`exists/list` 走内存树。
#[derive(Clone)]
pub struct SharedMem {
    inner: std::sync::Arc<std::sync::Mutex<MemSource>>,
}

impl std::fmt::Debug for SharedMem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedMem").field("len", &self.len()).finish()
    }
}

impl SharedMem {
    pub fn new(codec: Codec) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(MemSource::new(codec))),
        }
    }

    /// 写入一个文件(路径归一;重复键覆盖)。
    pub fn insert(&self, raw_path: &[u8], bytes: Vec<u8>) -> Result<(), Diag> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.insert(raw_path, bytes)
    }

    /// 已写入的条目数(诊断 / 用例断言)。
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl DataSource for SharedMem {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .open(path)
    }

    fn exists(&self, path: &LookupKey) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .exists(path)
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .list(path)
    }

    fn source_name(&self) -> &str {
        "mem-overlay"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yu_log::code;

    #[test]
    fn insert_and_read_with_case_folding() {
        let mut src = MemSource::new(Codec::Cp932);
        src.insert(br"CG\Ev\A.PNG", vec![1, 2, 3]).unwrap();
        let key = LookupKey::new(b"cg/ev/a.png", Codec::Cp932).unwrap();
        assert_eq!(src.open(&key).unwrap(), vec![1, 2, 3]);
        assert!(src.exists(&key));
        let root = src.list(&LookupKey::root()).unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, b"cg");
    }

    #[test]
    fn missing_entry_is_recoverable() {
        let src = MemSource::new(Codec::Cp932);
        let key = LookupKey::new(b"nope", Codec::Cp932).unwrap();
        let diag = src.open(&key).unwrap_err();
        assert_eq!(diag.code, code::vfs::MISSING);
        assert!(!src.exists(&key));
    }

    #[test]
    fn explicit_empty_dir_is_listable() {
        let mut src = MemSource::new(Codec::Cp932);
        src.insert_dir(b"empty").unwrap();
        let key = LookupKey::new(b"empty", Codec::Cp932).unwrap();
        assert!(src.list(&key).unwrap().is_empty());
    }
}
