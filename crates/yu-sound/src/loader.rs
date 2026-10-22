//! 装载抽象：`yu-sound` 不碰文件系统、不认识 ypf/包格式。
//!
//! 装配层（`yu-cli`）注入一个 VFS 实现；扩展名探测序是**引擎语义**，
//! 由本 crate 驱动：`(无扩展名) → .OGG → .WAV`（手册 `sound/sound.html`）。

use yu_log::Diag;

/// 按工程内相对路径读取文件；`Ok(None)` = 不存在。
pub trait SoundLoader {
    fn load(&self, name: &[u8]) -> Result<Option<Vec<u8>>, Diag>;
}

/// 内存装载器（单测/golden 用）。
#[derive(Default)]
pub struct MemoryLoader {
    files: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
}

impl MemoryLoader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<Vec<u8>>, bytes: impl Into<Vec<u8>>) {
        self.files.insert(name.into(), bytes.into());
    }
}

impl SoundLoader for MemoryLoader {
    fn load(&self, name: &[u8]) -> Result<Option<Vec<u8>>, Diag> {
        Ok(self.files.get(name).cloned())
    }
}
