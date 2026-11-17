//! 低版本运行核集成测试:0.292 / 0.255 官方语料过 `yu-format` → `yu-rt` 生产路径。
//!
//! 环境变量(缺样本显式 skip 并计数):
//! - `YURIS_SAMPLE_V2_292` = 0.292 编译产物目录
//! - `YURIS_SAMPLE_V2_255` = 0.255 样本解包目录
//!
//! 覆盖:
//! 1. `TableSet::load` 七张表闭合并选中对应 ABI 族;
//! 2. V2 标签 `field_b`(code 偏移)在装载期换成组下标,且落在脚本组范围内;
//! 3. 每个 YSTB 经 `ScriptFile::parse_with_table` 投影,`GroupRec.param == 0`;
//! 4. `code_offsets ↔ pc` 双向一致;
//! 5. 错键 / V5-only 入口对 V2 显式失败。

use std::path::PathBuf;

use yu_rt::tables::{ScriptFile, TableSet};
use yu_vfs::{Codec, DataSource, LookupKey, MemSource};

const KEY_255: [u8; 4] = [0x07, 0xB4, 0x02, 0x4A];

struct Skips(u32);
impl Skips {
    fn skip(&mut self, name: &str, reason: &str) {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {name}: {reason}");
        }
        self.0 += 1;
        println!("SKIP[count={}] {name}: {reason}", self.0);
    }
}

fn dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_dir())
}

/// 语料目录是散盘 ybn(没有 `ysbin/` 前缀);测试侧统一映射到 `ysbin/`。
fn source_of(dir: &std::path::Path) -> MemSource {
    let mut mem = MemSource::new(Codec::Cp932);
    for entry in std::fs::read_dir(dir).expect("read corpus dir") {
        let path = entry.expect("entry").path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy();
        let raw = format!("ysbin/{name}");
        mem.insert(raw.as_bytes(), std::fs::read(&path).expect("read file"))
            .expect("mem insert");
    }
    mem
}

fn check(dir: PathBuf, key: [u8; 4], version: u32, expected_labels: usize, expected_scripts: usize) {
    let src = source_of(&dir);
    let mut tables = TableSet::load(&src, key).expect("V2 表装载");
    assert_eq!(tables.boot.config.version, version);
    assert_eq!(tables.boot.labels.version, version);
    assert_eq!(tables.boot.labels.len(), expected_labels);
    assert_eq!(tables.boot.scripts.len(), expected_scripts);

    // 标签在装载期已换成组下标;逐个装载目标脚本核对范围。
    let labels: Vec<(u16, u32, String)> = tables
        .boot
        .labels
        .iter()
        .map(|l| (l.script_id, l.pc, l.name.clone()))
        .collect();
    assert_eq!(labels.len(), expected_labels);
    for (script_id, pc, name) in &labels {
        let script = tables
            .load_script(&src, u32::from(*script_id))
            .unwrap_or_else(|d| panic!("label {name} 的脚本装载失败: {d}"));
        assert!(
            (*pc as usize) < script.group_count(),
            "标签 {name} pc={pc} 超出脚本 {script_id} 的 {} 组",
            script.group_count()
        );
        assert!(script.is_v2(), "V2 语料的脚本必须走 V2 路径");
    }

    // 全部 YSTB:生产投影 + code_offsets 双向一致。
    let dir_key = LookupKey::new(b"ysbin", Codec::Cp932).unwrap();
    let mut files = 0usize;
    for entry in src.list(&dir_key).unwrap() {
        let name = String::from_utf8_lossy(&entry.name).into_owned();
        if !(name.starts_with("yst0") && name.ends_with(".ybn")) {
            continue;
        }
        let mut raw = b"ysbin/".to_vec();
        raw.extend_from_slice(&entry.name);
        let key = LookupKey::new(&raw, Codec::Cp932).unwrap();
        let bytes = src.open(&key).unwrap();
        if bytes.get(0..4) != Some(b"YSTB") {
            continue;
        }
        files += 1;
        let sid: u32 = name[3..8].parse().unwrap();
        let script =
            ScriptFile::parse_with_table(sid, &bytes, key_of(version), &tables.boot.yscm.format)
                .unwrap_or_else(|d| panic!("{name}: {d}"));
        assert!(script.is_v2());
        // V2 无 gosub_npar:合成视图的 param 必须为 0。
        for pc in 0..script.group_count() {
            assert_eq!(script.group(pc).unwrap().param, 0, "{name} pc={pc} param");
        }
        for offset in 0..script.group_count() as u32 {
            // 用公开查询接口验证 `pc_for_code_offset` 与组下标一致。
            let code_offset = script.code_offset_at(offset as usize).expect("偏移存在");
            assert_eq!(script.pc_for_code_offset(code_offset), Some(offset));
        }
    }
    assert!(files > 0, "V2 语料必须至少有一个 YSTB");

    // V5-only 入口必须拒绝 V2(保留 DIV-009 的反向约束)。
    let first = {
        let dir_key = LookupKey::new(b"ysbin/yst00000.ybn", Codec::Cp932).unwrap();
        src.open(&dir_key).unwrap()
    };
    if first.get(0..4) == Some(b"YSTB") {
        let error = ScriptFile::parse(0, &first, key_of(version)).unwrap_err();
        // `ScriptFile::parse` 是 V5-only 入口;yu-rt 适配层把 `fmt.*`
        // 包装成 `table.incomplete`(原始诊断在 causes 里)。
        assert_eq!(error.code, "table.incomplete", "V5 入口必须拒绝 V2");
    }
}

fn key_of(version: u32) -> [u8; 4] {
    if version < 290 {
        KEY_255
    } else {
        yu_rt::tables::DEFAULT_SCRIPT_KEY
    }
}

#[test]
fn corpus_v2_292_runtime() {
    let mut skips = Skips(0);
    let Some(d) = dir("YURIS_SAMPLE_V2_292") else {
        skips.skip("corpus_v2_292_runtime", "缺少 YURIS_SAMPLE_V2_292");
        assert_eq!(skips.0, 1);
        return;
    };
    check(d, yu_rt::tables::DEFAULT_SCRIPT_KEY, 292, 1189, 116);
}

#[test]
fn corpus_v2_255_runtime() {
    let mut skips = Skips(0);
    let Some(d) = dir("YURIS_SAMPLE_V2_255") else {
        skips.skip("corpus_v2_255_runtime", "缺少 YURIS_SAMPLE_V2_255");
        assert_eq!(skips.0, 1);
        return;
    };
    check(d, KEY_255, 255, 1026, 100);
}

/// 错键:标签转换期走查失败,`TableSet::load` 必须显式报错。
#[test]
fn corpus_v2_wrong_key_is_rejected() {
    let mut skips = Skips(0);
    let Some(d) = dir("YURIS_SAMPLE_V2_292") else {
        skips.skip("corpus_v2_wrong_key_is_rejected", "缺少 YURIS_SAMPLE_V2_292");
        assert_eq!(skips.0, 1);
        return;
    };
    let src = source_of(&d);
    let error = match TableSet::load(&src, [0x00, 0x11, 0x22, 0x33]) {
        Ok(_) => panic!("错键必须显式失败,但表装载成功了"),
        Err(error) => error,
    };
    assert!(
        error.code.starts_with("table.") || error.code.starts_with("fmt."),
        "错键必须显式失败,得到 {}",
        error.code
    );
}
