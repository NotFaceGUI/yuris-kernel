//! P0 复现探针(任务 J2):逐 tick 汇总 transition / copy / image 的
//! 可执行性与 skip 原因,并解析 title 相关资产的真实命中情况。
//!
//! 输出三张清单的机读形式(TSV),供 `docs/title-motion-ledger.md` 引用:
//! 1. 每帧 `transition/copy/image` 计数 + 状态;
//! 2. 跳过原因(按 `(kind, reason, target)` 聚合);
//! 3. 资产缺失(按 key 去重,跨帧累计)。
//!
//! 用法:
//!
//! ```text
//! set YURIS_SAMPLE_ERIS_ROOT=<project-eris>
//! cargo run -p yu-backend-soft --example title_probe -- [limit]
//! ```
//!
//! 该示例不做像素工作(不渲染整帧),只用 `TextureStore` 解析被引用的
//! 资产 key;因此它只回答“命令/状态/资产”三件事,画面验收仍由
//! `first_frame` 负责。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use yu_backend_soft::TextureStore;
use yu_gfx::{
    CompositorFrame, GfxConfig, GfxHost, GfxState, ImageAction, Op, OpStatus, PassTarget,
    TextureRef, Viewport,
};
use yu_rt::input::InputTimeline;
use yu_rt::{BootConfig, Vm};
use yu_vfs::{Codec, DirSource, Mount, YpfSource};

fn main() {
    if let Err(message) = run() {
        eprintln!("title_probe: {message}");
        std::process::exit(2);
    }
}

#[derive(Default)]
struct KindCounts {
    transition: u32,
    transition_missing_pair: u32,
    transition_executable: u32,
    copy: u32,
    copy_status: BTreeMap<String, u32>,
    image: u32,
    image_actions: BTreeMap<String, u32>,
    image_status: BTreeMap<String, u32>,
    layers: u32,
    groups: u32,
    texture_passes: u32,
}

fn run() -> Result<(), String> {
    let root = eris_root().ok_or_else(|| {
        "缺少样本:请设置 YURIS_SAMPLE_ERIS_ROOT 或 YURIS_SAMPLE_ERIS_YSBIN".to_string()
    })?;
    let root_path = PathBuf::from(&root);
    let data_dir = root_path.join("data");
    let src: Box<dyn yu_vfs::DataSource> = match ypf_path(&root_path) {
        Some(ypf) => Box::new(
            YpfSource::open(&ypf, Codec::Cp932)
                .map_err(|d| format!("打开 {} 失败: {}", ypf.display(), d.detail))?,
        ),
        None => Box::new(DirSource::open(&data_dir, Codec::Cp932).map_err(|d| {
            format!(
                "打开 {} 失败(且无 data.ypf): {}",
                data_dir.display(),
                d.detail
            )
        })?),
    };
    let aux = DirSource::open(&root_path, Codec::Cp932)
        .map_err(|d| format!("打开 {} 失败: {}", root_path.display(), d.detail))?;
    let boot = BootConfig {
        entry_label: "SYSTEM_START".to_string(),
        fps: 60,
        record_events: false,
        // 标题锚点用工程配置运行值 @_DEBUGMODE=0
        // (<project-eris>/save/data/config/projectconfig.txt DEBUGMODE=0);
        // `YURIS_DEBUG_MODE=1` 走 debug 分支对照(B4:debug=true 已可进 title,
        // DIV-L-001 复核;黑帧根因 = _MOUSE_X2 误判按钮,已修)。
        debug_mode: std::env::var("YURIS_DEBUG_MODE")
            .map(|v| v == "1")
            .unwrap_or(false),
        paths: host_paths(&root_path),
        ..BootConfig::default()
    };
    let mut vm = Vm::boot_with_aux(src, Some(Box::new(aux)), boot)
        .map_err(|d| format!("boot 失败: {}", d.detail))?;
    let viewport = Viewport::new(
        vm.tables().boot.config.width,
        vm.tables().boot.config.height,
    );
    let state = Rc::new(RefCell::new(
        GfxState::from_tables(vm.tables(), GfxConfig::new(viewport))
            .map_err(|d| format!("GfxState 构造失败: {}", d.detail))?,
    ));
    vm.set_graphics_host(Box::new(GfxHost::new(state.clone())));

    let mut mount = Mount::new(Codec::Cp932);
    if let Some(ypf) = ypf_path(&root_path) {
        mount.push(Box::new(YpfSource::open(&ypf, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", ypf.display(), d.detail),
        )?));
    }
    if data_dir.is_dir() {
        mount.push(Box::new(DirSource::open(&data_dir, Codec::Cp932).map_err(
            |d| format!("打开 {} 失败: {}", data_dir.display(), d.detail),
        )?));
    }
    let limit: u32 = env::var("YURIS_TITLE_PROBE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| env::args().nth(1).and_then(|v| v.parse().ok()))
        .unwrap_or(600)
        .max(1);
    let mut store = TextureStore::new(
        Box::new(mount),
        Codec::Cp932,
        state.borrow().frame().image_probe_order.clone(),
    );

    let timeline = InputTimeline::new();
    let mut resolved: BTreeMap<Vec<u8>, Result<(), String>> = BTreeMap::new();
    let mut skipped: BTreeMap<(&'static str, String, String), u32> = BTreeMap::new();
    let mut missing: BTreeMap<Vec<u8>, (String, String, u32)> = BTreeMap::new();
    let mut image_actions: BTreeMap<String, u32> = BTreeMap::new();
    let mut image_status: BTreeMap<String, u32> = BTreeMap::new();
    let mut copy_status: BTreeMap<String, u32> = BTreeMap::new();
    let mut title_events: Vec<String> = Vec::new();
    let mut last_title_key_count = 0usize;

    println!("tick\tframe\tlayers\tvisible\tpasses\ttransition\tcopy\timage\ttrans_missing_pair\ttexpass");
    for tick in 0..limit {
        let _ = vm.tick_at(tick.saturating_mul(16), &timeline);
        let _ = vm.take_events();
        let frame = state.borrow().frame();
        if let Ok(path) = env::var("YURIS_TITLE_PROBE_FULL") {
            if tick + 1 == limit {
                std::fs::write(path, yu_gfx::json::to_json(&frame)).expect("write full json");
            }
        }
        let counts = count_frame(&frame);
        if counts.transition > 0 || counts.copy > 0 || counts.image > 0 {
            println!(
                "{tick}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                frame.frame,
                counts.layers,
                frame.layers.iter().filter(|l| l.visible).count(),
                frame.passes.len(),
                counts.transition,
                counts.copy,
                counts.image,
                counts.transition_missing_pair,
                counts.texture_passes
            );
        }
        collect_skips(&frame, &mut skipped);
        for (name, n) in &counts.image_actions {
            *image_actions.entry(name.clone()).or_insert(0) += n;
        }
        for (name, n) in &counts.image_status {
            *image_status.entry(name.clone()).or_insert(0) += n;
        }
        for (name, n) in &counts.copy_status {
            *copy_status.entry(name.clone()).or_insert(0) += n;
        }
        // 资产:只解析本帧被引用的 key;缓存让重复帧零成本。
        let mut title_keys = 0usize;
        for layer in &frame.layers {
            let TextureRef::Asset(key) = &layer.texture else {
                continue;
            };
            if key.windows(7).any(|w| w == b"cgsys/") || key.windows(4).any(|w| w == b"btn_") {
                title_keys += 1;
            }
            if resolved.contains_key(key) {
                continue;
            }
            store.begin_frame();
            match store.resolve(key) {
                Ok(tex) => {
                    resolved.insert(key.clone(), Ok(()));
                    title_events.push(format!(
                        "resolved {} -> {}x{}",
                        display_bytes(key),
                        tex.width,
                        tex.height
                    ));
                }
                Err(diag) => {
                    resolved.insert(key.clone(), Err(diag.code.to_string()));
                    let entry = missing
                        .entry(key.clone())
                        .or_insert_with(|| (diag.code.to_string(), diag.detail.clone(), 0));
                    entry.2 += 1;
                }
            }
        }
        if title_keys != last_title_key_count {
            title_events.push(format!(
                "tick {tick}: cgsys/btn asset refs = {title_keys} (visible {})",
                frame.layers.iter().filter(|l| l.visible).count()
            ));
            last_title_key_count = title_keys;
        }
    }

    println!("\n# image actions (action\tcount)");
    for (name, n) in &image_actions {
        println!("IMAGE_ACTION\t{name}\t{n}");
    }
    println!("\n# image status (status\tcount)");
    for (name, n) in &image_status {
        println!("IMAGE_STATUS\t{name}\t{n}");
    }
    println!("\n# copy status (status\tcount)");
    for (name, n) in &copy_status {
        println!("COPY_STATUS\t{name}\t{n}");
    }
    println!("\n# skipped (kind\treason\ttarget\tcount)");
    for ((kind, reason, target), count) in &skipped {
        println!("SKIP\t{kind}\t{reason}\t{target}\t{count}");
    }
    println!("\n# assets (key\tcode\tdetail\tframes)");
    for (key, (code, detail, frames)) in &missing {
        println!("ASSET\t{}\t{code}\t{detail}\t{frames}", display_bytes(key));
    }
    println!("\n# trace");
    for line in &title_events {
        println!("TRACE\t{line}");
    }
    println!(
        "\nsummary: ticks={} resolved_keys={} missing_keys={} title_events={}",
        limit,
        resolved.len(),
        missing.len(),
        title_events.len()
    );
    Ok(())
}

fn collect_skips(
    frame: &CompositorFrame,
    skipped: &mut BTreeMap<(&'static str, String, String), u32>,
) {
    for pass in &frame.passes {
        for op in &pass.ops {
            if let OpStatus::Unimplemented(reason) = op.status() {
                let key = (op_kind(op), reason.to_string(), op_target(op));
                *skipped.entry(key).or_insert(0) += 1;
            }
        }
    }
}

fn count_frame(frame: &CompositorFrame) -> KindCounts {
    let mut c = KindCounts::default();
    for pass in &frame.passes {
        if matches!(pass.target, PassTarget::Texture(_)) {
            c.texture_passes += 1;
        }
        for op in &pass.ops {
            match op {
                Op::Layer(_) => c.layers += 1,
                Op::Group(_) => c.groups += 1,
                Op::Transition(t) => {
                    c.transition += 1;
                    if t.to.is_none() {
                        c.transition_missing_pair += 1;
                    }
                    if matches!(op.status(), OpStatus::Executable) {
                        c.transition_executable += 1;
                    }
                }
                Op::Copy(copy) => {
                    c.copy += 1;
                    let status = match copy.status {
                        OpStatus::Executable => "executable".to_string(),
                        OpStatus::Unimplemented(why) => why.to_string(),
                    };
                    *c.copy_status.entry(status).or_insert(0) += 1;
                }
                Op::Image(image) => {
                    c.image += 1;
                    let name = match &image.action {
                        ImageAction::Other(s) => (*s).to_string(),
                        other => other.name().to_string(),
                    };
                    *c.image_actions.entry(name).or_insert(0) += 1;
                    let status = match image.status {
                        OpStatus::Executable => "executable".to_string(),
                        OpStatus::Unimplemented(why) => why.to_string(),
                    };
                    *c.image_status.entry(status).or_insert(0) += 1;
                }
                Op::Mask(_) => {}
            }
        }
    }
    c
}

fn op_kind(op: &Op) -> &'static str {
    match op {
        Op::Layer(_) => "layer",
        Op::Group(_) => "group",
        Op::Transition(_) => "transition",
        Op::Mask(_) => "mask",
        Op::Copy(_) => "copy",
        Op::Image(_) => "image",
    }
}

fn op_target(op: &Op) -> String {
    match op {
        Op::Layer(layer) => layer.id.display(),
        Op::Group(group) => group.layer.id.display(),
        Op::Transition(transition) => transition.layer.id.display(),
        Op::Mask(mask) => mask.layer.id.display(),
        Op::Copy(copy) => copy.dest.display(),
        Op::Image(image) => image.target.as_ref().unwrap_or(&image.source).display(),
    }
}

fn display_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

fn eris_root() -> Option<String> {
    if let Ok(root) = env::var("YURIS_SAMPLE_ERIS_ROOT") {
        return Some(root);
    }
    let ysbin = env::var("YURIS_SAMPLE_ERIS_YSBIN").ok()?;
    Some(Path::new(&ysbin).ancestors().nth(2)?.display().to_string())
}

fn ypf_path(root: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("YURIS_SAMPLE_ERIS_YPF") {
        return Some(PathBuf::from(path));
    }
    let candidate = root.join("data.ypf");
    candidate.exists().then_some(candidate)
}

/// 宿主注入的路径系统变量(手册 `basic/svariable.html`)。
///
/// - `$_SYSTEM_PATH` = YU-RIS 起動パス → 工程根(官方 `yu-ris.exe` 所在目录);
/// - `$_SAVE_PATH` = YU-RIS セーブデータパス → 官方引擎在本工程**实际生成**
///   的存档目录 `<root>/save/`(原版数据自证:`save/*.sd` 由官方 exe 写入);
/// - `$_APPDATA_PATH` = ユーザのホームディレクトリパス → `USERPROFILE`;
/// - `$_PERSONAL_PATH` = マイドキュメントのパス → `USERPROFILE\Documents`。
///
/// `$_PROJECT_PATH` 手册无页(DIV-RT-010),不猜值 ⇒ `None`(仅 debug 脚本读)。
fn host_paths(root: &std::path::Path) -> yu_rt::vm::HostPaths {
    let mut sys = root.to_string_lossy().replace('\\', "/");
    if !sys.ends_with('/') {
        sys.push('/');
    }
    let save = format!("{sys}save/");
    let home = std::env::var("USERPROFILE").ok();
    yu_rt::vm::HostPaths {
        system_path: Some(sys.clone().into_bytes()),
        save_path: Some(save.into_bytes()),
        appdata_path: home.as_ref().map(|h| h.clone().into_bytes()),
        personal_path: home
            .as_ref()
            .map(|h| format!("{h}\\Documents").into_bytes()),
        project_path: None,
    }
}
