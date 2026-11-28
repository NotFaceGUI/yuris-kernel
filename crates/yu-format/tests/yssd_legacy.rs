//! 0.263 官方系统存档自证(真值层级 3):旧式记录容器的 parse → rebuild
//! **逐字节一致**,并核对记录 id / 值类型与维数。
//!
//! 语料:`<games>/<0.263 商业>/save/{gsd,kidoku,seld}.sd`(引擎 0.263,
//! 容器版本 0x107,头 `+0x0C = 0`)。三份文件都从 `+0x10` 起按
//! `id u32 + btype u8 + strict u8 + var u16 + 未压缩 YssdValue` 逐条解析,
//! 恰好走到文件尾 —— 这就是旧式布局的结构证据。
//!
//! 环境变量:`YURIS_SAMPLE_V2_263_SAVE`(save 目录)。缺样本时打印 SKIP;
//! `YURIS_REQUIRE_SAMPLES=1` 时改为失败(CI 口径)。

use std::path::PathBuf;

use yu_format::yssd::{YssdLayout, YssdLegacyWriter, YssdSave, YssdValue, YssdValueType};

const FILES: [&str; 3] = ["gsd.sd", "kidoku.sd", "seld.sd"];

fn save_dir() -> Option<PathBuf> {
    std::env::var("YURIS_SAMPLE_V2_263_SAVE")
        .ok()
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
}

fn dir_or_skip() -> Option<PathBuf> {
    if let Some(dir) = save_dir() {
        return Some(dir);
    }
    if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
        panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_V2_263_SAVE is not set/valid");
    }
    println!("SKIP[yssd-legacy] 无 YURIS_SAMPLE_V2_263_SAVE");
    None
}

#[test]
fn real_0263_system_saves_roundtrip_byte_exact() {
    let Some(dir) = dir_or_skip() else { return };
    for name in FILES {
        let path = dir.join(name);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        let save = YssdSave::parse(&bytes, name.as_bytes()).unwrap_or_else(|d| {
            panic!("{name}: 旧式容器解析失败: {d}");
        });
        assert_eq!(save.layout, YssdLayout::Legacy, "{name}: 布局应为旧式记录表");
        assert_eq!(save.version, 0x107, "{name}: 0.263 容器版本");
        // 记录 id 严格升序(parse 已校验,这里再钉住官方产物的事实)。
        let ids: Vec<u32> = save.entries.iter().map(|e| e.dno).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]), "{name}: id 非升序 {ids:?}");
        // rebuild(保留 btype/strict/var 与未压缩载荷)必须逐字节一致。
        let mut writer = YssdLegacyWriter::new(save.version);
        for entry in &save.entries {
            let raw = entry
                .decompress(name.as_bytes())
                .unwrap_or_else(|d| panic!("{name} DNO={}: {d}", entry.dno));
            writer.set_block(entry.dno, entry.btype, entry.strict, entry.var_id, raw);
        }
        assert_eq!(writer.build(), bytes, "{name}: rebuild 不是逐字节一致");
    }
}

#[test]
fn real_0263_system_save_shapes() {
    let Some(dir) = dir_or_skip() else { return };

    let gsd = read_legacy(&dir, "gsd.sd");
    assert_eq!(
        gsd.entries.iter().map(|e| e.dno).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 11, 12, 13],
        "gsd 记录 id(001..004 + 011..013)"
    );

    let kidoku = read_legacy(&dir, "kidoku.sd");
    let value = only_value(&kidoku, "kidoku.sd");
    assert_eq!(value.ty, YssdValueType::Int);
    assert_eq!(value.dims, vec![256, 160], "既读标志 INT[256,160]");

    let seld = read_legacy(&dir, "seld.sd");
    let value = only_value(&seld, "seld.sd");
    assert_eq!(value.ty, YssdValueType::Int);
    assert_eq!(value.dims, vec![1000], "选择数据 INT[1000]");
    assert_eq!(value.data.len(), 8000);
}

fn read_legacy(dir: &std::path::Path, name: &str) -> YssdSave {
    let path = dir.join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    YssdSave::parse(&bytes, name.as_bytes()).unwrap_or_else(|d| panic!("{name}: {d}"))
}

fn only_value(save: &YssdSave, name: &str) -> YssdValue {
    assert_eq!(save.entries.len(), 1, "{name}: 应只有一条记录");
    let entry = &save.entries[0];
    let raw = entry.decompress(name.as_bytes()).expect("decompress");
    YssdValue::parse(&raw, name.as_bytes(), entry.dno).expect("value")
}
