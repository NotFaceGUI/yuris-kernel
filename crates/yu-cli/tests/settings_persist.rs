//! 设置读写闭环（`save/gsd.sd`）：官方文件 → 内核 `LOAD` → 应用（音量）→
//! `GSD.SAVE` 重写。
//!
//! 语料事实（ERIS `save2/data/script/ERIS`）：
//! - `es_GSD.yst:154..165` 启动时 `FILEINFO[EXIST]` + `LOAD[FILE="gsd.sd"
//!   DNO=01 LET=@es.GSD()]`（int 组）、`DNO=02`（str 组）、`DNO=03/04`
//!   （`G_INT2/G_STR2` 组块）；
//! - `es_GSD.yst:558..585` `\GSD.SAVE` → `SAVE[DNO=01 SET=@es.GSD()]`、
//!   `DNO=02 SET=$es.GSD()`、`DNO=03/04 G_INT2/G_STR2=1`、`SAVE[FILE="gsd.sd"]`；
//! - `es_First.yst:139` `GSD.INIT`（读）→ `:201` `SOUND.INIT`（`SETVOL` 应用
//!   主/频道/单条音量）→ `:237` `GSD.REFLECT`；
//! - 设置项目录见 `es_GSD.yst:1..130` 的逐项注释（本测试只钉音量链）。
//!
//! 运行：`YURIS_SAMPLE_ERIS_ROOT=<project-eris> cargo test -p yu-cli --test
//! settings_persist -- --ignored --nocapture`。

use std::path::{Path, PathBuf};

use yu_cli::engine::{Engine, EngineOptions};
use yu_cli::project::Project;
use yu_format::yssd::{
    encode_block_body, encode_multi_body, parse_multi_payload, YssdSave, YssdSubBlockRaw,
    YssdWriter,
};
use yu_rt::input::{InputFrame, InputTimeline};

/// `gsd.sd` 里 `@es.GSD(g, i)` 的线性下标：dims `[256,128]`，行主序。
/// 脚本下标直接参与映射（实测 `(141,1)`=启动回数 → `141*128+1`）。
const GSD_STRIDE: usize = 128;
const fn gsd(group: usize, item: usize) -> usize {
    group * GSD_STRIDE + item
}
const GSD_MASTER_VOLUME: usize = gsd(1, 1);

/// 解析值载荷头（int/str 组通用），返回 `(dims, data 起始偏移)`。
/// 头 = `ty u32 + dim u32 + dims + dlen u32`。
fn value_layout(raw: &[u8]) -> (Vec<u32>, usize) {
    let dim = u32::from_le_bytes(raw[4..8].try_into().expect("dim")) as usize;
    let dims: Vec<u32> = (0..dim)
        .map(|i| u32::from_le_bytes(raw[8 + i * 4..12 + i * 4].try_into().expect("dims 越界")))
        .collect();
    (dims, 8 + dim * 4 + 4)
}

fn sample_root() -> Option<PathBuf> {
    match std::env::var("YURIS_SAMPLE_ERIS_ROOT") {
        Ok(root) => Some(PathBuf::from(root)),
        Err(_) => {
            if std::env::var("YURIS_REQUIRE_SAMPLES").ok().as_deref() == Some("1") {
                panic!("YURIS_REQUIRE_SAMPLES=1 but YURIS_SAMPLE_ERIS_ROOT is missing");
            }
            eprintln!("SKIP settings_persist: no sample root");
            None
        }
    }
}

fn temp_root(name: &str) -> PathBuf {
    // 测试的 cwd 是 crate 目录（crates/yu-cli），产物必须落工作区
    // `target/tmp`，不能污染 crate 目录。
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/yu-cli -> workspace root")
        .join("target/tmp/settings")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp save root");
    dir
}

/// 官方 `gsd.sd` 解析后重编码；`patch_master` 为 `Some(v)` 时把
/// `@es.GSD(001,01)` 改成 v。块结构与官方逐块对应（btype 2 用
/// `encode_multi_body`，外层不整体压缩）。
fn rebuild_gsd(source: &Path, patch_master: Option<i64>) -> Vec<u8> {
    let bytes = std::fs::read(source).expect("read gsd.sd");
    let origin = b"gsd.sd";
    let save = YssdSave::parse(&bytes, origin).expect("parse gsd.sd");
    // 沿用源文件的容器版本。
    let mut writer = YssdWriter::new(save.version);
    for entry in &save.entries {
        match entry.btype {
            0 => {
                let mut raw = entry.decompress(origin).expect("decompress type0");
                if entry.dno == 1 {
                    if let Some(v) = patch_master {
                        let (dims, data_at) = value_layout(&raw);
                        assert_eq!(dims, vec![256, 128], "gsd.sd int 组维数变了");
                        let at = data_at + GSD_MASTER_VOLUME * 8;
                        raw[at..at + 8].copy_from_slice(&v.to_le_bytes());
                    }
                }
                writer.set_block(
                    entry.dno,
                    encode_block_body(entry.btype, entry.strict, entry.var_id, &raw),
                );
            }
            2 => {
                let subs = parse_multi_payload(&entry.compressed, origin, entry.dno)
                    .expect("parse multi payload");
                let mut raws: Vec<(u16, Vec<u8>)> = Vec::new();
                for sub in &subs {
                    raws.push((
                        sub.var_id,
                        sub.decompress(origin, entry.dno).expect("decompress sub"),
                    ));
                }
                let blocks: Vec<YssdSubBlockRaw<'_>> = raws
                    .iter()
                    .map(|(var_id, raw)| YssdSubBlockRaw {
                        var_id: *var_id,
                        raw,
                    })
                    .collect();
                writer.set_block(entry.dno, encode_multi_body(&blocks));
            }
            other => panic!("gsd.sd 出现未知块类型 {other}"),
        }
    }
    writer.build()
}

/// 解析 DNO=1 的 int 组（dims/values）。
fn gsd_ints(path: &Path) -> (Vec<u32>, Vec<i64>) {
    let bytes = std::fs::read(path).expect("read");
    let save = YssdSave::parse(&bytes, b"gsd.sd").expect("parse");
    let entry = save.entries.iter().find(|e| e.dno == 1).expect("DNO=1");
    let raw = entry.decompress(b"gsd.sd").expect("decompress");
    let (dims, data_at) = value_layout(&raw);
    let dlen = u32::from_le_bytes(raw[data_at - 4..data_at].try_into().expect("dlen")) as usize;
    let data = &raw[data_at..data_at + dlen];
    let vals = data
        .chunks_exact(8)
        .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    (dims, vals)
}

/// 解析 DNO=2 的 str 组，返回非空元素 `(下标, 内容)`。
fn gsd_nonempty_strings(path: &Path) -> Vec<(usize, Vec<u8>)> {
    let bytes = std::fs::read(path).expect("read");
    let save = YssdSave::parse(&bytes, b"gsd.sd").expect("parse");
    let entry = save.entries.iter().find(|e| e.dno == 2).expect("DNO=2");
    let raw = entry.decompress(b"gsd.sd").expect("decompress");
    let (dims, data_at) = value_layout(&raw);
    let count: usize = dims.iter().map(|d| *d as usize).product();
    let dlen = u32::from_le_bytes(raw[data_at - 4..data_at].try_into().expect("dlen")) as usize;
    let data = &raw[data_at..data_at + dlen];
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for i in 0..count {
        let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if len > 0 {
            out.push((i, data[cursor..cursor + len].to_vec()));
        }
        cursor += len;
    }
    assert_eq!(cursor, data.len(), "STR 组载荷必须恰好闭合");
    out
}

fn run_frames(engine: &mut Engine, n: u32) {
    for _ in 0..n {
        engine.tick().expect("tick");
    }
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn gsd_master_volume_is_loaded_and_applied() {
    let Some(root) = sample_root() else { return };
    let source = root.join("save/gsd.sd");
    if !source.is_file() {
        panic!("缺语料 {}（不能空跑通过）", source.display());
    }
    let dir = temp_root("master-volume");
    let patched = rebuild_gsd(&source, Some(128));
    std::fs::write(dir.join("gsd.sd"), &patched).expect("write patched gsd.sd");
    // 自证补丁落在 DNO=1 的 GSD(001,01)。
    let (dims, vals) = gsd_ints(&dir.join("gsd.sd"));
    assert_eq!(dims, vec![256, 128]);
    assert_eq!(vals[GSD_MASTER_VOLUME], 128);

    let project = Project::discover(&root, None).expect("open project");
    let opts = EngineOptions {
        os_sound: true,
        save_root: Some(dir.clone()),
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).expect("engine");
    run_frames(&mut engine, 300);

    // 标题 BGM 由 `es.SND` 播在 `BGM/B1`；有效音量 =
    // 主音量(gsd.sd=128) × 频道 LS(256) × 单条(256) = 128。
    let snap = engine.sound_snapshot();
    let bgm = snap
        .iter()
        .find(|l| l.id == b"S0/BGM/B1")
        .unwrap_or_else(|| {
            panic!(
                "标题 BGM 未在 S0/BGM/B1（层: {:?}）",
                snap.iter()
                    .map(|l| String::from_utf8_lossy(&l.id).into_owned())
                    .collect::<Vec<_>>()
            )
        });
    assert_eq!(
        bgm.effective_volume, 128,
        "gsd.sd 的主音量必须经 SOUND.INIT 应用到层树"
    );
    // 自身字段仍是单条音量（256），有效值才含祖先链。
    assert_eq!(bgm.volume, 256);
}

#[test]
#[ignore = "需要真实语料;显式 --ignored 运行"]
fn gsd_settings_survive_resave() {
    let Some(root) = sample_root() else { return };
    let source = root.join("save/gsd.sd");
    if !source.is_file() {
        panic!("缺语料 {}（不能空跑通过）", source.display());
    }
    let dir = temp_root("resave");
    let patched = rebuild_gsd(&source, Some(128));
    std::fs::write(dir.join("gsd.sd"), &patched).expect("write patched gsd.sd");

    let project = Project::discover(&root, None).expect("open project");
    let opts = EngineOptions {
        os_sound: true,
        save_root: Some(dir.clone()),
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).expect("engine");
    // 标题 スタート（217,281）：进入本篇触发 `\GSD.SAVE`（es_GSD.yst:558）。
    let mut timeline = InputTimeline::new();
    timeline.set(300, InputFrame::new(["MOUSE_L"]).with_cursor(217, 281));
    timeline.set(
        301,
        InputFrame::new(Vec::<String>::new()).with_cursor(217, 281),
    );
    engine.set_timeline(timeline);
    run_frames(&mut engine, 470);

    let out = dir.join("gsd.sd");
    let rewritten = std::fs::read(&out).expect("GSD.SAVE 应写出 gsd.sd");
    assert!(
        rewritten != patched,
        "进入本篇后 gsd.sd 必须被重写（启动计数/时间至少会变）"
    );
    // 块结构与官方一致：4 块（DNO 1..4），类型/变量 id/维数不变。
    let official = YssdSave::parse(&std::fs::read(&source).expect("read"), b"gsd.sd").unwrap();
    let now = YssdSave::parse(&rewritten, b"gsd.sd").expect("重写文件必须可解析");
    assert_eq!(now.entries.len(), official.entries.len());
    for (a, b) in official.entries.iter().zip(&now.entries) {
        assert_eq!((a.dno, a.btype, a.var_id), (b.dno, b.btype, b.var_id));
    }
    // 设置值原样保留：主音量仍是 128。
    let (dims, vals) = gsd_ints(&out);
    assert_eq!(dims, vec![256, 128]);
    assert_eq!(vals[GSD_MASTER_VOLUME], 128, "主音量设置必须存活");
    // 与官方文件的差异只允许落在「运行期自动更新」的字段上：
    // 101,01..03 = 窗口 WSX/WSY/MAX；113,01 = 当前字体号；
    // 141,01 = 启动回数；141,03 = 累计启动时间(ms)。
    let (_, official_vals) = gsd_ints(&source);
    let mut diff: Vec<usize> = Vec::new();
    for (i, (a, b)) in official_vals.iter().zip(&vals).enumerate() {
        if a != b {
            diff.push(i);
        }
    }
    let known = [
        GSD_MASTER_VOLUME, // 本测试把主音量改成 128（官方 256）
        gsd(101, 1),       // 窗口 WSX
        gsd(101, 2),       // 窗口 WSY
        gsd(101, 3),       // 窗口 MAX
        gsd(113, 1),       // 当前字体号
        gsd(141, 1),       // 启动回数
        gsd(141, 3),       // 累计启动时间(ms)
    ];
    let unexpected: Vec<usize> = diff
        .iter()
        .copied()
        .filter(|i| !known.contains(i))
        .collect();
    assert!(
        unexpected.is_empty(),
        "出现未预期的设置差异（下标 {:?}）；diff 全表 {:?}",
        unexpected,
        diff
    );
    // 字符串组：只允许 `$es.GSD(113)`（默认字体名）非空。
    let strings = gsd_nonempty_strings(&out);
    for (idx, value) in &strings {
        assert_eq!(*idx, 113, "字符串设置只允许 113(字体名)非空: {:?}", strings);
        assert!(!value.is_empty());
    }
}
