//! 栅格化微基准:分解一次 `render_glyph` 缓存 miss 的耗时构成。
//!
//! ```text
//! cargo run --release -p yu-font --example raster_bench
//! ```

use std::time::Instant;

use yu_font::{AaMode, FontRenderer, FontSource, StylePolicy, TextStyle};

/// 字节缓存在 FontSource 里;为基准单独再枚举一份(语义同主路径)。
fn load_second() -> FontSource {
    FontSource::load_system_fonts().expect("system fonts (second)")
}

fn main() {
    let t0 = Instant::now();
    let source = FontSource::load_system_fonts().expect("system fonts");
    let face = source.default_face().expect("default face");
    println!(
        "enumerate: {} faces, {:?}, default -> {}",
        source.len(),
        t0.elapsed(),
        source.describe_face(face)
    );

    let mut renderer = FontRenderer::new(
        load_second(),
        yu_font::cache::CacheLimits {
            max_entries: 4096,
            max_pixels: 64 * 1024 * 1024,
        },
    );
    for sy in [24u16, 48, 96] {
        let face = renderer.source().default_face().expect("default face");
        let style = TextStyle {
            face,
            sx: sy,
            sy,
            aa: AaMode::Grayscale,
            ..TextStyle::engine_default(face)
        };
        let chars: Vec<char> = "あいうえお漢字テストＡＢ".chars().collect();
        let t1 = Instant::now();
        for ch in &chars {
            let _ = renderer.render_glyph(*ch, &style, StylePolicy::Reference).unwrap();
        }
        let first = t1.elapsed();
        let t2 = Instant::now();
        for ch in &chars {
            let _ = renderer.render_glyph(*ch, &style, StylePolicy::Reference).unwrap();
        }
        let cached = t2.elapsed();
        println!(
            "sy={sy}: {len} glyphs first={} ({:.0}us/glyph) cached={} ({:.2}us/glyph)",
            first.as_nanos() / 1000,
            first.as_nanos() as f64 / 1000.0 / chars.len() as f64,
            cached.as_nanos() / 1000,
            cached.as_nanos() as f64 / 1000.0 / chars.len() as f64,
            len = chars.len(),
        );
    }
}
