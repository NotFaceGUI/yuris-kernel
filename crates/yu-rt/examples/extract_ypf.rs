//! 从官方 `data.ypf` 导出条目到散盘目录(仅取证/回归用;不参与运行时)。
//!
//! 用法:
//!   cargo run -p yu-rt --example extract_ypf -- <data.ypf> <out-dir> [prefix]
//!
//! `prefix` 省略 = 全量;常用 `ysbin/`(真实语料测试需要散盘 ysbin)。
//! 导出的是**存储字节**(压缩条目已由 `YpfSource` 解压),路径按包内名字
//! 原样落盘(`/` 分隔),不做任何改名。
use std::path::{Path, PathBuf};

use yu_vfs::{Codec, DataSource, YpfSource};

fn main() {
    let ypf = std::env::args().nth(1).expect("data.ypf");
    let out = PathBuf::from(std::env::args().nth(2).expect("out-dir"));
    let prefix = std::env::args().nth(3).unwrap_or_default();
    let src = YpfSource::open(&ypf, Codec::Cp932).expect("open ypf");
    let mut written = 0usize;
    let mut bytes = 0u64;
    for entry in src.entries() {
        let name = String::from_utf8_lossy(&entry.name).replace('\\', "/");
        if !prefix.is_empty() && !name.starts_with(&prefix) {
            continue;
        }
        let data = src.open(&entry.normalized).expect("read entry");
        let path: PathBuf = name.split('/').fold(out.clone(), |p, seg| p.join(seg));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&path, &data).expect("write");
        written += 1;
        bytes += data.len() as u64;
    }
    println!(
        "extracted {written} entries ({bytes} bytes) from {} to {}",
        Path::new(&ypf).display(),
        out.display()
    );
}
