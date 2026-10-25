//! 真实语料验收(任务书 B §2):官方 YSCom 编译的 ERIS 产物。
//!
//! 环境变量(缺样本 = 打印 skip 并计数,不许空跑通过):
//! - `YURIS_SAMPLE_ERIS_YSBIN` = `…/workbench/project-eris/data/ysbin`
//! - `YURIS_SAMPLE_SDK0495`   = `…/yu-ris_0495_032`(取 YSCom.ycd 与手册)
//!
//! 全部断言跑在真实字节上;钉死的期望值全部来自本次实测
//! (原始输出见 PROGRESS 成果 B2),非猜测。

use std::collections::BTreeSet;
use std::path::PathBuf;

use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::yscm::CommandTable;
use yu_format::yslb::YslLabels;
use yu_format::ystb::{self, Segments};
use yu_format::ystl::ScriptList;
use yu_format::ysvr::VarDecls;
use yu_format::ystd::YstdHeader;
use yu_format::yser::YserTable;
use yu_format::Codec;

struct Skips(u32);
impl Skips {
    fn skip(&mut self, name: &str, reason: &str) {
        // 设 YURIS_REQUIRE_SAMPLES=1 时缺样本 = 硬失败,禁止静默通过。
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {name}: {reason}");
        }
        self.0 += 1;
        println!("SKIP[count={}] {name}: {reason}", self.0);
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_dir())
}

fn corpus() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let ysbin = env_path("YURIS_SAMPLE_ERIS_YSBIN")?;
    let sdk = env_path("YURIS_SAMPLE_SDK0495")?;
    // `YURIS_SAMPLE_SDK0495` 有两种既有约定,本函数两种都接受:
    //   (a) SDK 根 `…/yu-ris_0495_032`(本文件历史的写法,`マニュアル`
    //       与 `システム` 都在它下面);
    //   (b) `…/yu-ris_0495_032/システム`(AGENTS.md §2 与
    //       `yu-vfs/tests/real_sample.rs` 的写法)。
    // 判据用目录存在性,不猜:哪种布局的 `マニュアル` 在位就用哪个根。
    let sdk_root = if sdk.join("マニュアル").is_dir() {
        sdk
    } else if sdk
        .parent()
        .map(|p| p.join("マニュアル").is_dir())
        .unwrap_or(false)
    {
        sdk.parent().expect("parent 已判存在").to_path_buf()
    } else {
        // 两种布局都不是 ⇒ 样本不可用(显式 skip,不算通过)。
        return None;
    };
    let manual = sdk_root.join("マニュアル").join("YU-RIS").join("html");
    Some((ysbin, sdk_root, manual))
}

// ---------------------------------------------------------------------------

#[test]
fn corpus_acceptance() {
    let mut skips = Skips(0);
    let Some((ysbin, _sdk, manual)) = corpus() else {
        skips.skip("corpus_acceptance", "缺少 YURIS_SAMPLE_* 环境变量");
        assert_eq!(skips.0, 1, "样本缺失必须显式 skip 并计数");
        return;
    };
    let origin = |name: &str| format!("ysbin/{name}").into_bytes();

    // ---- 七张表 + 全部脚本体 ----
    let read = |name: &str| std::fs::read(ysbin.join(name)).unwrap();
    let ysc = CommandTable::parse(&read("ysc.ybn"), &origin("ysc.ybn")).unwrap();
    let ysl = YslLabels::parse(&read("ysl.ybn"), &origin("ysl.ybn")).unwrap();
    let ysv = VarDecls::parse(&read("ysv.ybn"), &origin("ysv.ybn")).unwrap();
    let yst = YstdHeader::parse(&read("yst.ybn"), &origin("yst.ybn")).unwrap();
    let ystl = ScriptList::parse(&read("yst_list.ybn"), &origin("yst_list.ybn")).unwrap();
    let _yscf = yu_format::yscf::Yscf::parse(&read("yscfg.ybn"), &origin("yscfg.ybn")).unwrap();
    let yse = YserTable::parse(&read("yse.ybn"), &origin("yse.ybn")).unwrap();

    // ysc:119 条命令,名字唯一(0.495 语料实测)
    assert_eq!(ysc.commands.len(), 119, "ysc 命令条数");
    let ysc_names: BTreeSet<String> =
        ysc.commands.iter().map(|c| c.name.as_ref().to_string()).collect();
    assert_eq!(ysc_names.len(), 119, "命令名唯一");
    // 关键序位(与源码对拍的实证锚点)
    for (rank, name) in [(14usize, "END"), (21, "FILEACT"), (43, "GO"), (44, "GOSUB"), (96, "TEXT")] {
        assert_eq!(ysc.name_of(rank as u16), Some(name), "opcode {rank} 应为 {name}");
    }

    // ---- 三方互证 ①:ysc ⊆ YSCom.ycd 的 COM_* 名(ERROR 命令参数表)----
    let ycd = std::fs::read(
        _sdk.join("システム").join("system").join("YSCom").join("YSCom.ycd"),
    )
    .unwrap();
    let com_names = ycd_com_names(&ycd);
    assert_eq!(com_names.len(), 105, "ycd 的 COM_* 名单应有 105 条");
    let com_stripped: BTreeSet<String> = com_names
        .iter()
        .map(|n| n.trim_start_matches("COM_").to_string())
        .collect();
    // 实测差异:ycd 独有 COM_PRODUCT(引擎 ysc 表无 PRODUCT 命令;
    // 编译器语言表与引擎命令集的版本差,证据存 docs/divergences/DIV-008)
    let missing_in_ysc: Vec<_> = com_stripped.difference(&ysc_names).collect();
    assert_eq!(missing_in_ysc, vec!["PRODUCT"], "ycd 独有命令集(钉死)");
    // ysc − COM_* = 编译器指令/设置项/无错误级的 0.495 命令(数量与成员钉死;
    // 104 = 105 − PRODUCT,15 = 下列集合)
    let ysc_extra: BTreeSet<String> = ysc_names
        .difference(&com_stripped)
        .map(|s| s.to_string())
        .collect();
    println!("ysc − COM_* ({} 条):{:?}", ysc_extra.len(), ysc_extra);
    let expected_extra: BTreeSet<String> = [
        ");", "CDDA", "COMPILEMODE", "EMOTE", "EMOTEINFO", "F_FLT", "F_INT", "F_STR",
        "F_VAR", "MATH3D", "MIDIEND", "MIDIINFO", "PROJECTFOLDER", "RELEASEMODE",
        "SYSTEMMODE",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(ysc_extra, expected_extra, "非 COM_* 条目集合(钉死)");
    assert_eq!(ysc_names.len(), 104 + 15, "119 = 104 个 COM + 15 个非 COM");

    // ---- 三方互证 ②:ysc ⊇ 手册 menu.html 文档化命令集 ----
    let manual_cmds = manual_commands(&manual);
    println!("手册文档化命令:{}", manual_cmds.len());
    let manual_missing: Vec<_> = manual_cmds.difference(&ysc_names).collect();
    assert!(
        manual_missing.is_empty(),
        "手册命令不在 ysc 中:{manual_missing:?}"
    );

    // ---- 标签闭环:全部 yst 解码 + GO/GOSUB 引用对 ysl ----
    // 标签按文本比较:ysl 名(原始 CP932 字节)先解码
    let ysl_names: BTreeSet<String> = ysl
        .entries
        .iter()
        .map(|e| Codec::Cp932.decode(&e.name, b"ysl").unwrap())
        .collect();
    let mut files = 0u32;
    let mut total_insts = 0u64;
    let mut total_windows = 0u64;
    let mut go_gosub_refs = 0u64;
    let mut resolved = 0u64;
    let mut dynamic = 0u64;
    let mut unresolved: Vec<(u32, String)> = Vec::new();
    let mut text_samples: Vec<Vec<u8>> = Vec::new();
    for entry in 0..=140u32 {
        let name = format!("yst{entry:05}.ybn");
        let path = ysbin.join(&name);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        files += 1;
        let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, &origin(&name)).unwrap();
        // 异或对合:逐文件回封 = 原文件
        assert_eq!(segs.encrypt(495, &DEFAULT_KEY_BYTES), bytes, "{name} 往返");
        let script =
            ystb::decode(&segs, entry, &ysc, Codec::Cp932, &origin(&name)).unwrap();
        total_insts += script.groups.len() as u64;
        for (gi, group) in script.groups.iter().enumerate() {
            total_windows += group.windows.len() as u64;
            if group.command != "GO" && group.command != "GOSUB" {
                continue;
            }
            go_gosub_refs += 1;
            let refs: Vec<_> = script
                .labels
                .entries
                .iter()
                .filter(|r| r.group as usize == gi)
                .collect();
            if refs.is_empty() {
                dynamic += 1; // 非引号串载荷(H/V/数值)= 运行期动态目标
            }
            for r in &refs {
                if ysl_names.contains(&r.name) {
                    resolved += 1;
                } else {
                    unresolved.push((entry, r.name.clone()));
                }
            }
            // 采样 M 形载荷文本供 auto 评分(脱敏:只取长度,样本即脚本常量)
            if text_samples.len() < 40 {
                let w = &group.windows[0];
                let (size, off) = (
                    u32::from_le_bytes(w.bytes[4..8].try_into().unwrap()) as usize,
                    u32::from_le_bytes(w.bytes[8..12].try_into().unwrap()) as usize,
                );
                if size >= 5 && off + size <= segs.args_data.len() && segs.args_data[off] == b'M' {
                    let ln = u16::from_le_bytes(
                        segs.args_data[off + 1..off + 3].try_into().unwrap(),
                    ) as usize;
                    if ln >= 6 && off + 3 + ln <= segs.args_data.len() {
                        text_samples.push(segs.args_data[off + 4..off + 2 + ln].to_vec());
                    }
                }
            }
        }
    }
    println!(
        "YSTB: files={files} insts={total_insts} windows={total_windows} \
         GO/GOSUB={go_gosub_refs} resolved={resolved} dynamic={dynamic} unresolved={} (unique {})",
        unresolved.len(),
        unresolved.iter().collect::<BTreeSet<_>>().len()
    );
    assert_eq!(files, 136, "yst 文件数(0..140 中缺 5 个无产物脚本)");
    // 标签闭环:未解析引用必须与钉死清单完全一致(ERIS 样板的可选扩展点)
    let uniq: BTreeSet<(u32, String)> = unresolved.into_iter().collect();
    let expected: BTreeSet<(u32, String)> = PINNED_UNRESOLVED
        .iter()
        .map(|&(s, l)| (s, l.to_string()))
        .collect();
    let diff: Vec<_> = uniq.symmetric_difference(&expected).take(6).collect();
    assert!(diff.is_empty(), "未解析标签与钉死清单不符:{diff:?}");
    println!(
        "未解析标签 {} 组全部为 ERIS 样板的音声/更新钩子(根因见 docs/divergences/DIV-006)",
        uniq.len()
    );

    // ---- F1 钉死:yst00008 第 232 条 TASK 的 slot 34 `#` 标签 ----
    // 证据:ysl 记录 `es.SS.FADE.TASK` = (script_no=8, field_b=232)、
    // `es.SS.FADE.TASK.MAIN` = (8, 234);ysc TASK 参数表第 34 项为 `#`。
    {
        let bytes = std::fs::read(ysbin.join("yst00008.ybn")).unwrap();
        let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, b"yst00008").unwrap();
        let script = ystb::decode(&segs, 8, &ysc, Codec::Cp932, b"yst00008").unwrap();
        // 源码第 524 行:`TASK[ID=es.SS.FADE.TASK #=es.SS.FADE.TASK.MAIN ...]`
        // ID(slot 0)是任务 ID 字符串,`#`(slot 34)才是标签引用。
        let task_labels: Vec<(&str, u32)> = script
            .labels
            .entries
            .iter()
            .filter(|r| r.group == 232)
            .map(|r| (r.name.as_str(), r.group))
            .collect();
        assert_eq!(
            task_labels,
            vec![("es.SS.FADE.TASK.MAIN", 232u32)],
            "TASK 的 slot 34 `#` 必须按窗口 slot 索引收进 DecodedScript.labels"
        );
    }

    // ---- auto 评分:真实文本池 → CP932(ERIS 为日文原生形态)----
    let refs: Vec<&[u8]> = text_samples.iter().map(|v| v.as_slice()).collect();
    let det = yu_format::codec::auto::detect(&refs);
    println!(
        "auto: cp932={:?} gbk={:?}",
        det.cp932.points, det.gbk.points
    );
    assert_eq!(det.codec, Codec::Cp932, "ERIS 文本池应判为 CP932:{det:?}");

    // ---- yst_list ↔ yst 文件 ↔ ysl 一致性 ----
    assert_eq!(ystl.entries.len(), 141, "清单条数");
    for (i, entry) in ystl.entries.iter().enumerate() {
        assert_eq!(entry.index as usize, i, "index 恰为下标序");
    }
    let no_script: Vec<u32> = ystl
        .entries
        .iter()
        .filter(|e| !e.has_script())
        .map(|e| e.index)
        .collect();
    assert_eq!(no_script, vec![26, 27, 69, 74, 108], "无产物条目 = 磁盘缺号");
    for entry in &ystl.entries {
        if !entry.has_script() {
            continue;
        }
        let file = format!("yst{:05}.ybn", entry.index);
        assert!(ysbin.join(&file).is_file(), "{file} 应存在");
        // labelCount 与 ysl 逐条互证
        let in_ysl = ysl
            .entries
            .iter()
            .filter(|l| l.script_no == entry.index as u16)
            .count();
        assert_eq!(
            entry.label_count as usize, in_ysl,
            "脚本 {} 的标签数:清单 {} vs ysl {}",
            entry.index, entry.label_count, in_ysl
        );
    }
    // ysl:全部标签的脚本号都指向存在的文件
    for label in &ysl.entries {
        let file = format!("yst{:05}.ybn", label.script_no);
        assert!(ysbin.join(&file).is_file(), "ysl 标签指向不存在的 {file}");
    }
    // ysl 的 field_b(疑似指令位置)全部落在对应脚本的指令数内 —— 假设佐证
    let mut pc_checked = 0usize;
    for label in &ysl.entries {
        let bytes = std::fs::read(ysbin.join(format!("yst{:05}.ybn", label.script_no))).unwrap();
        let inst_count = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        assert!(
            (label.field_b as usize) < inst_count,
            "标签 {:?} 的 field_b={} 超出脚本 {} 指令数 {inst_count}",
            String::from_utf8_lossy(&label.name),
            label.field_b,
            label.script_no
        );
        pc_checked += 1;
    }
    println!("ysl field_b < 指令数: {pc_checked}/{pc_checked}(field_b = 脚本内位置,假设成立)");

    // ---- ysv 语义抽样 ----
    assert_eq!(ysv.decls.len(), 2568, "ysv 记录数");
    let kind: std::collections::BTreeMap<u8, usize> =
        ysv.decls.iter().fold(Default::default(), |mut m, d| {
            *m.entry(d.kind).or_default() += 1;
            m
        });
    assert_eq!(kind.get(&1), Some(&1167), "全局 G_*");
    assert_eq!(kind.get(&2), Some(&928), "局部");
    assert_eq!(kind.get(&3), Some(&473), "静态 S_*");
    assert_eq!(
        ysv.decls.iter().filter(|d| d.ty == 0).count(),
        452,
        "仅声明(ty=0,无初值段)"
    );
    for d in &ysv.decls {
        assert!(
            u32::from(d.slot) < yst.slot_count(),
            "槽位 {} 超出 yst 槽位数 {}",
            d.slot,
            yst.slot_count()
        );
    }
    // 初值抽样:G_INT[@es.SMV.SX=400] / FPS60 相关(实测非零初值集合的成员)
    let int_inits: BTreeSet<i64> = ysv
        .decls
        .iter()
        .filter_map(|d| match d.init {
            yu_format::Init::Int(v) => Some(v),
            _ => None,
        })
        .collect();
    for v in [400, 720, 128] {
        assert!(int_inits.contains(&v), "非零整数初值应含 {v}: {int_inits:?}");
    }
    // 恢复组号分布(实测:G_* 为 0..=3,未见 4)
    let flags: BTreeSet<u8> = ysv
        .decls
        .iter()
        .filter(|d| d.kind == 1)
        .map(|d| d.flag)
        .collect();
    assert_eq!(flags, BTreeSet::from([0, 1, 2, 3]), "全局组号分布");

    // ---- yse ----
    assert_eq!(yse.entries.len(), 121, "yse 记录数");
    assert_eq!(yse.entries[0].id, 100010);
    let msg = Codec::Cp932.decode(&yse.entries[0].name, b"yse[0]").unwrap();
    assert_eq!(msg, "メモリ不足です。");

    println!(
        "corpus_acceptance: 全部通过(ysc=119, ysl={}, ysv={}, yst_list=141, yst 文件={files})",
        ysl.entries.len(),
        ysv.decls.len()
    );
}

// ---------------------------------------------------------------------------
// 钉死清单:GO/GOSUB 静态引用中不在 ysl 的全部 (脚本号, 标签) 组
// (ERIS 样板的音声再生钩子 ES.*.VoicePlay.N 与更新钩子;官方样板未定义,
//  YSCom 不校验目标,引擎以 ERROR[] 错误等级容忍 —— 见 DIV-006)
// 钉死清单由 include 引入:tests/fixtures/pinned_unresolved.rs
include!("fixtures/pinned_unresolved.rs");

// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 测试侧解析器:YSCom.ycd(4 字节参数元数据)与手册 menu.html
// ---------------------------------------------------------------------------

/// 从 ycd 的 119 条命令记录中取 ERROR 命令的 `COM_*` 参数名。
fn ycd_com_names(ycd: &[u8]) -> Vec<String> {
    assert_eq!(&ycd[0..4], b"YSCD");
    assert_eq!(u32::from_le_bytes(ycd[4..8].try_into().unwrap()), 495);
    let count = u32::from_le_bytes(ycd[8..12].try_into().unwrap()) as usize;
    let mut at = 0x10usize;
    let mut com = Vec::new();
    for _ in 0..count {
        let end = ycd[at..].iter().position(|&b| b == 0).unwrap() + at;
        let name = String::from_utf8_lossy(&ycd[at..end]).into_owned();
        let argc = ycd[end + 1];
        let mut p = end + 2;
        for _ in 0..argc {
            let e2 = ycd[p..].iter().position(|&b| b == 0).unwrap() + p;
            if name == "ERROR" {
                let arg = String::from_utf8_lossy(&ycd[p..e2]).into_owned();
                if arg.starts_with("COM_") {
                    com.push(arg);
                }
            }
            p = e2 + 5; // 参数名 NUL + 2×u16 元数据
        }
        at = p;
    }
    com
}

/// 从手册 menu.html 的分类页链接提取文档化命令名。
fn manual_commands(manual: &std::path::Path) -> BTreeSet<String> {
    let text = std::fs::read(manual.join("menu.html")).unwrap();
    let text = String::from_utf8_lossy(&text);
    let mut cmds = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while let Some(pos) = text[i..].find("HREF=\"./").map(|p| p + i) {
        let rest = &text[pos + 8..];
        let Some(slash) = rest.find('/') else { break };
        let Some(end_q) = rest[slash..].find('"') else { break };
        let page = &rest[slash + 1..slash + end_q];
        // 链接文本在 > 之后
        let after = &rest[end_q..];
        let Some(gt) = after.find('>') else { break };
        let tail = &after[gt + 1..];
        let Some(lt) = tail.find('<') else { break };
        let label = tail[..lt].trim();
        // 链接文本形如 `GO［］`(CP932 全角括);命令名 = 前缀 ASCII 段
        let cat = &rest[..slash];
        let name: String = label
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .map(|b| b as char)
            .collect();
        if !name.is_empty()
            && page.ends_with(".html")
            && page != "top.html"
            && cat != "basic"
        {
            cmds.insert(name);
        }
        i = pos + 8;
    }
    let _ = bytes;
    cmds
}
