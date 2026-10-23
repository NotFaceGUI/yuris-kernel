//! 官方存档自证(真值层级 3):parse → rebuild 对官方 `.sd` **逐字节一致**。
//!
//! 语料:`target/tmp/yssd-corpus/*.sd` —— `<project-eris>/save/` 的**快照**
//! (该目录是官方 exe 的活动写盘位,文件会被游戏运行改写;快照保证断言
//! 有稳定靶子。快照缺失时回退 live 目录并打印来源)。覆盖
//! extra/gsd/kidoku/save50001/seld 共 269 块,含 type 0(单变量)/
//! type 1(配置)/ type 2(多变量)三种块与 zlib level-1 重编码。

use std::path::PathBuf;

use yu_format::yssd::{
    encode_block_body, encode_multi_body, parse_multi_payload, YssdConfigPayload, YssdSave,
    YssdSubBlockRaw, YssdValue, YssdWriter,
};

fn corpus_dir() -> Option<(PathBuf, &'static str)> {
    if let Ok(dir) = std::env::var("YURIS_SAVE_CORPUS") {
        return Some((PathBuf::from(dir), "YURIS_SAVE_CORPUS"));
    }
    let root = std::env::var("YURIS_SAMPLE_ERIS_ROOT").ok()?;
    let snapshot = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("yssd-corpus");
    if snapshot.is_dir() {
        return Some((snapshot, "target/tmp 快照"));
    }
    Some((PathBuf::from(root).join("save"), "live save/"))
}

const OFFICIAL_FILES: [&str; 5] = ["extra.sd", "gsd.sd", "kidoku.sd", "save50001.sd", "seld.sd"];

#[test]
fn official_sd_files_roundtrip_byte_exact() {
    let Some((dir, source)) = corpus_dir() else {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but no save corpus dir is set");
        }
        println!("SKIP[yssd] 无 YURIS_SAVE_CORPUS / YURIS_SAMPLE_ERIS_ROOT");
        return;
    };

    let mut total_blocks = 0usize;
    let mut total_values = 0usize;
    for name in OFFICIAL_FILES {
        let path = dir.join(name);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| panic!("读 {} 失败: {e}", path.display()));
        let origin = name.as_bytes();
        let save = YssdSave::parse(&bytes, origin).unwrap_or_else(|d| panic!("{name}: {d}"));

        // 1) 容器级重建:type 0/1 经 compress2(level=1) 重编码;type 2 外层
        //    保持裸多子块、内层子块各自重压缩;块按 DNO 升序落盘 ⇒ 整文件
        //    逐字节一致(头/偏移表/块序/压缩流全对齐)。
        let mut writer = YssdWriter::default();
        for entry in &save.entries {
            let entry_dno = entry.dno;
            let body = match entry.btype {
                0 => {
                    let raw = entry
                        .decompress(origin)
                        .unwrap_or_else(|d| panic!("{name}: {d}"));
                    assert_eq!(raw.len() as u64, u64::from(entry.ulen), "{name} ulen 闭合");
                    let value = YssdValue::parse(&raw, origin, entry.dno)
                        .unwrap_or_else(|d| panic!("{name}: {d}"));
                    assert_eq!(value.to_bytes(), raw, "{name} DNO={} 值载荷往返", entry.dno);
                    total_values += 1;
                    encode_block_body(entry.btype, entry.strict, entry.var_id, &raw)
                }
                1 => {
                    let raw = entry
                        .decompress(origin)
                        .unwrap_or_else(|d| panic!("{name}: {d}"));
                    let cfg = YssdConfigPayload::parse(&raw, origin, entry.dno)
                        .unwrap_or_else(|d| panic!("{name}: {d}"));
                    assert_eq!(cfg.to_bytes(), raw, "{name} DNO={} 配置载荷往返", entry.dno);
                    encode_block_body(entry.btype, entry.strict, entry.var_id, &raw)
                }
                2 => {
                    let raw = &entry.compressed;
                    let subs = parse_multi_payload(raw, origin, entry.dno)
                        .unwrap_or_else(|d| panic!("{name}: {d}"));
                    for sub in &subs {
                        assert_eq!(sub.pad, 0x0100, "{name} DNO={} pad 恒值", entry.dno);
                    }
                    let raws: Vec<Vec<u8>> = subs
                        .iter()
                        .map(|sub| {
                            sub.decompress(origin, entry.dno)
                                .unwrap_or_else(|d| panic!("{name}: {d}"))
                        })
                        .collect();
                    let rebuild: Vec<YssdSubBlockRaw<'_>> = subs
                        .iter()
                        .zip(&raws)
                        .map(|(sub, raw)| YssdSubBlockRaw {
                            var_id: sub.var_id,
                            raw,
                        })
                        .collect();
                    encode_multi_body(&rebuild)
                }
                other => panic!("{name} DNO={entry_dno}: 未知块类型 {other}"),
            };
            writer.set_block(entry.dno, body);
            total_blocks += 1;
        }
        let rebuilt = writer.build();
        assert_eq!(rebuilt, bytes, "{name}: 容器重建与官方产物逐字节不一致");
        println!("{name}: {} 块,roundtrip OK", save.entries.len());
    }
    println!("语料来源:{source}({})", dir.display());
    println!("合计: {total_blocks} 块 / {total_values} 值载荷,全部逐字节一致");
    assert_eq!(total_blocks, 269, "官方 5 文件快照的已知块数");
}
