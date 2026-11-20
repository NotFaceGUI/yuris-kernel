//! 合成工程上的行为断言:事件流 golden、变量终值、错误路径。
//!
//! 所有用例都走**生产路径**(`Vm::boot` → `TableSet::load` → `ScriptFile::parse`),
//! 只是数据换成合成夹具。这样语义与真实语料共享同一批处理器。

mod support;

// 子模块文件在 `tests/fixture/` 下:内联模块名 `fixture` 提供目录层级,
// 因此不需要 `#[path]`(标准模块解析)。
mod fixture {
    mod common;
    mod control;
    mod file_save;
    mod media;
    mod system;
    mod text;
    mod variables;
}
