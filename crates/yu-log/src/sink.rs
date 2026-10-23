//! 日志汇。显式注入,不设全局单例(A3: 确定性,无隐藏状态)。

use crate::Diag;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// 诊断输出目标。
pub trait LogSink {
    /// 输出一条诊断。失败时由调用方决定是否升级为致命错误。
    fn emit(&mut self, diag: &Diag) -> io::Result<()>;

    /// 刷出缓冲。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// JSONL 文件汇: 一行一条确定性 JSON。
pub struct JsonlSink<W: Write> {
    writer: W,
}

impl<W: Write> JsonlSink<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub fn into_inner(self) -> W {
        self.writer
    }
}

impl JsonlSink<BufWriter<File>> {
    /// 创建/截断日志文件。
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::create(path)?;
        Ok(Self::new(BufWriter::new(file)))
    }
}

impl<W: Write> LogSink for JsonlSink<W> {
    fn emit(&mut self, diag: &Diag) -> io::Result<()> {
        self.writer.write_all(diag.to_json_line().as_bytes())?;
        self.writer.write_all(b"\n")
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// 内存汇(测试与短生命周期装配用)。
#[derive(Default)]
pub struct MemorySink {
    records: Vec<Diag>,
}

impl MemorySink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> &[Diag] {
        &self.records
    }

    pub fn take(&mut self) -> Vec<Diag> {
        std::mem::take(&mut self.records)
    }

    /// 丢弃全部记录但保留容量(无人消费时的低成本排空)。
    pub fn clear(&mut self) {
        self.records.clear();
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

impl LogSink for MemorySink {
    fn emit(&mut self, diag: &Diag) -> io::Result<()> {
        self.records.push(diag.clone());
        Ok(())
    }
}

/// 丢弃所有输出。
pub struct NullSink;

impl LogSink for NullSink {
    fn emit(&mut self, _diag: &Diag) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{code, Boundary, Diag};

    fn diag(msg: &str) -> Diag {
        Diag::recoverable(code::vfs::MISSING, Boundary::Data).with_detail(msg)
    }

    #[test]
    fn memory_sink_collects_and_drains() {
        let mut sink = MemorySink::new();
        sink.emit(&diag("a")).unwrap();
        sink.emit(&diag("b")).unwrap();
        assert_eq!(sink.len(), 2);
        assert_eq!(sink.take().len(), 2);
        assert!(sink.is_empty());
    }

    #[test]
    fn jsonl_sink_writes_parseable_lines() {
        let mut sink = JsonlSink::new(Vec::<u8>::new());
        sink.emit(&diag("a")).unwrap();
        sink.emit(&diag("b")).unwrap();
        sink.flush().unwrap();
        let bytes = sink.into_inner();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let parsed = Diag::from_json_line(line).unwrap();
            assert_eq!(parsed.code, code::vfs::MISSING);
        }
    }
}
