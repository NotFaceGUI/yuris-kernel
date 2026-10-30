//! 探针:验证 `CGACT[TEXT SET=n]` 的多遍合成在 CPU 文本链上真的画出
//! 偏移环(影子/轮廓),并把结果打成 ASCII 图。
use yu_gfx::{LayerId, TextStyle, TextDraw};
use yu_render_core::text::TextComposer;

fn draw(color: u32, set: i64) -> TextDraw {
    TextDraw {
        target: LayerId::new(b"T".to_vec()),
        bytes: b"A".to_vec(),
        x: 8,
        y: 4,
        style: TextStyle {
            color,
            color2: Some(color),
            size_x: 24,
            size_y: 24,
            ..TextStyle::default()
        },
        set,
        bno: None,
    }
}

fn main() {
    let (w, h) = (48u32, 40u32);
    for (label, draws) in [
        ("only main (SET=0)", vec![draw(0x00_FFFF, 0)]),
        ("shadow black SET=3 + main", vec![draw(0x00_0000, 3), draw(0x00_FFFF, 0)]),
        ("shadow black SET=2 + main", vec![draw(0x00_0000, 2), draw(0x00_FFFF, 0)]),
    ] {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let mut tc = TextComposer::new();
        let stats = tc.paint_draws(&mut buf, w, h, &draws);
        println!(
            "== {label}: draws={} glyphs={} missing={} notes={:?}",
            stats.draws, stats.glyphs, stats.missing, stats.diags
        );
        for y in 0..h {
            let mut line = String::new();
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let (r, g, b, a) = (buf[i], buf[i + 1], buf[i + 2], buf[i + 3]);
                line.push(match (a, r, g, b) {
                    (0, ..) => '.',
                    (_, 0, 0, 0) => '#',       // 黑(影子)
                    (_, _, g, b) if g > 100 && b > 100 => 'C', // 青(正文)
                    _ => '+',
                });
            }
            println!("{line}");
        }
        println!();
    }
}
