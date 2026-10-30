//! 官方存档自证(真值层级 3):parse → rebuild 对官方 `.sd` **逐字节一致**。
//!
//! 语料:`target/tmp/yssd-corpus/*.sd` —— `<project-eris>/save/` 的**快照**
//! (该目录是官方 exe 的活动写盘位,文件会被游戏运行改写;快照保证断言
//! 有稳定靶子。快照缺失时回退 live 目录并打印来源)。覆盖
//! extra/gsd/kidoku/save50001/seld 共 269 块,含 type 0(单变量)/
//! type 1(配置)/ type 2(多变量)三种块与 zlib level-1 重编码。

use std::path::PathBuf;

use yu_format::yssd::{
    encode_block_body, encode_block_body_with, encode_multi_body, parse_multi_payload, BlockCodec,
    YssdConfigPayload, YssdSave, YssdSubBlockRaw, YssdValue, YssdWriter,
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
        // 沿用源文件版本(0x138/0x1E0/0x1E1 都可能;不写死常量)。
        let mut writer = YssdWriter::new(save.version);
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

/// 商业包(0.481)存档:逐块解出原字节 → 用 Snappy 重压 → 再解,必须回到原值。
///
/// 覆盖三种块形态:
/// - `btype 0/1`:单变量/配置块,载荷 = 一层压缩流(本代 = Snappy);
/// - `btype 2`:多变量块,**外层裸多子块流、内层子块各自压缩**(所以外层不
///   当压缩流解);
/// - `DNO=1024`:嵌套容器槽 —— `u32 inner_len | u32 len_b | zlib(内层 YSSD)`,
///   解出的内层是完整的 `YSSD` 容器(见 `docs/divergences/DIV-RT-056`)。
#[test]
fn commercial_save_blocks_recompress_as_snappy() {
    let Some(root) = std::env::var_os("YURIS_SAMPLE_PAC481") else {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_PAC481 is unset");
        }
        println!("SKIP[yssd-snappy] 无 YURIS_SAMPLE_PAC481");
        return;
    };
    let dir = PathBuf::from(root).join("save");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        println!("SKIP[yssd-snappy] {} 不可读", dir.display());
        return;
    };
    let (mut files, mut plain, mut multi, mut nested) = (0usize, 0usize, 0usize, 0usize);
    let (mut snappy_blocks, mut zlib_blocks) = (0usize, 0usize);
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sd") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let origin = path.to_string_lossy().into_owned();
        let Ok(save) = YssdSave::parse(&bytes, origin.as_bytes()) else {
            continue;
        };
        files += 1;
        for entry in &save.entries {
            let o = origin.as_bytes();
            // 嵌套容器槽:8 字节头 + zlib(内层 YSSD)。
            if entry.dno == 1024 {
                let head = &entry.compressed;
                assert!(head.len() > 8, "{origin}: DNO1024 载荷过短");
                let inner_len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
                let raw = yu_format::yssd::zlib_decompress(
                    &head[8..],
                    inner_len as u32,
                    o,
                    entry.dno,
                )
                .unwrap_or_else(|d| panic!("{origin}: DNO1024 内层解压失败 {d:?}"));
                assert_eq!(raw.len(), inner_len, "{origin}: DNO1024 长度头不符");
                assert_eq!(&raw[..4], b"YSSD", "{origin}: 内层应为 YSSD 容器");
                nested += 1;
                continue;
            }
            match entry.btype {
                2 => {
                    let subs = parse_multi_payload(&entry.compressed, o, entry.dno)
                        .unwrap_or_else(|d| panic!("{origin} DNO{}: {d:?}", entry.dno));
                    for sub in &subs {
                        sub.decompress(o, entry.dno)
                            .unwrap_or_else(|d| panic!("{origin} DNO{}: {d:?}", entry.dno));
                    }
                    // 内层子块重压 → 再解,逐字节回到原值。
                    let raws: Vec<(u16, Vec<u8>)> = subs
                        .iter()
                        .map(|sub| {
                            (
                                sub.var_id,
                                sub.decompress(o, entry.dno).expect("sub decompress"),
                            )
                        })
                        .collect();
                    let refs: Vec<YssdSubBlockRaw<'_>> = raws
                        .iter()
                        .map(|(v, raw)| YssdSubBlockRaw { var_id: *v, raw })
                        .collect();
                    let body = yu_format::yssd::encode_multi_body_with(BlockCodec::Snappy, &refs);
                    // `encode_multi_body*` 返回的是**整块体**(12 字节块头 +
                    // 裸子块流),所以要走一遍容器重建再取载荷。
                    let mut w = YssdWriter::new(save.version);
                    w.set_block(entry.dno, body);
                    let rebuilt = YssdSave::parse(&w.build(), b"re").expect("re-parse");
                    let payload = &rebuilt.entry(entry.dno).expect("dno").compressed;
                    let again =
                        parse_multi_payload(payload, b"re", entry.dno).expect("re-parse multi");
                    assert_eq!(again.len(), raws.len(), "{origin} DNO{}: 子块数不符", entry.dno);
                    for (i, sub) in again.iter().enumerate() {
                        let back = sub.decompress(b"re", entry.dno).expect("sub re-decompress");
                        assert_eq!(back, raws[i].1, "{origin} DNO{} sub{i}: 往返不一致", entry.dno);
                    }
                    multi += 1;
                }
                _ => {
                    let raw = entry
                        .decompress(o)
                        .unwrap_or_else(|d| panic!("{origin} DNO{}: {d:?}", entry.dno));
                    let codec = BlockCodec::detect(entry).expect("btype 0/1 必可判定");
                    match codec {
                        BlockCodec::Snappy => snappy_blocks += 1,
                        BlockCodec::Zlib => zlib_blocks += 1,
                    }
                    // ① 用**它自己的**压缩层重压:写回路径的保真度
                    //    (官方存档 = Snappy;本引擎早先写出的是 zlib)。
                    // ② 再用 Snappy 压一遍:编码器在真实载荷上的正确性。
                    for (label, c) in [("自层", codec), ("snappy", BlockCodec::Snappy)] {
                        let re = encode_block_body_with(
                            c,
                            entry.btype,
                            entry.strict,
                            entry.var_id,
                            &raw,
                        );
                        let mut w = YssdWriter::new(save.version);
                        w.set_block(entry.dno, re);
                        let again = YssdSave::parse(&w.build(), b"re").expect("re-parse");
                        let back = again
                            .entry(entry.dno)
                            .expect("dno")
                            .decompress(b"re")
                            .unwrap_or_else(|d| {
                                panic!("{origin} DNO{} [{label}]: 重压结果解不开 {d:?}", entry.dno)
                            });
                        assert_eq!(
                            back, raw,
                            "{origin} DNO{} [{label}]: 往返不一致",
                            entry.dno
                        );
                    }
                    plain += 1;
                }
            }
        }
    }
    assert!(files > 0, "{} 下没有可解析的 .sd", dir.display());
    println!(
        "yssd-snappy: {files} 文件 / 单块 {plain}(Snappy {snappy_blocks} + zlib {zlib_blocks})         / 多块 {multi} / 嵌套 {nested} 全部往返一致"
    );
}
