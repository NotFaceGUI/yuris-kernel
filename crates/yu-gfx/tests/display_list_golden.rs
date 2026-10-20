//! 小型合成 fixture 的显示列表 golden(字段顺序/数值格式/分组顺序)。

mod support;

use std::path::PathBuf;

use support::*;
use yu_gfx::json;

fn write_or_compare(actual: &str, name: &str) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join(name);
    if std::env::var("UPDATE_GFX_GOLDEN").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(path.parent().unwrap()).expect("golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "missing golden {} ({e}); run with UPDATE_GFX_GOLDEN=1",
            path.display()
        )
    });
    assert_eq!(actual, expected);
}

#[test]
fn synthetic_group_layers_display_golden() {
    let table = table();
    let mut gfx = state();
    gfx.consume_events(&[
        frame_start(1),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/")),
                arg(&table, "CG", "SX", i(400)),
                arg(&table, "CG", "SY", i(300)),
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "MODE", i(1)),
                arg(&table, "CG", "Z", i(10)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/BG")),
                arg(&table, "CG", "FILE", s("cg/bg")),
                arg(&table, "CG", "Z", i(1)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/SPR")),
                arg(&table, "CG", "FILE", s("cg/spr")),
                arg(&table, "CG", "Z", i(2)),
                arg(&table, "CG", "A", i(128)),
                arg(&table, "CG", "X", i(10)),
                arg(&table, "CG", "Y", i(20)),
                arg(&table, "CG", "MODE", i(2)),
            ],
        ),
        call(
            1,
            "CG",
            vec![
                arg(&table, "CG", "ID", s("G/TR")),
                arg(&table, "CG", "FILE", s("cg/a")),
                arg(&table, "CG", "T", i(128)),
                arg(&table, "CG", "TA", i(256)),
                arg(&table, "CG", "TID", s("G/SPR")),
                arg(&table, "CG", "Z", i(3)),
            ],
        ),
        frame_end(1),
    ]);
    let f = gfx.frame();
    let a = json::to_display_json(&f);
    let b = json::to_display_json(&f);
    assert_eq!(a, b);
    write_or_compare(&a, "synthetic_group_layers_display.json");
}
