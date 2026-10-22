//! 边界①:任意 [`yu_vfs::DataSource`] 的录制包装器。
//!
//! 包装器只记录**调用事实与结果身份**,不改变任何语义:
//! - `open` 成功 → `bytes` 长度 + 内容 SHA-256;失败 → 稳定错误码;
//! - `exists` → 布尔结果;
//! - `list` → 返回顺序原样保存的目录项(名字字节/种类/大小),失败 → 错误码。
//!
//! 返回值与错误对象与内层数据源**完全相同**(错误 `Diag` 逐字段克隆),
//! 这是 D2 等价性测试的契约。

use std::sync::{Arc, Mutex};

use yu_log::Diag;
use yu_vfs::{DataSource, Entry, LookupKey};

use crate::event::{EventRecorder, SharedLog};
use crate::hash::sha256_hex;
use crate::record::{ListEntry, Payload};

/// 帧号句柄。包装器被装箱后,宿主仍可通过它标注当前逻辑帧。
pub type FrameHandle = Arc<Mutex<Option<u64>>>;

/// 录制包装器。持有内层数据源,日志通过 [`SharedLog`] 共享。
pub struct TracingSource<D: DataSource> {
    inner: D,
    recorder: EventRecorder,
    source_name: String,
    /// `None` = 帧外读取(启动链)。
    frame: FrameHandle,
}

impl<D: DataSource> TracingSource<D> {
    pub fn new(inner: D, log: SharedLog) -> Self {
        let source_name = inner.source_name().to_string();
        Self {
            inner,
            recorder: EventRecorder::new(log),
            source_name,
            frame: Arc::new(Mutex::new(None)),
        }
    }

    pub fn from_recorder(inner: D, recorder: EventRecorder) -> Self {
        let source_name = inner.source_name().to_string();
        Self {
            inner,
            recorder,
            source_name,
            frame: Arc::new(Mutex::new(None)),
        }
    }

    /// 与另一个录制源共享同一个帧号句柄(主源与 aux 源必须一致)。
    pub fn from_recorder_with_frame(inner: D, recorder: EventRecorder, frame: FrameHandle) -> Self {
        let source_name = inner.source_name().to_string();
        Self {
            inner,
            recorder,
            source_name,
            frame,
        }
    }

    /// 设定后续读取的逻辑帧号(`None` = 帧外)。
    ///
    /// 启动链读取应保持 `None`;帧内读取由宿主在推进前设置。
    pub fn set_frame(&self, frame: Option<u64>) {
        *self.frame.lock().unwrap() = frame;
    }

    /// 可跨装箱持有的帧号句柄。
    pub fn frame_handle(&self) -> FrameHandle {
        Arc::clone(&self.frame)
    }

    pub fn inner(&self) -> &D {
        &self.inner
    }

    pub fn recorder(&self) -> &EventRecorder {
        &self.recorder
    }

    pub fn into_inner(self) -> D {
        self.inner
    }
}

impl<D: DataSource> DataSource for TracingSource<D> {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        match self.inner.open(path) {
            Ok(bytes) => {
                self.recorder.shared_log().lock().unwrap().push(
                    *self.frame.lock().unwrap(),
                    Payload::VfsOpen {
                        source: self.source_name.clone(),
                        key: path.as_bytes().to_vec(),
                        bytes: Some(bytes.len() as u64),
                        sha256: Some(sha256_hex(&bytes)),
                        diag: None,
                    },
                );
                Ok(bytes)
            }
            Err(diag) => {
                self.recorder.shared_log().lock().unwrap().push(
                    *self.frame.lock().unwrap(),
                    Payload::VfsOpen {
                        source: self.source_name.clone(),
                        key: path.as_bytes().to_vec(),
                        bytes: None,
                        sha256: None,
                        diag: Some(diag.code.to_string()),
                    },
                );
                Err(diag)
            }
        }
    }

    fn exists(&self, path: &LookupKey) -> bool {
        let exists = self.inner.exists(path);
        self.recorder.shared_log().lock().unwrap().push(
            *self.frame.lock().unwrap(),
            Payload::VfsExists {
                source: self.source_name.clone(),
                key: path.as_bytes().to_vec(),
                exists,
            },
        );
        exists
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        match self.inner.list(path) {
            Ok(entries) => {
                let recorded: Vec<ListEntry> = entries.iter().map(ListEntry::from_entry).collect();
                self.recorder.shared_log().lock().unwrap().push(
                    *self.frame.lock().unwrap(),
                    Payload::VfsList {
                        source: self.source_name.clone(),
                        key: path.as_bytes().to_vec(),
                        entries: recorded,
                        diag: None,
                    },
                );
                Ok(entries)
            }
            Err(diag) => {
                self.recorder.shared_log().lock().unwrap().push(
                    *self.frame.lock().unwrap(),
                    Payload::VfsList {
                        source: self.source_name.clone(),
                        key: path.as_bytes().to_vec(),
                        entries: Vec::new(),
                        diag: Some(diag.code.to_string()),
                    },
                );
                Err(diag)
            }
        }
    }

    fn source_name(&self) -> &str {
        &self.source_name
    }
}

/// `Box<dyn DataSource>` 适配器。
///
/// 孤儿规则不允许为外部 trait + 外部类型写 blanket impl,因此用本 crate 的
/// newtype 承接已装箱的数据源,再交给 [`TracingSource`] 包装。
pub struct BoxedSource(Box<dyn DataSource>);

impl BoxedSource {
    pub fn new(source: Box<dyn DataSource>) -> Self {
        Self(source)
    }

    pub fn into_inner(self) -> Box<dyn DataSource> {
        self.0
    }
}

impl DataSource for BoxedSource {
    fn open(&self, path: &LookupKey) -> Result<Vec<u8>, Diag> {
        self.0.open(path)
    }

    fn exists(&self, path: &LookupKey) -> bool {
        self.0.exists(path)
    }

    fn list(&self, path: &LookupKey) -> Result<Vec<Entry>, Diag> {
        self.0.list(path)
    }

    fn source_name(&self) -> &str {
        self.0.source_name()
    }
}
