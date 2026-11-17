//! 低版本(2xx)官方语料验收:0.292 / 0.255 的 YSCom 产物。
//!
//! 环境变量(缺样本 = 打印 skip 并计数,不许空跑通过):
//! - `YURIS_SAMPLE_V2_292` = 0.292 编译产物目录(119 文件 / 112 YSTB)
//! - `YURIS_SAMPLE_V2_255` = 0.255 样本解包目录(103 文件 / 96 YSTB)
//!
//! 期望值全部来自本轮实测(原始输出见 `PROGRESS.md` 成果 L-1):
//! 指令总数、控制流目标、标签、YSCF 布局与官方 0.292 引擎
//! (`yu-ris.exe` sha256 `72628dfa…`)装载器 `FUN_00437da9` 的走查规则一致。

use std::collections::BTreeSet;
use std::path::PathBuf;

use yu_format::key::{crc32, DEFAULT_KEY_BYTES};
use yu_format::yscf::{Yscf, YscfLayout};
use yu_format::yscm::CommandTable;
use yu_format::{yslb, yser, ystb, ystl, ysvr};

/// 0.255 的官方旧键(`CRC32` 之外的固定字节序列;样本实证)。
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

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_dir())
}

struct Corpus {
    dir: PathBuf,
    version: u32,
    key: [u8; 4],
    commands: usize,
    labels: usize,
    scripts_in_list: usize,
    vars: usize,
    ystb_files: usize,
    instructions: usize,
    control_targets: usize,
    layout: YscfLayout,
    title: &'static [u8],
}

fn check_corpus(spec: Corpus) {
    let origin = |name: &str| format!("ysbin/{name}").into_bytes();
    let read = |name: &str| std::fs::read(spec.dir.join(name)).unwrap();

    let ysc = CommandTable::parse(&read("ysc.ybn"), &origin("ysc.ybn")).unwrap();
    assert_eq!(ysc.commands.len(), spec.commands, "ysc 命令条数");
    assert!(
        ysc.index_of("RETURNCODE").is_some(),
        "V2 走查必须按名找到 RETURNCODE"
    );

    let ysl = yslb::YslLabels::parse(&read("ysl.ybn"), &origin("ysl.ybn")).unwrap();
    assert_eq!(ysl.entries.len(), spec.labels, "ysl 标签数");
    assert_eq!(ysl.version, spec.version);

    let ysv = ysvr::VarDecls::parse(&read("ysv.ybn"), &origin("ysv.ybn")).unwrap();
    assert_eq!(ysv.decls.len(), spec.vars, "ysv 变量数");
    assert_eq!(ysv.version, spec.version);

    let ystl = ystl::ScriptList::parse(&read("yst_list.ybn"), &origin("yst_list.ybn")).unwrap();
    assert_eq!(ystl.entries.len(), spec.scripts_in_list, "yst_list 条数");

    let _yst = yu_format::ystd::YstdHeader::parse(&read("yst.ybn"), &origin("yst.ybn")).unwrap();
    let _yse = yser::YserTable::parse(&read("yse.ybn"), &origin("yse.ybn")).unwrap();
    let cfg = Yscf::parse(&read("yscfg.ybn"), &origin("yscfg.ybn")).unwrap();
    assert_eq!(cfg.layout, spec.layout, "YSCF 布局");
    assert_eq!((cfg.width, cfg.height), (800, 600), "画面尺寸");
    assert_eq!(cfg.title, spec.title, "标题");

    // 逐个脚本体:V2 解码闭合、控制目标换算、原字节回写。
    let mut ystb_count = 0usize;
    let mut instructions = 0usize;
    let mut control_targets = 0usize;
    let mut offsets_by_script: std::collections::BTreeMap<u16, Vec<u32>> =
        std::collections::BTreeMap::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&spec.dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("yst0") && n.ends_with(".ybn"))
        })
        .collect();
    entries.sort();
    for path in entries {
        let bytes = std::fs::read(&path).unwrap();
        if bytes.get(0..4) != Some(b"YSTB") {
            continue;
        }
        ystb_count += 1;
        let name = path.file_name().unwrap().to_str().unwrap();
        let script_id: u16 = name[3..8].parse().unwrap();
        let segs = ystb::Segments::decrypt_with_table(&bytes, &spec.key, &ysc, name.as_bytes())
            .unwrap_or_else(|d| panic!("{name}: {d}"));
        assert_eq!(segs.version, spec.version, "{name} 版本");
        assert!(segs.layout.is_v2(), "{name} 布局");
        assert_eq!(
            segs.encrypt(spec.version, &spec.key),
            bytes,
            "{name} 逐字节回写"
        );
        instructions += segs.count as usize;
        let offsets = ystb::v2_code_offsets(&bytes, &spec.key, &ysc, name.as_bytes()).unwrap();
        assert_eq!(offsets, segs.code_offsets, "{name} 偏移表一致");
        assert!(
            offsets.windows(2).all(|w| w[0] < w[1]),
            "{name} 偏移表必须严格升序"
        );
        offsets_by_script.insert(script_id, offsets);

        // 控制流目标在合成视图里已经是组下标;计数并做边界检查。
        let if_op = ysc.index_of("IF").unwrap() as u8;
        let else_op = ysc.index_of("ELSE").unwrap() as u8;
        let loop_op = ysc.index_of("LOOP").unwrap() as u8;
        let mut window_at = 0usize;
        for pc in 0..segs.count as usize {
            let op = segs.inst[pc * 4];
            let argc = segs.inst[pc * 4 + 1] as usize;
            if [if_op, else_op, loop_op].contains(&op) {
                for slot in 1..argc {
                    let at = window_at + slot * 12;
                    let len = u32::from_le_bytes([
                        segs.args_index[at + 4],
                        segs.args_index[at + 5],
                        segs.args_index[at + 6],
                        segs.args_index[at + 7],
                    ]);
                    if len != 0 {
                        control_targets += 1;
                        assert!(
                            (len as usize) < segs.count as usize,
                            "{name} pc={pc} 槽 {slot} 目标 {len} 越界"
                        );
                    }
                }
            }
            window_at += argc * 12;
        }
    }
    assert_eq!(ystb_count, spec.ystb_files, "YSTB 文件数");
    assert_eq!(instructions, spec.instructions, "指令总数");
    assert_eq!(control_targets, spec.control_targets, "控制流目标数");

    // 标签:V2 的 field_b 是 code 偏移 → 必须落在指令起点并换成组下标。
    let mut converted = 0usize;
    for label in &ysl.entries {
        let Some(offsets) = offsets_by_script.get(&label.script_no) else {
            // 标签指向未随样本发布的脚本(样本缺文件,不是解码失败)。
            continue;
        };
        let Ok(pc) = offsets.binary_search(&label.field_b) else {
            panic!(
                "标签 {:?} 的偏移 {:#x} 不是脚本 {} 的指令起点",
                String::from_utf8_lossy(&label.name),
                label.field_b,
                label.script_no
            );
        };
        assert!(pc < offsets.len());
        converted += 1;
    }
    assert_eq!(converted, spec.labels, "标签必须全部可换算");

    // 交叉检查:0.292 用默认键;0.255 的旧键不是 CRC32("Yu-ris")。
    if spec.version < 290 {
        assert_ne!(spec.key, DEFAULT_KEY_BYTES);
        assert_ne!(crc32(b"Yu-ris").to_be_bytes(), spec.key);
    } else {
        assert_eq!(spec.key, DEFAULT_KEY_BYTES);
    }

    // 名称唯一性(标签/命令)顺手校验,防表解析半闭合。
    let labels: BTreeSet<&[u8]> = ysl.entries.iter().map(|e| e.name.as_slice()).collect();
    assert_eq!(labels.len(), ysl.entries.len(), "ysl 名字唯一");
}

#[test]
fn corpus_v2_292() {
    let mut skips = Skips(0);
    let Some(dir) = env_dir("YURIS_SAMPLE_V2_292") else {
        skips.skip("corpus_v2_292", "缺少 YURIS_SAMPLE_V2_292");
        assert_eq!(skips.0, 1, "样本缺失必须显式 skip 并计数");
        return;
    };
    eprintln!("REAL SAMPLE 0.292: {}", dir.display());
    check_corpus(Corpus {
        dir,
        version: 292,
        key: DEFAULT_KEY_BYTES,
        commands: 91,
        labels: 1189,
        scripts_in_list: 116,
        vars: 2393,
        ystb_files: 112,
        instructions: 20_117,
        control_targets: 2590,
        layout: YscfLayout::V292,
        title: b"ERIS Ver0.92",
    });
}

#[test]
fn corpus_v2_255() {
    let mut skips = Skips(0);
    let Some(dir) = env_dir("YURIS_SAMPLE_V2_255") else {
        skips.skip("corpus_v2_255", "缺少 YURIS_SAMPLE_V2_255");
        assert_eq!(skips.0, 1, "样本缺失必须显式 skip 并计数");
        return;
    };
    eprintln!("REAL SAMPLE 0.255: {}", dir.display());
    check_corpus(Corpus {
        dir,
        version: 255,
        key: KEY_255,
        commands: 88,
        labels: 1026,
        scripts_in_list: 100,
        vars: 2101,
        ystb_files: 96,
        instructions: 16_351,
        control_targets: 2279,
        layout: YscfLayout::V255,
        title: b"ERIS Ver0.55",
    });
}
