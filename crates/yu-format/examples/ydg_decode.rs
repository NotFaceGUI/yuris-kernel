//! 取证探针:解码一个本地 `.ydg`(或裸 `.qoi`),打印画布/瓦片与像素摘要。
//!
//! 用法:`ydg_decode <file.ydg|file.qoi>`
//!
//! 只依赖 `yu-format`,不启动引擎/渲染器;用于把 YDG/QOI 解码问题从
//! 资产查找/渲染链里隔离出来。

use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("file.ydg|file.qoi");
    let bytes = std::fs::read(&path).expect("read");
    if yu_format::ydg::is_ydg(&bytes) {
        let ydg = yu_format::ydg::parse(&bytes).expect("parse ydg");
        println!(
            "YDG canvas {}x{} tiles={}",
            ydg.width,
            ydg.height,
            ydg.frames.len()
        );
        let mut y = 0u32;
        let mut total = Vec::new();
        for (i, frame) in ydg.frames.iter().enumerate() {
            println!(
                " tile {i}: codec={:?} h={} bytes={}",
                frame.codec,
                frame.height,
                frame.bytes.len()
            );
            if frame.codec == yu_format::ydg::YdgCodec::Qoi {
                let img = yu_format::ydg::decode_qoi(frame.bytes, (ydg.width, frame.height))
                    .expect("decode qoi");
                println!(
                    "  decoded {}x{} first={:02x?}",
                    img.width,
                    img.height,
                    &img.rgba[..8.min(img.rgba.len())]
                );
                total.extend_from_slice(&img.rgba);
                y += img.height;
            }
        }
        println!("assembled {y} rows, {} bytes", total.len());
        let sum: u64 = total.iter().map(|&b| u64::from(b)).sum();
        println!("rgba byte sum = {sum}");
    } else if bytes.starts_with(b"qoif") && bytes.len() >= 14 {
        let w = u16::try_from(u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
            .expect("qoi width");
        let h = u16::try_from(u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]))
            .expect("qoi height");
        match yu_format::ydg::decode_qoi(&bytes, (w, h)) {
            Ok(img) => println!("QOI {}x{} first={:02x?}", img.width, img.height, &img.rgba[..8]),
            Err(diag) => println!("QOI decode failed: {} {}", diag.code, diag.detail),
        }
    } else {
        println!("not YDG/QOI: {}", Path::new(&path).display());
    }
}
