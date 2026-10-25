//! 取证探针:逐帧导出匹配前缀的图层状态(是否存在/可见/A/Z/纹理/尺寸)。
//!
//! 用法:`cargo run -p yu-cli --example probe_sprite_frames -- <root> <from> <to> [prefix]`
//!
//! 用途(DIV-GFX-046):判定 `SPL1`/`SPLBUF1` 这类 sprite 缓冲层在**帧末**
//! 是否真的存在并有纹理 —— 也就是"没上屏"发生在脚本层(帧末已被 CGEND 删除)
//! 还是渲染层(层在、后端跳过)。
use std::path::Path;

use yu_cli::engine::{Backend, Engine, EngineOptions};
use yu_cli::project::Project;

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_sprite_frames: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let root = args.next().ok_or("用法: probe_sprite_frames <root> <from> <to> [prefix]")?;
    let from: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(60);
    let to: u64 = args.next().and_then(|v| v.parse().ok()).unwrap_or(100);
    let prefix = args.next().unwrap_or_else(|| "SPL".to_string());

    let project = Project::discover(Path::new(&root), None).map_err(|e| e.message.clone())?;
    let opts = EngineOptions {
        backend: Backend::Soft,
        ..EngineOptions::default()
    };
    let mut engine = Engine::new(project, opts).map_err(|e| e.message.clone())?;
    for f in 0..to {
        engine.tick().map_err(|e| e.message.clone())?;
        if f + 1 < from {
            continue;
        }
        let frame_no = engine.frame();
        let mut lines: Vec<String> = Vec::new();
        if let Some(frame) = engine.last_frame() {
            for layer in &frame.layers {
                let id = layer.id.as_str_lossy();
                if !id.contains(&prefix) {
                    continue;
                }
                let tex = match &layer.texture {
                    yu_gfx::TextureRef::None => "none".to_string(),
                    yu_gfx::TextureRef::Asset(key) => {
                        format!("asset:{}", String::from_utf8_lossy(key))
                    }
                    yu_gfx::TextureRef::SolidBlack1x1 => "black1x1".to_string(),
                    other => format!("{other:?}"),
                };
                let size = layer
                    .image_size()
                    .map(|s| format!("{}x{}", s.width, s.height))
                    .or_else(|| layer.group_size().map(|s| format!("grp{}x{}", s.width, s.height)))
                    .unwrap_or_else(|| "-".to_string());
                lines.push(format!(
                    "    {:<12} vis={} A={:<4} Z={:<8} {} {} status={:?}",
                    id,
                    layer.visible,
                    layer.alpha,
                    layer.z,
                    size,
                    tex,
                    layer.status
                ));
            }
        }
        println!("f{frame_no} layers={}", lines.len());
        for l in lines {
            println!("{l}");
        }
    }
    Ok(())
}
