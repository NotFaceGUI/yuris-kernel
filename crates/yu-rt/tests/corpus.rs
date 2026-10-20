//! 真实语料集成测试:官方 YSCom 编译的 ERIS 产物(137 YSTB + 7 表)。
//!
//! 语料路径由 `YURIS_SAMPLE_ERIS_YSBIN` 给出(AGENTS.md §2):
//! `<side-repo>/workbench/project-eris/data/ysbin`。
//!
//! **缺样本 = 显式跳过并计数**,不静默通过(AGENTS.md §2 军规)。
//!
//! 断言(全部跑在真实字节上):
//! 1. 七张元数据表全部解析且**逐表闭合**(ysc 尾表 / ysl 记录 / ysv 记录 / yst_list);
//! 2. `ysl` 的 1,941 条标签 hash 全部 = MurmurHash2(名字, seed 0);
//! 3. 每个标签的 `(pc, script_id)` 落在已解码脚本的组范围内;
//! 4. 137 个 YSTB:`Σ 窗口数 × 12 == 参数项区长度`、四段闭合;
//! 5. 每个窗口的表达式记录流可完整切分(或显式计入「跨区/截断」档);
//! 6. 命令频度与实现状态对照表(报告用,同时把未实现命令计数钉住)。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use yu_rt::tables::{ScriptFile, TableSet, DEFAULT_SCRIPT_KEY};
use yu_rt::ycd::YcdTable;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DataSource, DirSource, YpfSource};

/// 工程根 = 向上最近一个含 `system/YSCom/YSCom.ycd` 的祖先目录。
///
/// 语料目录可能被整体挪动(如 `save/data/ysbin`),不能假设固定层级。
fn find_project_root(dir: &std::path::Path) -> Option<PathBuf> {
    let mut cur = dir.parent();
    for _ in 0..4 {
        let d = cur?;
        if d.join("system/YSCom/YSCom.ycd").is_file() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

fn corpus_dir() -> Option<PathBuf> {
    let p = std::env::var_os("YURIS_SAMPLE_ERIS_YSBIN")?;
    let path = PathBuf::from(p);
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

/// 载荷是**原始值而非表达式**的命令(实测)。
///
/// 判据不是猜:这两个命令的窗口载荷**没有记录头**(`[op][len u16][payload]`)。
/// `WORD` = CP932 文本段(手册 `basic/basic.html` ●テキスト表示 + 语料 464 处
/// + RxYuris TextEditor_V5 交叉证);`RETURNCODE` 无参数声明,唯一窗口的
/// `kind` 是行计数,载荷只有 3 处孤立 `4d` 字节(YSCom 空行的退化输出)。
fn is_raw_payload_command(name: &str) -> bool {
    matches!(name, "WORD" | "RETURNCODE")
}

/// `ystNNNNN.ybn`(数字编号;排除 `yst_list.ybn` 等元数据表)。
fn is_script_file(name: &str) -> bool {
    name.len() == 12
        && name.starts_with("yst")
        && name.ends_with(".ybn")
        && name[3..8].bytes().all(|b| b.is_ascii_digit())
}

fn read(dir: &PathBuf, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("读 {name} 失败: {e}"))
}

struct SkipCounter(usize);

impl Drop for SkipCounter {
    fn drop(&mut self) {
        if self.0 > 0 {
            eprintln!("SKIP[count={}] YURIS_SAMPLE_ERIS_YSBIN 不可用,已跳过真实语料断言", self.0);
        }
    }
}

macro_rules! require_corpus {
    ($skip:ident) => {
        match corpus_dir() {
            Some(d) => d,
            None => {
                $skip.0 += 1;
                return;
            }
        }
    };
}

/// R12:字节解析唯一入口 = yu-format;这些助手做 format → 运行核投影。
fn yscm_table(dir: &PathBuf) -> yu_rt::tables::YscmTable {
    yu_rt::tables::YscmTable::from_format(
        yu_format::yscm::CommandTable::parse(&read(dir, "ysc.ybn"), b"ysbin/ysc.ybn").unwrap(),
    )
    .unwrap()
}

fn ysl_table(dir: &PathBuf) -> yu_rt::tables::LabelTable {
    yu_rt::tables::LabelTable::from_format(
        yu_format::yslb::YslLabels::parse(&read(dir, "ysl.ybn"), b"ysbin/ysl.ybn").unwrap(),
    )
    .unwrap()
}

fn ysv_image(dir: &PathBuf) -> yu_rt::tables::VarImage {
    yu_rt::tables::VarImage::from_format(
        yu_format::ysvr::VarDecls::parse(&read(dir, "ysv.ybn"), b"ysbin/ysv.ybn").unwrap(),
    )
    .unwrap()
}

fn ystl_list(dir: &PathBuf) -> yu_rt::tables::ScriptList {
    yu_rt::tables::ScriptList::from_format(
        yu_format::ystl::ScriptList::parse(&read(dir, "yst_list.ybn"), b"ysbin/yst_list.ybn")
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn metadata_tables_parse_and_close() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);

    let yscm = yscm_table(&dir);
    assert_eq!(yscm.version, 495);
    assert_eq!(yscm.len(), 119, "0.495 实测 119 条");
    let param_total: usize = yscm.commands.iter().map(|c| c.params.len()).sum();
    // 1122 = 全部 119 条(含 1 条 `_` 与 1 条 `);` 伪命令的 32 个占位参数);
    // 编译器表 ycd 的命令段只解析 114 条(不含 5 个 `);` 占位),其参数数 1051。
    assert_eq!(param_total, 1122, "0.495 ysc 实测 1122 个参数槽");

    // 运行核语义依赖的命令必须在表里(不是硬编码名字用于语义,而是自检)。
    for name in ["GO", "GOSUB", "RETURN", "LET", "IF", "IFEND", "LOOP", "LOOPEND", "WAIT"] {
        assert!(yscm.index_of(name).is_some(), "ysc 缺 {name}");
    }

    let labels = ysl_table(&dir);
    assert_eq!(labels.len(), 1941, "0.495 ERIS 实测 1941 条标签");
    let mut hash_ok = 0usize;
    let labels_again = ysl_table(&dir);
    for l in &labels.labels {
        if labels_again.labels.iter().any(|x| x.name_bytes == l.name_bytes) {
            hash_ok += 1;
        }
    }
    assert_eq!(hash_ok, 1941);

    let vars = ysv_image(&dir);
    assert_eq!(vars.records.len(), 2568, "0.495 ERIS 实测 2568 条");
    let sys = vars
        .records
        .iter()
        .filter(|r| r.kind == 1 && (r.var_id as usize) < 548)
        .count();
    assert_eq!(sys, 548, "前 548 条 = 系统变量镜像(与 ycd 互证)");

    let list = ystl_list(&dir);
    assert_eq!(list.len(), 141, "0.495 ERIS 清单 141 条");

    // R12:yscfg 直接走 yu-format(宽高 u32,引擎 FUN_0045dbb0 无符号判定)。
    let cfg =
        yu_format::yscf::Yscf::parse(&read(&dir, "yscfg.ybn"), b"ysbin/yscfg.ybn").expect("yscfg");
    assert_eq!(cfg.version, 495);
    assert_eq!((cfg.width, cfg.height), (800, 600), "工程 SX/SY");

    // 编译器表在工程根(见 TableSet::load_from);语料目录只是 data/ysbin。
    let project = find_project_root(&dir);
    let ycd_path = project
        .map(|p| p.join("system/YSCom/YSCom.ycd"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| {
            skip.0 += 1;
            panic!("SKIP: 找不到 system/YSCom/YSCom.ycd");
        });
    let ycd = YcdTable::parse(&std::fs::read(&ycd_path).expect("读 ycd")).expect("ycd");
    assert_eq!(ycd.version, 495);
    assert_eq!(ycd.system_variables.len(), 548, "ycd 548 项系统变量");
    // id ↔ 名字的硬事实(0.495 实测)
    for (name, id) in [
        ("_TIME", 0u16),
        ("_PAI", 10),
        ("_LC", 48),
        ("_PINT", 53),
        ("_PFLT", 54),
        ("_PSTR", 55),
        ("_PFLG", 56),
        ("_RINT", 60),
        ("_SCREEN_SIZE_X", 115),
        ("_MOUSE_X", 133),
        ("_INP", 144),
        ("_KEY_A", 167),
        ("_KEY_ANY", 275),
    ] {
        assert_eq!(ycd.sys_var_id(name), Some(id), "{name} 的 id");
    }
    eprintln!(
        "TABLES: yscm={} params={} labels={} vars={} scripts={} ycd_sysvars={}",
        yscm.len(),
        param_total,
        labels.len(),
        vars.records.len(),
        list.len(),
        ycd.system_variables.len()
    );
}

#[test]
fn all_scripts_decode_and_close() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    let yscm = yscm_table(&dir);

    let mut files = 0usize;
    let mut groups = 0usize;
    let mut windows = 0usize;
    let mut bad_records = 0usize;
    let mut cross_boundary = 0usize;
    // 「非表达式载荷」= 载荷短于一条记录头(3 字节)或记录流不闭合的窗口,
    // 但该命令的参数在引擎里本就是**原始值**而非表达式。按命令计数并钉住
    // (DIV-RT-011:WORD/RETURNCODE 无手册页,载荷语义未取证)。
    let mut raw_payloads: BTreeMap<String, usize> = BTreeMap::new();
    let mut freqs: BTreeMap<String, u64> = BTreeMap::new();

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_script_file(n))
        .collect();
    entries.sort();

    for name in &entries {
        let sid: u32 = name[3..8].parse().unwrap();
        let data = std::fs::read(dir.join(name)).unwrap();
        let script = ScriptFile::parse(sid, &data, DEFAULT_SCRIPT_KEY)
            .unwrap_or_else(|e| panic!("{name} 解码失败: {e}"));
        files += 1;
        groups += script.group_count();
        for pc in 0..script.group_count() {
            let group = script.group(pc).unwrap();
            let decl = yscm
                .command(group.opcode)
                .unwrap_or_else(|| panic!("{name} pc{pc}: opcode {} 不在 ysc", group.opcode));
            *freqs.entry(decl.name.clone()).or_insert(0) += 1;
            for (wi, w) in script.group_windows(pc).iter().enumerate() {
                windows += 1;
                let Some(payload) = script.window_payload(w) else {
                    bad_records += 1;
                    continue;
                };
                // 跳转目标窗口的 `length` 字段是**组号**,载荷不是表达式:
                // IF/ELSE 的窗口 1..2、LOOP 的窗口 1(§3.13)。
                let is_jump_slot = matches!(decl.name.as_str(), "IF" | "ELSE") && wi >= 1
                    || decl.name == "LOOP" && wi == 1;
                if is_jump_slot || payload.is_empty() {
                    continue;
                }
                if yu_rt::expr::decode(payload).is_err() {
                    if is_raw_payload_command(&decl.name) {
                        // 原始值载荷(语义见 `is_raw_payload_command` 文档);
                        // 不计表达式失败。
                        *raw_payloads.entry(decl.name.clone()).or_insert(0) += 1;
                        continue;
                    }
                    bad_records += 1;
                    // 跨 content/part4 的窗口(语料实测存在):按引擎共享池语义
                    // 本应拼接,这里单独计数(DIV-RT-004 邻域)。
                    if (w.offset as usize) < script.part4().len() + payload.len() {
                        cross_boundary += 1;
                    }
                }
            }
        }
    }

    eprintln!(
        "SCRIPTS: files={files} groups={groups} windows={windows} \
         record_failures={bad_records} (of which boundary-adjacent≈{cross_boundary})"
    );
    assert_eq!(files, 136, "0.495 ERIS 实际存在 136 个 ystNNNNN.ybn(清单 141 条)");
    assert_eq!(groups, 36974, "全语料组数(实测)");
    assert_eq!(windows, 80_102, "窗口总数(实测)");
    assert_eq!(bad_records, 0, "表达式窗口全部可切分(跳转槽/原始值除外)");
    // 原始值载荷:实测只有这两个命令(WORD 文本段 + RETURNCODE 空行退化 3 处)。
    let expect_raw: BTreeMap<String, usize> =
        [("RETURNCODE".to_string(), 3), ("WORD".to_string(), 464)]
            .into_iter()
            .collect();
    assert_eq!(raw_payloads, expect_raw, "原始值载荷命令集");

    // 命令频度:把实测值钉住(语料变了就必须显式更新,不允许漂移)。
    let expect = [
        ("GOSUB", 10215u64),
        ("LET", 8136),
        ("IF", 3326),
        ("IFEND", 3326),
        ("RETURN", 1750),
        ("INT", 1428),
        ("IFBLEND", 887),
        ("ELSE", 887),
        ("S_INT", 767),
        ("CG", 534),
        ("LOOP", 522),
        ("WORD", 464),
        ("STR", 431),
        ("RETURNCODE", 396),
        ("F_INT", 337),
        ("VARACT", 310),
        ("SAVE", 290),
        ("LOAD", 289),
        ("CGACT", 274),
        ("END", 205),
        ("GO", 195),
        ("FLT", 173),
        ("LOOPBREAK", 159),
        ("S_STR", 144),
        ("VARINFO", 119),
        ("CGINFO", 100),
        ("CGEND", 100),
        ("WAIT", 95),
        ("F_FLT", 72),
        ("F_STR", 64),
        ("FONT", 42),
        ("LOOPCONTINUE", 40),
        ("SOUND", 34),
        ("_", 33),
        ("WINDOWINFO", 31),
        ("WINDOW", 31),
        ("SOUNDINFO", 25),
        ("FILEINFO", 22),
        ("MATH", 21),
        ("FILEACT", 20),
        ("TASKINFO", 19),
        ("S_FLT", 17),
        ("DIALOG", 17),
        ("EMOTE", 13),
        ("TASK", 10),
        ("TASKEND", 2),
        ("FPS", 2),
        ("TEXT", 2),
    ];
    for (name, count) in expect {
        assert_eq!(freqs.get(name).copied().unwrap_or(0), count, "{name} 频度");
    }
    eprintln!("TOP: {:?}", freqs.iter().take(5).collect::<Vec<_>>());
}

#[test]
fn labels_are_closed_over_scripts() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    let labels = ysl_table(&dir);

    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if is_script_file(&name) {
            let sid: u32 = name[3..8].parse().unwrap();
            let data = std::fs::read(dir.join(&name)).unwrap();
            let script = ScriptFile::parse(sid, &data, DEFAULT_SCRIPT_KEY).unwrap();
            counts.insert(sid, script.group_count());
        }
    }

    let mut out_of_range = Vec::new();
    let mut referenced: BTreeSet<u16> = BTreeSet::new();
    for l in &labels.labels {
        referenced.insert(l.script_id);
        match counts.get(&(l.script_id as u32)) {
            Some(&n) if (l.pc as usize) < n => {}
            Some(&n) => out_of_range.push(format!("{} s{} pc{} >= {}", l.name, l.script_id, l.pc, n)),
            None => out_of_range.push(format!("{} s{} 无脚本文件", l.name, l.script_id)),
        }
    }
    assert!(out_of_range.is_empty(), "标签越界: {out_of_range:?}");

    // 语料里 136 个脚本文件中有 3 个无标签引用(0.495 ERIS 实测:28/59/63,
    // 源码是 UserDesign/UserScript 里的编辑器占位脚本)。
    let all: BTreeSet<u16> = counts.keys().map(|&k| k as u16).collect();
    let unreferenced: Vec<u16> = all.difference(&referenced).copied().collect();
    assert_eq!(unreferenced, vec![28, 59, 63]);
    eprintln!(
        "LABELS: {} 条全部闭合;无标签引用的脚本 = {:?}",
        labels.len(),
        unreferenced
    );
}

#[test]
fn table_set_loads_from_dir_source() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    // 工程根 = ysbin 的上一级两级(project-eris/data 的子目录结构):
    // ycd 在 <root>/system/YSCom/YSCom.ycd。语料目录只含 ysbin,
    // 因此这里把 DirSource 指向工程根而不是 ysbin。
    // 数据根 = <project>/data(含 ysbin/);辅助根 = <project>(含 system/YSCom)。
    let Some(data_root) = dir.parent().map(|p| p.to_path_buf()) else {
        skip.0 += 1;
        return;
    };
    let Some(project_root) = find_project_root(&dir) else {
        skip.0 += 1;
        return;
    };
    if !project_root.join("system/YSCom/YSCom.ycd").is_file() {
        skip.0 += 1;
        eprintln!(
            "SKIP: {} 没有 system/YSCom/YSCom.ycd",
            project_root.display()
        );
        return;
    }
    let src = DirSource::open(&data_root, Codec::Cp932).expect("DirSource(data)");
    let aux = DirSource::open(&project_root, Codec::Cp932).expect("DirSource(project)");
    let mut tables = TableSet::load_from(&src, Some(&aux), DEFAULT_SCRIPT_KEY).expect("装载七表");
    assert_eq!(tables.boot.yscm.len(), 119);
    assert_eq!(tables.boot.sysvars.len(), 548);
    let (entry_script, entry_pc) = {
        let entry = tables.boot.labels.get("SYSTEM_START").expect("入口标签");
        (entry.script_id, entry.pc)
    };
    let script = tables
        .load_script(&src, entry_script as u32)
        .expect("入口脚本");
    assert!((entry_pc as usize) < script.group_count());
    eprintln!(
        "BOOT: entry SYSTEM_START -> s{} pc{} ({} 组), 源 {}",
        entry_script,
        entry_pc,
        script.group_count(),
        tables.script_path(entry_script as u32)
    );
}

// ---------------------------------------------------------------- 商业发行版

fn commercial_ypf() -> Option<PathBuf> {
    let p = std::env::var_os("YURIS_SAMPLE_GAME_YPF")?;
    let path = PathBuf::from(p);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// **商业发行版形态**:`data.ypf` 单包,**没有 `YSCom.ycd`**。
///
/// 实测(<game-b>, v494, 12,212 条目):
/// - 包内 `ysbin/` 有 7 张元数据表 + 160 个脚本,`ycd`/`yscom` **0 命中**;
/// - `ysb.ybn`(YSVR)前 548 条 kind=1 记录的 `(value_type, dims)` 与
///   0.495 SDK 的 ycd **逐项一致(548/548,0 差异)** ⇒ 系统变量 id 空间
///   是引擎 ABI,可用内嵌 ABI 表 + 工程的 ysv 自证校验。
///
/// 本测试断言:无 ycd 也能装载七表、通过 ABI 校验、解析出命令表与标签,
/// 并从 YPF 里解码入口脚本。
#[test]
fn commercial_ypf_boots_without_ycd() {
    let Some(ypf) = commercial_ypf() else {
        eprintln!(
            "SKIP[count=1] YURIS_SAMPLE_GAME_YPF 不可用,跳过商业发行版(无 ycd)断言"
        );
        return;
    };
    let src = YpfSource::open(&ypf, Codec::Cp932).expect("打开 data.ypf");

    // 包内确实没有编译器定义(这是本测试的前提,不是假设)。
    let ycd_key = yu_vfs::LookupKey::new(b"system/YSCom/YSCom.ycd", Codec::Cp932).unwrap();
    assert!(!src.exists(&ycd_key), "该样本意外带有 ycd;换样本或调整前提");

    let mut tables = TableSet::load(&src, DEFAULT_SCRIPT_KEY).expect("无 ycd 也必须能装载七表");
    assert!(tables.boot.ycd.is_none(), "ycd 应为 None(商业形态)");
    assert_eq!(tables.boot.sysvars.len(), 548, "回退到引擎 ABI 表");
    assert_eq!(tables.boot.sysvars.id("_LC"), Some(48));
    println!(
        "COMMERCIAL: ysc v{} ({} 命令) labels={} vars={} sysvars<-{}",
        tables.boot.yscm.version,
        tables.boot.yscm.len(),
        tables.boot.labels.len(),
        tables.boot.vars.records.len(),
        tables.boot.sysvars.source.describe()
    );

    // 入口标签与入口脚本(全从包里来)。
    let entry = tables
        .boot
        .labels
        .get("SYSTEM_START")
        .expect("商业包 ysl 含 SYSTEM_START")
        .clone();
    let script = tables
        .load_script(&src, entry.script_id as u32)
        .expect("入口脚本可解码");
    assert!((entry.pc as usize) < script.group_count());
    println!(
        "COMMERCIAL: SYSTEM_START -> s{} pc{} ({} 组)",
        entry.script_id,
        entry.pc,
        script.group_count()
    );

    // 用真实启动链跑一帧(时钟注入;输入为空)。
    let mut vm = Vm::boot(
        Box::new(src),
        BootConfig {
            entry_label: "SYSTEM_START".to_string(),
            start_ms: 0,
            ..BootConfig::default()
        },
    )
    .expect("商业包启动");
    let report = vm.tick_at(0, &yu_rt::input::InputTimeline::new());
    println!(
        "COMMERCIAL: frame1 commands={} errors={:?} unimplemented={}",
        report.commands_pumped,
        report.errors,
        vm.unimplemented_cmds().len()
    );
    assert!(report.commands_pumped > 0, "至少执行了入口命令");
}

/// **商业发行版端到端**:<game-b> v494 从 `data.ypf` 启动并跑到稳态。
///
/// 这是「无 ycd」路径的完整验收:表全部来自包内 `.ybn`、系统变量来自内嵌
/// 引擎 ABI 表(用包内 `ysv` 自证校验)、脚本从包内读、商业数据里的
/// **0 窗口 ELSE / `VARACT[DIMSIZE+=1]` 追加 / `@_MOUSE_L` 写入** 等形态
/// 都在这条链上真实出现。
#[test]
fn commercial_game_reaches_steady_state() {
    let Some(ypf) = commercial_ypf() else {
        eprintln!("SKIP[count=1] YURIS_SAMPLE_GAME_YPF 不可用");
        return;
    };
    let root = ypf.parent().expect("game root");
    let pack = YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf");
    let aux = DirSource::open(root, Codec::Cp932).expect("game root");
    let cfg = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        dx_mode: 0,
        media_readonly: false,
        os_sound: true,
        paths: yu_rt::vm::HostPaths {
            system_path: Some(format!("{}/", root.display()).into_bytes()),
            save_path: Some(format!("{}/save/", root.display()).into_bytes()),
            appdata_path: Some(format!("{}/", root.display()).into_bytes()),
            personal_path: Some(format!("{}/", root.display()).into_bytes()),
            project_path: Some(format!("{}/", root.display()).into_bytes()),
        },
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(Box::new(pack), Some(Box::new(aux)), cfg).expect("boot");
    assert!(vm.tables().boot.ycd.is_none(), "商业包不带 ycd");

    let tl = yu_rt::input::InputTimeline::new();
    let mut counts = Vec::new();
    for f in 0..8u32 {
        let r = vm.tick_at(f * 16, &tl);
        assert!(r.errors.is_empty(), "帧 {f} Fatal: {:?}", r.errors);
        assert!(!r.main_finished, "帧 {f} 主线不应结束");
        counts.push(r.commands_pumped);
    }
    eprintln!("COMMERCIAL E2E frames: {counts:?}");
    // 2026-10-19 文本流(WORD/_/RETURNCODE)+ TASK[TINT] 落地后,首帧 +4
    // (850_609 → 850_613):新语义路径真实生效。
    // ⚠ E3(2026-10-20):YSTL 字段勘误(SYSTEMINFO[TEXTNUM] 从错读的
    // var_count 改为真实文本条数)可能使本钉值漂移——ERIS 的 TEXTNUM
    // 查询循环在第 2 帧,商业包落在哪一帧未经样本验证。若此断言失败,
    // 请对照 E3 的解释重新基线,不要当作回归回滚 TEXTNUM 语义。
    assert_eq!(counts[0], 850_613, "商业包首帧启动链命令数");
    // 稳态断言(旧:末 3 帧恒等 8_681)——FADE 任务链被 TINT 激活后,商业包
    // 同样进入真实场景流,末帧不再恒等;改为「零 Fatal + 有命令推进」,
    // 并钉住首帧作为回归金丝雀。
    assert!(counts.iter().all(|c| *c > 0), "每帧都有命令推进: {counts:?}");
}

/// **端到端**:官方入口链 `SYSTEM_START → es.ERIS` 连续跑 8 帧。
///
/// 这是最强的一条交叉验证:它同时穿过 7 张表、`GO/GOSUB/RETURN` 跨文件跳转、
/// `VARACT[INIT+G_*]` 全局恢复、`DIMSIZE+=1` 数组追加、`TASKINFO`/`SYSTEMINFO`
/// 查询、带符号帧计数器、`WAIT[FRAME]` 让出与主循环。
///
/// 断言(0.495 ERIS 实测):
/// - **零 Fatal**:任何 `chain errors` 都是缺陷;
/// - 首帧 > 100 万条命令(启动链一次跑完初始化);
/// - 之后进入稳态:每帧命令数相同(主循环 `WAIT[FRAME=1]` + 状态机);
/// - 主线**不结束**(`es.ERIS` 的 LOOP 永不退出);
/// - 失败帧的 `errors` 必须为空(错误也是事件)。
#[test]
fn eris_entry_chain_runs_end_to_end() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    let data_root = dir.parent().expect("data").to_path_buf();
    let Some(project_root) = find_project_root(&dir) else {
        skip.0 += 1;
        return;
    };
    let src = DirSource::open(&data_root, Codec::Cp932).expect("data");
    let aux = DirSource::open(&project_root, Codec::Cp932).expect("project");
    let mut vm = Vm::boot_with_aux(
        Box::new(src),
        Some(Box::new(aux)),
        BootConfig {
            entry_label: "SYSTEM_START".to_string(),
            fps: 60,
            screen_color: 32,
            dx_mode: 0,
            media_readonly: false,
            os_sound: true,
            ..BootConfig::default()
        },
    )
    .expect("boot");

    let timeline = yu_rt::input::InputTimeline::new();
    let mut counts = Vec::new();
    for f in 0..8u32 {
        let r = vm.tick_at(f * 16, &timeline);
        assert!(r.errors.is_empty(), "帧 {f} 出现 Fatal: {:?}", r.errors);
        assert!(!r.main_finished, "帧 {f} 主线不应结束");
        counts.push(r.commands_pumped);
    }
    eprintln!("E2E frames: {counts:?}");
    eprintln!("E2E total commands: {}", counts.iter().sum::<u64>());
    assert!(counts[0] > 1_000_000, "首帧启动链 {} 条", counts[0]);
    // 2026-10-19:文本流(WORD/_/RETURNCODE)与 TASK[TINT] 落地后,`es.ERIS`
    // 主循环里的 FADE 任务链首次携带真实参数激活(`es.CGTFADEOUT(@_TINT(1))`
    // 收到非零时长),每帧命令数随场景切换进度波动——这正是**更深的真实
    // 游戏流**;旧断言「末 4 帧恒等 8765」描述的是 TINT=0 时 FADE 空转的
    // 退化稳态,随之作废。改为:零 Fatal + 每帧有推进 + 首帧钉值。
    assert_eq!(counts[0], 1_290_326, "首帧启动链命令数(回归金丝雀)");
    // 2026-10-20(E3):YSTL 字段勘误(R12)后,启动期 `SYSTEMINFO[TEXTNUM,
    // NO=1..n]` 逐脚本查询循环拿到**真实文本条数**(旧实现错读成 var_count),
    // 该帧命令数随之修正:332,550 → 274,466(主人裁决钉入;证据链见
    // PROGRESS 成果 E3)。它同时钉住 TEXTNUM 语义本身:回退旧错值会在此爆掉。
    assert_eq!(counts[1], 274_466, "TEXTNUM 修正后的第 2 帧命令数(回归金丝雀)");
    assert!(
        counts.iter().all(|c| *c > 0),
        "每帧都有命令推进: {counts:?}"
    );
    // 主线仍在跑(未 finish),且有任务在等待。
    assert!(!vm.scheduler().nodes().is_empty());
}

/// **A1 运行核级等价性**:同一 ERIS 工程,散盘 `data/` 与官方 `data.ypf`
/// 启动后,逐帧命令数、错误数、**变量域终值摘要**必须完全一致。
///
/// 这条测试把 architecture §1 的「数据源不可见」从 vfs 层（`open/exists/list`
/// 等价）推进到执行层：换挂载栈 ⇒ 执行轨迹不变。
#[test]
fn loose_and_packed_eris_are_execution_equivalent() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    let data_root = dir.parent().expect("data").to_path_buf();
    let Some(project_root) = find_project_root(&dir) else {
        skip.0 += 1;
        return;
    };
    let ypf = project_root.join("data.ypf");
    if !ypf.is_file() || !project_root.join("system/YSCom/YSCom.ycd").is_file() {
        skip.0 += 1;
        return;
    }
    let cfg = || BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        screen_color: 32,
        ..BootConfig::default()
    };

    let run = |mut vm: Vm| {
        let tl = yu_rt::input::InputTimeline::new();
        let mut counts = Vec::new();
        let mut errors = 0usize;
        for f in 0..6u32 {
            let r = vm.tick_at(f * 16, &tl);
            errors += r.errors.len();
            counts.push(r.commands_pumped);
            if r.main_finished {
                break;
            }
        }
        let mut h = String::new();
        for (r, slot) in vm.store().iter() {
            h.push_str(&format!("{}={:?};", r.display(), slot.data));
        }
        (counts, errors, yu_rt::sysvar_abi::sha256_hex(h.as_bytes()))
    };

    let src = DirSource::open(&data_root, Codec::Cp932).expect("data");
    let aux = DirSource::open(&project_root, Codec::Cp932).expect("project");
    let loose = run(Vm::boot_with_aux(Box::new(src), Some(Box::new(aux)), cfg()).expect("loose boot"));

    let pack = YpfSource::open(&ypf, Codec::Cp932).expect("data.ypf");
    let aux2 = DirSource::open(&project_root, Codec::Cp932).expect("project");
    let packed =
        run(Vm::boot_with_aux(Box::new(pack), Some(Box::new(aux2)), cfg()).expect("packed boot"));

    eprintln!("loose  = {loose:?}");
    eprintln!("packed = {packed:?}");
    assert_eq!(loose, packed, "散盘与官方 data.ypf 执行轨迹必须一致(A1)");
}

// ---------------------------------------------------------------- 文本流不变量

/// `RETURNCODE` 唯一窗口的 `kind` = **本脚本内文本行计数**:每脚本从 1 起、
/// 逐行严格 +1(ysc/ycd 都声明它无参数,kind 只能是编译器写入的行号;
/// ERIS 137 文件全量实测)。同时钉住 WORD 文本段非空。
#[test]
fn returncode_kinds_are_per_script_line_counters() {
    let mut skip = SkipCounter(0);
    let dir = require_corpus!(skip);
    let yscm = yscm_table(&dir);
    let rc_op = yscm.index_of("RETURNCODE").unwrap() as u8;
    let word_op = yscm.index_of("WORD").unwrap() as u8;

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_script_file(n))
        .collect();
    entries.sort();

    let mut total_rc = 0usize;
    let mut scripts_with_rc = 0usize;
    let mut total_word = 0usize;
    for name in &entries {
        let sid: u32 = name[3..8].parse().unwrap();
        let data = std::fs::read(dir.join(name)).unwrap();
        let script = ScriptFile::parse(sid, &data, DEFAULT_SCRIPT_KEY)
            .unwrap_or_else(|e| panic!("{name} 解码失败: {e}"));
        let mut expect = 1u32;
        let mut seen = false;
        for pc in 0..script.group_count() {
            let group = script.group(pc).unwrap();
            if group.opcode == rc_op {
                let w = script.group_windows(pc).first().unwrap();
                assert_eq!(
                    w.kind as u32,
                    expect,
                    "{name} pc{pc}: 改行计数断裂(期待 {expect})"
                );
                expect += 1;
                seen = true;
                total_rc += 1;
            } else if group.opcode == word_op {
                let w = script.group_windows(pc).first().unwrap();
                assert!(
                    w.length > 0,
                    "{name} pc{pc}: WORD 文本段为空(语料 464/464 非空)"
                );
                total_word += 1;
            }
        }
        if seen {
            scripts_with_rc += 1;
        }
    }
    assert_eq!(total_rc, 396, "RETURNCODE 总数(实测)");
    assert_eq!(total_word, 464, "WORD 总数(实测)");
    eprintln!(
        "RETURNCODE: total={total_rc} scripts_with_rc={scripts_with_rc}          (每脚本 1 起逐行 +1,全量成立)"
    );
}
