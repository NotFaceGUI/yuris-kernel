//! 取证探针:从**渲染结果**读取菜单按钮的标签与位置,生成点击点。
//!
//! 菜单按钮在 ERIS 里的构造链是
//! `es.POSDATA.SET`(算几何)→ `CG`(建层)→ `CGACT[TEXT]`(把标签画进层)。
//! 所以「标签 ↔ 层 ↔ 屏幕矩形」三者的权威映射在 `CompositorFrame.text_draws`
//! 与图层几何里,不需要也不应该靠截图估坐标。
//!
//! 用法:
//!   `cargo run -p yu-host-desktop --example probe_buttons -- <project-root>
//!    [--frames N] [--click F,X,Y]... [--label 部分文字]`
//!
//! 输出(`--label` 过滤,按屏幕 y 升序):每行给出标签原文、目标层、
//! 层左上角与尺寸、以及可直接写进场景 JSON 的点击中心。
use std::path::Path;

use yu_engine::engine::{Backend, Engine, EngineOptions};
use yu_engine::project::Project;
use yu_rt::input::{InputFrame, InputTimeline};

fn main() {
    if let Err(e) = run() {
        eprintln!("probe_buttons: {e}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or("用法: probe_buttons <project-root> [--frames N] [--click F,X,Y]... [--label TEXT]")?;
    let mut frames = 520u32;
    let mut clicks: Vec<(u64, i64, i64)> = Vec::new();
    let mut label_filter = String::new();
    let mut backend = Backend::Soft;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--frames" => {
                frames = args[i + 1].parse().map_err(|_| "frames 需要整数")?;
                i += 2;
            }
            "--click" => {
                let mut p = args[i + 1].split(',');
                let f: u64 = p.next().ok_or("click 缺 frame")?.parse().map_err(|_| "frame")?;
                let x: i64 = p.next().ok_or("click 缺 x")?.parse().map_err(|_| "x")?;
                let y: i64 = p.next().ok_or("click 缺 y")?.parse().map_err(|_| "y")?;
                clicks.push((f, x, y));
                i += 2;
            }
            "--label" => {
                label_filter = args[i + 1].clone();
                i += 2;
            }
            "--wgpu" => {
                backend = Backend::Wgpu;
                i += 1;
            }
            other => return Err(format!("未知参数 {other}")),
        }
    }

    let mut timeline = InputTimeline::new();
    clicks.sort_by_key(|c| c.0);
    for &(f, x, y) in &clicks {
        if f > 0 {
            timeline.set(f - 1, InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y));
        }
        timeline.set(f, InputFrame::new(["MOUSE_L"]).with_cursor(x, y));
        timeline.set(f + 1, InputFrame::new(std::iter::empty::<&str>()).with_cursor(x, y));
    }

    let project = Project::discover(Path::new(root), None).map_err(|e| e.message)?;
    let mut engine = Engine::new(
        project,
        EngineOptions {
            backend,
            record_events: false,
            ..EngineOptions::default()
        },
    )
    .map_err(|e| e.message)?;
    engine.set_timeline(timeline);
    for _ in 0..frames {
        engine.tick().map_err(|e| e.message)?;
    }

    let frame = engine.last_frame().ok_or("没有帧")?;
    // `text_draws` 是「标签画进哪个层」的权威记录;同一层可能有多段文本
    // (例如数字 + 名字),按层聚合后取层几何。
    let mut rows: Vec<(String, String, i64, i64, Option<yu_gfx::SizeU32>, i64, i64)> = Vec::new();
    for draw in &frame.text_draws {
        let Ok(text) = yu_format::Codec::Cp932.decode(&draw.bytes, &draw.bytes) else {
            continue;
        };
        if !label_filter.is_empty() && !text.contains(&label_filter) {
            continue;
        }
        let Some(layer) = frame.layers.iter().find(|l| l.id == draw.target) else {
            continue;
        };
        if !layer.visible {
            continue;
        }
        let size = layer.image_size();
        let (cx, cy) = size
            .map(|s| {
                (
                    layer.transform.x + i64::from(s.width) / 2,
                    layer.transform.y + i64::from(s.height) / 2,
                )
            })
            .unwrap_or((layer.transform.x, layer.transform.y));
        rows.push((
            text,
            draw.target.display(),
            layer.transform.x,
            layer.transform.y,
            size,
            cx,
            cy,
        ));
    }
    rows.sort_by_key(|r| (r.6, r.5));
    println!("== 可见按钮(标签来自 CGACT[TEXT];位置来自层几何)== frame {}", frame.frame);
    for (text, id, x, y, size, cx, cy) in &rows {
        println!(
            "  y={cy:<5} x={cx:<5} click=({cx},{cy})  label={text:?}\n      layer={id} xy=({x},{y}) size={size:?}"
        );
    }
    if rows.is_empty() {
        println!("  (没有匹配的可见文本按钮;换 --label 或加 --frames)");
    }

    // 屏幕 pass 里**实际被绘制**的图层(命中判定用的可视矩形来源)。
    // 按 y 升序给出每层的左上角/尺寸/中心;按钮矩形就是这些层的几何。
    println!("
== BG/立绘/CG 层(全部可见,按 z 升序)==");
    let mut big: Vec<&yu_gfx::Layer> = frame
        .layers
        .iter()
        .filter(|l| {
            !l.is_group()
                && l.visible
                && l.texture != yu_gfx::TextureRef::None
                && !l.id.display().contains("BT.")
                && !l.id.display().contains("FONTL")
        })
        .collect();
    big.sort_by_key(|l| (l.z, l.seq));
    for l in big.iter().take(30) {
        println!(
            "  z={} {} xy=({},{}) size={:?} a={} tex={:?} status={:?}",
            l.z,
            l.id.display(),
            l.transform.x,
            l.transform.y,
            l.image_size(),
            l.alpha,
            l.texture,
            l.status
        );
    }

    println!("
== 图像/复制操作解析检查 ==");
    for pass in &frame.passes {
        for op in &pass.ops {
            match op {
                yu_gfx::Op::Image(img) => {
                    let src_live = frame.layers.iter().any(|l| l.id == img.source);
                    let tgt_layer = img.target.as_ref().and_then(|t| frame.layers.iter().find(|l| &l.id == t));
                    let need_snapshot = tgt_layer
                        .map(|l| matches!(l.texture, yu_gfx::TextureRef::Snapshot(_)))
                        .unwrap_or(false);
                    println!(
                        "  IMG {:?} src={} src_live={src_live} tgt={} tgt_tex_snapshot={need_snapshot} status={:?}",
                        img.action,
                        img.source.display(),
                        img.target.as_ref().map(|t| t.display()).unwrap_or_default(),
                        img.status
                    );
                }
                yu_gfx::Op::Copy(c) => {
                    let src_live = match &c.source {
                        yu_gfx::CopySource::Layer(l) => frame.layers.iter().any(|x| &x.id == l),
                        yu_gfx::CopySource::Group(g) => frame.layers.iter().any(|x| &x.id == g),
                    };
                    println!(
                        "  COPY {:?} src_live={src_live} frozen={} dest={} status={:?}",
                        c.kind,
                        c.source_texture.is_some(),
                        c.dest.display(),
                        c.status
                    );
                }
                _ => {}
            }
        }
    }

    println!("
== 全部 Asset 纹理层(按 z 升序,前 40)==");
    let mut assets: Vec<&yu_gfx::Layer> = frame
        .layers
        .iter()
        .filter(|l| matches!(l.texture, yu_gfx::TextureRef::Asset(_)))
        .collect();
    assets.sort_by_key(|l| (l.z, l.seq));
    for l in assets.iter().take(40) {
        let key = match &l.texture {
            yu_gfx::TextureRef::Asset(k) => yu_format::Codec::Cp932
                .decode(k, k)
                .unwrap_or_else(|_| format!("{k:?}")),
            _ => String::new(),
        };
        println!(
            "  z={} vis={} {} xy=({},{}) size={:?} a={} file={key}",
            l.z,
            l.visible,
            l.id.display(),
            l.transform.x,
            l.transform.y,
            l.image_size(),
            l.alpha
        );
    }

    println!("
== 屏幕 pass 绘制层(按 y 升序;前 40)==");
    let mut drawn: Vec<(i64, i64, Option<yu_gfx::SizeU32>, String, i16)> = Vec::new();
    for pass in &frame.passes {
        if !matches!(pass.target, yu_gfx::PassTarget::Screen) {
            continue;
        }
        for op in &pass.ops {
            if let yu_gfx::Op::Layer(layer) = op {
                if !layer.visible {
                    continue;
                }
                drawn.push((
                    layer.transform.y,
                    layer.transform.x,
                    layer.image_size(),
                    layer.id.display(),
                    layer.alpha,
                ));
            }
        }
    }
    drawn.sort_by_key(|d| (d.0, d.1));
    for (y, x, size, id, alpha) in drawn.iter().take(40) {
        let click = size
            .map(|s| {
                format!(
                    "({},{})",
                    x + i64::from(s.width) / 2,
                    y + i64::from(s.height) / 2
                )
            })
            .unwrap_or_else(|| format!("({x},{y})"));
        println!("  y={y:<5} xy=({x},{y}) size={size:?} click={click} a={alpha} {id}");
    }
    Ok(())
}
