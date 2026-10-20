//! Style golden hashes.

mod support;

use yu_font::cache::CacheLimits;
use yu_font::{
    AaMode, FontRenderer, Gradation, Italic, Outline, Rgb, Rotation, Shadow, StylePolicy,
};

struct Case {
    name: &'static str,
    ch: char,
    mutate: fn(&mut yu_font::TextStyle),
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "solid_white_20",
            ch: 'あ',
            mutate: |_| {},
        },
        Case {
            name: "bold_20",
            ch: 'あ',
            mutate: |s| s.bold = true,
        },
        Case {
            name: "italic_shear12_20",
            ch: 'A',
            mutate: |s| s.italic = Italic::Synthetic { degrees: 12.0 },
        },
        Case {
            name: "aa_off_20",
            ch: 'あ',
            mutate: |s| s.aa = AaMode::None,
        },
        Case {
            name: "outline1_red_20",
            ch: 'あ',
            mutate: |s| s.outline = Some(Outline::new(1, Rgb::new(255, 0, 0))),
        },
        Case {
            name: "outline3_red_20",
            ch: 'あ',
            mutate: |s| s.outline = Some(Outline::new(3, Rgb::new(255, 0, 0))),
        },
        Case {
            name: "shadow_m3m4_20",
            ch: 'A',
            mutate: |s| s.shadow = Some(Shadow::new(-3, -4, Rgb::new(0, 0, 0))),
        },
        Case {
            name: "gradient_red_blue_20",
            ch: '口',
            mutate: |s| {
                s.color = Rgb::new(255, 0, 0);
                s.color2 = Rgb::new(0, 0, 255);
                s.gradation = Gradation::Direction { octant: 1 };
            },
        },
        Case {
            name: "rotation45_20",
            ch: 'A',
            mutate: |s| s.rotation = Rotation::Octant(1),
        },
        Case {
            name: "sx16_sy24",
            ch: 'あ',
            mutate: |s| {
                s.sx = 16;
                s.sy = 24;
            },
        },
    ]
}

#[test]
fn style_golden_hashes_are_stable() {
    let Some(font) = support::load_test_font() else {
        return;
    };
    let face = font.face;
    let mut r = FontRenderer::new(font.source, CacheLimits::default());
    let mut lines = Vec::new();
    for case in cases() {
        let mut style = yu_font::TextStyle::engine_default(face);
        style.sx = 20;
        style.sy = 20;
        (case.mutate)(&mut style);
        let img = r
            .render_glyph(case.ch, &style, StylePolicy::Reference)
            .unwrap();
        lines.push(format!(
            "{} size={}x{} left={} top={} adv={} hash={:016x} mass={}",
            case.name,
            img.width,
            img.height,
            img.left,
            img.top,
            img.advance,
            img.hash64(),
            img.alpha_mass()
        ));
    }
    let text = lines.join("\n");
    println!("{text}");
    let golden = include_str!("golden/styles.txt");
    assert_eq!(
        text.trim(),
        golden.trim(),
        "style golden changed; update tests/golden/styles.txt only with evidence"
    );
}
