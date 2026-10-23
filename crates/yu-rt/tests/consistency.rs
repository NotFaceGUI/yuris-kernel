//! 同源一致性(R12 验收 6):`yu-format` 直接解码 ↔ `yu-rt` 适配层投影
//! 逐字段比对,跑在官方 ERIS 语料上。
//!
//! 保证「唯一解码来源」不漂移:适配层任何字段错位/漏投影都在这里爆掉。
//! 缺样本 = 显式 skip 并计数;`YURIS_REQUIRE_SAMPLES=1` 时硬失败(AGENTS §2)。

use std::path::PathBuf;

use yu_format::key::DEFAULT_KEY_BYTES;
use yu_format::yscm::CommandTable as FmtYscm;
use yu_format::yslb::YslLabels as FmtYsl;
use yu_format::ystb::Segments;
use yu_format::ystl::ScriptList as FmtYstl;
use yu_format::ysvr::VarDecls as FmtYsv;
use yu_rt::tables::{ParamKind, ScriptFile, TableSet, VarInit, YscmTable, DEFAULT_SCRIPT_KEY};
use yu_rt::ycd::YcdTable;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource};

struct Skips(u32);
impl Skips {
    fn skip(&mut self, name: &str, reason: &str) -> ! {
        if std::env::var_os("YURIS_REQUIRE_SAMPLES").is_some_and(|v| v != "0" && !v.is_empty()) {
            panic!("YURIS_REQUIRE_SAMPLES=1 but sample unavailable: {name}: {reason}");
        }
        self.0 += 1;
        println!("SKIP[count={}] {name}: {reason}", self.0);
        unreachable!("skip 必须让调用方返回")
    }
}

fn corpus_dir(name: &str, skips: &mut Skips) -> PathBuf {
    match std::env::var_os("YURIS_SAMPLE_ERIS_YSBIN") {
        Some(p) if PathBuf::from(&p).is_dir() => PathBuf::from(p),
        _ => skips.skip(name, "YURIS_SAMPLE_ERIS_YSBIN 不可用"),
    }
}

/// 工程根 = 向上最近一个含 `system/YSCom/YSCom.ycd` 的祖先目录。
///
/// ysbin 与工程根之间不假定固定层级;以工程根标记定位,不认任何中间目录名。
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

fn read(dir: &std::path::Path, name: &str) -> Vec<u8> {
    std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("读 {name} 失败: {e}"))
}

/// ysc/ysl/ysv/yst_list/yscfg:版本、逐条字段、索引访问全部同源。
#[test]
fn adapter_projection_matches_format_metadata() {
    let mut skips = Skips(0);
    let dir = corpus_dir("adapter_projection_matches_format_metadata", &mut skips);
    let origin = |n: &str| format!("ysbin/{n}").into_bytes();

    // ---- YSCM:名字/参数名/种类/检查器/index_of ----
    let fysc = FmtYscm::parse(&read(&dir, "ysc.ybn"), &origin("ysc.ybn")).unwrap();
    let rtsc = YscmTable::from_format(fysc.clone()).unwrap();
    assert_eq!(rtsc.version, fysc.version, "ysc version");
    assert_eq!(rtsc.len(), fysc.commands.len(), "ysc 命令条数");
    for (i, (fc, rc)) in fysc.commands.iter().zip(&rtsc.commands).enumerate() {
        assert_eq!(rc.name, fc.name.as_ref(), "ysc[{i}] 命令名");
        assert_eq!(rc.params.len(), fc.args.len(), "ysc[{}] 参数数", fc.name);
        for (s, (fa, rp)) in fc.args.iter().zip(&rc.params).enumerate() {
            // 命名交叉:format.flag 字节 = 运行核种类码,format.kind = 检查器。
            assert_eq!(rp.name, fa.name.as_ref(), "ysc[{}] 参数 {s} 名", fc.name);
            assert_eq!(
                rp.kind,
                ParamKind::from_code(fa.flag),
                "ysc[{}] 参数 {s}({}) 种类码",
                fc.name,
                fa.name
            );
            assert_eq!(rp.checker, fa.kind, "ysc[{}] 参数 {s} 检查器", fc.name);
        }
        assert_eq!(rtsc.index_of(&rc.name), Some(i), "ysc index_of");
        assert_eq!(
            rtsc.command(i as u8).map(|c| c.name.as_str()),
            Some(fc.name.as_ref())
        );
        if !rc.params.is_empty() {
            assert_eq!(
                rtsc.param(i as u8, 0).map(|p| p.name.as_str()),
                Some(fc.args[0].name.as_ref()),
                "ysc param(0)"
            );
        }
    }

    // ---- YSLB:字节键 → script/pc/base(标签闭环核心) ----
    let fysl = FmtYsl::parse(&read(&dir, "ysl.ybn"), &origin("ysl.ybn")).unwrap();
    let rtsl = yu_rt::tables::LabelTable::from_format(fysl.clone()).unwrap();
    assert_eq!(rtsl.version, fysl.version, "ysl version");
    assert_eq!(rtsl.len(), fysl.entries.len(), "ysl 条数");
    for (fe, rl) in fysl.entries.iter().zip(&rtsl.labels) {
        assert_eq!(rl.name_bytes, fe.name, "标签字节名");
        assert_eq!(rl.name, String::from_utf8_lossy(&fe.name), "标签 lossy 名");
        assert_eq!(rl.hash, fe.field_a, "标签 hash");
        assert_eq!(rl.pc, fe.field_b, "标签 pc");
        assert_eq!(rl.script_id, fe.script_no, "标签脚本号");
        assert_eq!(rl.base_a, fe.field_d, "标签基址 A");
        assert_eq!(rl.base_b, fe.field_e, "标签基址 B");
        // by_name 字节键索引 ↔ format 线性 find,同一条记录。
        let found = rtsl.get_bytes(&fe.name).expect("字节键可查");
        let via_format = fysl.find(&fe.name).expect("format find");
        assert_eq!(
            (found.script_id, found.pc),
            (via_format.script_no, via_format.field_b)
        );
    }

    // ---- YSVR:kind/group/script/slot/type/dims/初值 ----
    let fysv = FmtYsv::parse(&read(&dir, "ysv.ybn"), &origin("ysv.ybn")).unwrap();
    let rtsv = yu_rt::tables::VarImage::from_format(fysv.clone()).unwrap();
    assert_eq!(rtsv.version, fysv.version, "ysv version");
    assert_eq!(rtsv.records.len(), fysv.decls.len(), "ysv 条数");
    for (i, (fd, rd)) in fysv.decls.iter().zip(&rtsv.records).enumerate() {
        assert_eq!(rd.kind, fd.kind, "ysv[{i}] kind");
        assert_eq!(rd.group, fd.flag, "ysv[{i}] group");
        assert_eq!(rd.script_id, fd.file_id, "ysv[{i}] script");
        assert_eq!(rd.var_id, fd.slot, "ysv[{i}] slot");
        assert_eq!(rd.value_type, fd.ty, "ysv[{i}] type");
        assert_eq!(rd.dims, fd.dims, "ysv[{i}] dims");
        match (&fd.init, &rd.init) {
            (yu_format::Init::None, VarInit::None) => {}
            (yu_format::Init::Int(a), VarInit::Int(b)) => assert_eq!(a, b, "ysv[{i}] int 初值"),
            (yu_format::Init::Float(a), VarInit::Float(b)) => {
                assert_eq!(a.to_bits(), b.to_bits(), "ysv[{i}] flt 初值(按位)")
            }
            (yu_format::Init::Str(a), VarInit::Str(b)) => assert_eq!(a, b, "ysv[{i}] str 初值"),
            other => panic!("ysv[{i}] 初值形态错位:{other:?}"),
        }
    }
    // 引擎装载视角过滤一致。
    assert_eq!(rtsv.globals().count(), fysv.loaded_at_init().count());
    assert_eq!(
        rtsv.locals_of(8).count(),
        fysv.loaded_for_script(8).count(),
        "s8 局部变量数"
    );

    // ---- YSTL:index/path/var_count/label_count/text_count/file_time ----
    // 字段定性以 yu-format 为准(用户复核;label_count 与 ysl 互证)。
    let fstl = FmtYstl::parse(&read(&dir, "yst_list.ybn"), &origin("yst_list.ybn")).unwrap();
    let rtstl = yu_rt::tables::ScriptList::from_format(fstl.clone()).unwrap();
    assert_eq!(rtstl.version, fstl.version, "yst_list version");
    assert_eq!(rtstl.len(), fstl.entries.len(), "清单条数");
    for (i, (fe, re)) in fstl.entries.iter().zip(&rtstl.entries).enumerate() {
        assert_eq!(re.index, fe.index, "清单[{i}] index");
        assert_eq!(re.path, fe.name, "清单[{i}] path");
        assert_eq!(re.file_time, fe.file_time, "清单[{}] file_time", re.index);
        assert_eq!(re.var_count, fe.var_count, "清单[{}] var_count", re.index);
        assert_eq!(
            re.label_count, fe.label_count,
            "清单[{}] label_count",
            re.index
        );
        assert_eq!(
            re.text_count, fe.text_count,
            "清单[{}] text_count",
            re.index
        );
        assert_eq!(rtstl.path_of(i as u16), Some(fe.name.as_slice()));
        assert_eq!(rtstl.text_count(i as u16), Some(fe.text_count));
    }

    // ---- YSCF:宽/高 u32(REA FUN_0045dbb0)+ 标题/扩展名序 ----
    let cfg = yu_format::yscf::Yscf::parse(&read(&dir, "yscfg.ybn"), &origin("yscfg.ybn")).unwrap();
    assert_eq!(
        (cfg.width, cfg.height),
        (800, 600),
        "ERIS 工程 800x600(u32)"
    );

    eprintln!(
        "CONSISTENCY: ysc={} ysl={} ysv={} yst_list={} 全部逐字段同源",
        rtsc.len(),
        rtsl.len(),
        rtsv.records.len(),
        rtstl.len()
    );
}

/// yst%05d:ScriptFile 投影 ↔ `Segments` 逐字段(组/窗口/池/part4/源码位置)。
#[test]
fn adapter_projection_matches_format_script_bodies() {
    let mut skips = Skips(0);
    let dir = corpus_dir(
        "adapter_projection_matches_format_script_bodies",
        &mut skips,
    );
    let origin = |n: &str| format!("ysbin/{n}").into_bytes();

    let mut files = 0u32;
    let mut total_groups = 0u64;
    let mut total_windows = 0u64;
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.len() == 12
                && n.starts_with("yst")
                && n.ends_with(".ybn")
                && n[3..8].bytes().all(|b| b.is_ascii_digit())
        })
        .collect();
    entries.sort();
    for name in &entries {
        let sid: u32 = name[3..8].parse().unwrap();
        let bytes = read(&dir, name);
        let segs = Segments::decrypt(&bytes, &DEFAULT_KEY_BYTES, &origin(name)).unwrap();
        let sf = ScriptFile::parse(sid, &bytes, DEFAULT_SCRIPT_KEY)
            .unwrap_or_else(|e| panic!("{name} 投影失败: {e}"));
        assert_eq!(sf.version, segs.version, "{name} version");
        assert_eq!(sf.group_count(), segs.count as usize, "{name} 组数");
        assert_eq!(
            sf.part4(),
            segs.line_numbers,
            "{name} part4 = 行号区(逐字节)"
        );
        assert_eq!(
            sf.pool(),
            [segs.args_data.as_slice(), segs.line_numbers.as_slice()].concat(),
            "{name} 共享池 = args_data ‖ line_numbers"
        );
        // 窗口下标 = 全文件扁平序(参数索引区);组投影按 first_window 切片。
        let mut flat = 0usize;
        for (pc, g) in sf.groups.iter().enumerate() {
            let b = &segs.inst[pc * 4..pc * 4 + 4];
            assert_eq!(g.opcode, b[0], "{name} pc{pc} opcode");
            assert_eq!(g.window_count, b[1], "{name} pc{pc} window_count");
            assert_eq!(
                g.param,
                u16::from_le_bytes([b[2], b[3]]),
                "{name} pc{pc} param"
            );
            for (wi, w) in sf.group_windows(pc).iter().enumerate() {
                let at = flat * 12;
                let wb = &segs.args_index[at..at + 12];
                let expect = (
                    wb[0],
                    wb[1],
                    u16::from_le_bytes([wb[2], wb[3]]),
                    u32::from_le_bytes([wb[4], wb[5], wb[6], wb[7]]),
                    u32::from_le_bytes([wb[8], wb[9], wb[10], wb[11]]),
                );
                assert_eq!(
                    (w.kind, w.flag, w.slot, w.length, w.offset),
                    expect,
                    "{name} pc{pc} 窗口 {wi}(扁平 {flat})"
                );
                // 载荷切片:仅在完全落在 args_data 内时与直切片比对
                // (跨池边界的窗口由下面的 pool 逐字节相等保证)。
                if w.length > 0 {
                    let end = w.offset as usize + w.length as usize;
                    if end <= segs.args_data.len() {
                        assert_eq!(
                            sf.window_payload(w),
                            Some(&segs.args_data[w.offset as usize..end]),
                            "{name} pc{pc} 窗口 {wi} 载荷"
                        );
                    }
                }
                flat += 1;
            }
            total_windows += sf.group_windows(pc).len() as u64;
        }
        assert_eq!(flat, segs.args_index.len() / 12, "{name} 扁平窗口总数");
        assert_eq!(
            sf.windows.len(),
            sf.groups
                .iter()
                .map(|g| g.window_count as usize)
                .sum::<usize>(),
            "{name} Σ窗口数"
        );
        // 源码位置:全组抽查逐 pc(与 part4 dword 一致)。
        for pc in 0..sf.group_count() {
            assert_eq!(
                sf.source_position(pc),
                Some(u32::from_le_bytes(
                    segs.line_numbers[pc * 4..pc * 4 + 4].try_into().unwrap()
                )),
                "{name} pc{pc} source_position"
            );
        }
        files += 1;
        total_groups += sf.group_count() as u64;
    }
    assert_eq!(files, 136, "0.495 ERIS 实际存在 136 个 ystNNNNN.ybn");
    assert_eq!(total_groups, 36_974, "全语料组数(基线)");
    assert_eq!(total_windows, 80_102, "全语料窗口数(基线)");
    eprintln!(
        "CONSISTENCY: yst files={files} groups={total_groups} windows={total_windows} 投影同源"
    );
}

/// TableSet 装配路径(生产启动链)↔ 直接解码:入口标签、config、按需装载
/// 的脚本体与直解一致。
#[test]
fn tableset_matches_direct_decoding() {
    let mut skips = Skips(0);
    let dir = corpus_dir("tableset_matches_direct_decoding", &mut skips);
    let Some(data_root) = dir.parent().map(PathBuf::from) else {
        skips.skip("tableset", "ysbin 无上级目录");
    };
    let Some(project_root) = find_project_root(&dir) else {
        skips.skip("tableset", "向上找不到工程根(含 system/YSCom/YSCom.ycd)");
    };
    let src = DirSource::open(&data_root, Codec::Cp932).expect("DirSource(data)");
    let aux = DirSource::open(&project_root, Codec::Cp932).expect("DirSource(project)");
    let mut tables = TableSet::load_from(&src, Some(&aux), DEFAULT_SCRIPT_KEY).expect("装载七表");

    // config(直接采用 yu-format 类型)与 screen 投影。
    assert_eq!(
        (tables.boot.config.width, tables.boot.config.height),
        (800, 600)
    );
    let ycd_bytes = std::fs::read(project_root.join("system/YSCom/YSCom.ycd")).unwrap();
    let ycd = YcdTable::parse(&ycd_bytes).unwrap();
    assert_eq!(
        tables.boot.ycd.as_ref().map(|t| t.version),
        Some(ycd.version)
    );

    // 入口标签 ↔ ysl 直解。
    let ysl = FmtYsl::parse(&read(&dir, "ysl.ybn"), b"ysbin/ysl.ybn").unwrap();
    let entry = tables.boot.labels.get("SYSTEM_START").expect("入口标签");
    let entry_fmt = ysl.find(b"SYSTEM_START").expect("format 入口标签");
    assert_eq!(
        (entry.script_id, entry.pc),
        (entry_fmt.script_no, entry_fmt.field_b)
    );

    // 按需装载的脚本体 ↔ ScriptFile::parse 直解 ↔ VM 启动后的 screen。
    for sid in [entry.script_id as u32, 8, 10] {
        let loaded = tables.load_script(&src, sid).expect("按需装载");
        let bytes = read(&dir, &format!("yst{sid:05}.ybn"));
        let direct = ScriptFile::parse(sid, &bytes, DEFAULT_SCRIPT_KEY).unwrap();
        assert_eq!(loaded.group_count(), direct.group_count(), "s{sid} 组数");
        assert_eq!(loaded.windows, direct.windows, "s{sid} 窗口");
        assert_eq!(loaded.pool(), direct.pool(), "s{sid} 共享池");
        assert_eq!(loaded.part4(), direct.part4(), "s{sid} part4");
    }

    // VM 启动链的 screen(BOOT config 投影)。
    let vm = Vm::boot_with_aux(
        Box::new(DirSource::open(&data_root, Codec::Cp932).unwrap()),
        Some(Box::new(
            DirSource::open(&project_root, Codec::Cp932).unwrap(),
        )),
        BootConfig {
            entry_label: "SYSTEM_START".to_string(),
            ..BootConfig::default()
        },
    )
    .expect("boot");
    assert_eq!(vm.screen(), (800, 600), "_SCREEN_SIZE_X/Y 真值(u32 来源)");
}

/// 商业发行版形态(真实 ERIS 数据,单根无 ycd):系统变量走内嵌引擎 ABI
/// 兜底时必须留下**可见日志**(`table.fallback`,进诊断汇、不进事件流),
/// 且命令表仍全部来自工程 ysc.ybn(数据驱动,无版本兜底)。
#[test]
fn fallback_abi_logs_notice_on_real_data() {
    let mut skips = Skips(0);
    let dir = corpus_dir("fallback_abi_logs_notice_on_real_data", &mut skips);
    let Some(data_root) = dir.parent().map(PathBuf::from) else {
        skips.skip("fallback", "ysbin 无上级目录");
    };
    // 前提:数据根下确实没有 ycd(散盘工程的 ycd 在工程根,不在 data/)。
    assert!(
        !data_root.join("system/YSCom/YSCom.ycd").is_file(),
        "数据根意外带 ycd,本用例前提不成立"
    );
    let vm = Vm::boot(
        Box::new(DirSource::open(&data_root, Codec::Cp932).expect("DirSource(data)")),
        BootConfig {
            entry_label: "SYSTEM_START".to_string(),
            ..BootConfig::default()
        },
    )
    .expect("单根(无 ycd)也必须能启动");

    // 兜底确实生效:ycd 缺席、系统变量 = 内嵌 ABI(548,已过 ysv 逐项校验)。
    assert!(vm.tables().boot.ycd.is_none(), "ycd 应为 None");
    assert_eq!(vm.sysvars().len(), 548, "系统变量 = 内嵌 ABI 表");
    // 指纹选族:ERIS 的 ysv 应选中 0495 族(不是按版本号)。
    assert_eq!(
        vm.sysvars().source_name(),
        "sysvar-abi-0495.tsv",
        "ERIS ysv 指纹应选中 0495 族"
    );
    // 命令表仍来自工程数据。
    assert_eq!(vm.tables().boot.yscm.len(), 119, "命令表来自工程 ysc.ybn");

    // 兜底日志:诊断汇里必须有 table.fallback(可定位到 YSCom.ycd)。
    let notice = vm
        .diags()
        .iter()
        .find(|d| d.code == "table.fallback")
        .expect("兜底必须有日志提示(table.fallback)");
    assert!(
        notice.detail.contains("ysv.ybn")
            && notice.detail.contains("ysc.ybn")
            && notice.detail.contains("sysvar-abi-0495"),
        "日志应说明所选 ABI 族、校验来源与命令表来源: {}",
        notice.detail
    );
    // 日志只进诊断汇,不进事件流(事件流是对齐对象,golden 不因此变化)。
    assert!(
        !vm.events().iter().any(|e| matches!(
            e,
            yu_rt::VmEvent::Diag { code, .. } if code == "table.fallback"
        )),
        "兜底提示不得进入事件流: {:?}",
        vm.events()
    );
}
