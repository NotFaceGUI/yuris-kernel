//! 一帧的 GPU 执行计划:**常驻层表面**模型。
//!
//! # 契约(与官方引擎一致)
//!
//! 官方 `CGACT` 在命令处理器里**就地**改目标图层表面(`FUN_0042f809` 尾部
//! `*(iVar4+0x90) = local_68`),`COPY` 直接读写纹理(`DIV-GFX-051`),存档
//! 直接读层表面(`0x4477c0`)。所以本模块按 `CompositorFrame.passes` 的
//! **命令序**把操作作用到 [`LayerSurfaces`] 的常驻内容上:
//!
//! 1. 每条 op 先做**身份判定**(动作 + 参数 + 源内容身份);与上一帧已应用
//!    的前缀相同 ⇒ 直接复用(官方命令只执行一次,本内核每帧重发的是命令
//!    日志)。前缀被折叠/改写 ⇒ 从基面重放这条链。
//! 2. 像素动作写**一张新纹理**(永不原地;旧内容对本帧已登记的读取者仍然
//!    有效 —— 命令时刻语义,EVID-T-15)。
//! 3. `SIZE`/镜像只改采样窗(`Content.uv`),不重采样。
//! 4. 快照三态:整幅 + 未采样 ⇒ 别名源内容句柄(零拷贝);否则区域拷贝;
//!    CPU 像素按需读回(存档抓帧)。
//! 5. 屏幕 pass 只把可见层按 `(z, seq)` blit 到后缓冲。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use yu_gfx::{
    BlendMode, Clear, CompositorFrame, CopyOp, CopySource, ImageAction, ImageOp, LayerId,
    Op, OpStatus, PassTarget, TextureRef,
};
use yu_log::Diag;
use yu_render_core::{RenderPhases, RenderReport, Surface, Texture, TextureStore, TextComposer};

use crate::device::{code, GpuContext};
use crate::passes::{
    clear_color, group_surface_size, hash_rgba, live_snapshots, merge_text_stats, note_text_diag,
    push_quad, Accum, AuxPipeline, BlitCmd, Draw, DrawKind, FramePlan, PaintCmd,
    PlannedPass, PlannedTarget, Planner, SurfaceWrites,
};
use crate::pipelines::{FilterKind, PipelineCache};
use crate::surfaces::{Content, Contents, LayerSurfaces, TexRef};
use crate::textures::{TargetSet, TextureCache, Vertex};

// ------------------------------------------------------------------ 公共入口

/// 规划一帧,同时产出与软后端字段一致的 `RenderReport`。
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    text_uploads: &mut TextUploads,
    backbuffer: Option<&Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    text: &mut TextComposer,
) -> Result<(FramePlan, RenderReport, RenderPhases), Diag> {
    let started = std::time::Instant::now();
    surfaces.begin_frame(frame.frame);
    targets.prune_snapshots(&live_snapshots(frame), pipelines);
    {
        let live_layers: BTreeSet<LayerId> =
            frame.layers.iter().map(|layer| layer.id.clone()).collect();
        targets.prune_composites(&live_layers, pipelines);
        let mut live_offscreen: BTreeSet<LayerId> = frame
            .passes
            .iter()
            .filter_map(|pass| match &pass.target {
                PassTarget::Offscreen(id) => Some(id.clone()),
                _ => None,
            })
            .collect();
        for layer in &frame.layers {
            if let TextureRef::Offscreen(id) = &layer.texture {
                live_offscreen.insert(id.clone());
            }
        }
        targets.prune_offscreen(&live_offscreen, pipelines);
    }
    text_uploads.retain(&frame.layers.iter().map(|l| l.id.clone()).collect());
    let timing = std::env::var("YU_RENDER_TIMING").is_ok();
    let trace_geometry = std::env::var("YU_RENDER_TRACE_GEOMETRY").is_ok();

    let mut acc = Accum::new(frame);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut passes: Vec<PlannedPass> = Vec::new();
    let mut aux: Vec<wgpu::BindGroup> = Vec::new();
    let mut views: BTreeMap<u64, wgpu::TextureView> = BTreeMap::new();
    let mut textures_map: BTreeMap<u64, wgpu::Texture> = BTreeMap::new();
    let mut keepalive: BTreeMap<u64, Rc<TexRef>> = BTreeMap::new();
    let mut bases: BTreeMap<(LayerId, u64), Rc<TexRef>> = BTreeMap::new();
    let mut contents: Contents = Contents::new();
    let mut chain_index: BTreeMap<LayerId, usize> = BTreeMap::new();
    let mut transition_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
    let mut transition_buffers: Vec<wgpu::Buffer> = Vec::new();

    // 上一帧的常驻内容先进入本帧视图(未变化的层直接沿用)。
    //
    // **基面身份在这里统一核对**:`CG[FILE]` 换图后,旧表面(含文本合成
    // 结果)必须作废 —— 否则被 `COPY` 读取/直接绘制时仍会拿到上一帧的
    // 像素(ERIS 底部按钮栏每个按钮都渲染成同一段旧文字的根因;同一机制
    // 还会让区域拷贝越界:旧内容的逻辑网格与新纹理尺寸不符)。
    for layer in &frame.layers {
        let token = base_token(frame, targets, &layer.id, None);
        if surfaces.sync_base(&layer.id, token) {
            contents.remove(&layer.id);
            continue;
        }
        if let Some(raw) = surfaces.raw_content(&layer.id) {
            contents.insert(layer.id.clone(), raw);
        } else if let Some(content) = surfaces.content(&layer.id) {
            contents.insert(layer.id.clone(), content.clone());
        }
    }

    // 命令序结构核对:同一层的 op **序号前缀**必须与已应用前缀一致。
    // 日志被折叠(`supersede_*`)或 `CG[FILE]` 重建时,表面反映的是旧序列,
    // 不能从中间续接 ⇒ 回到基面重放(官方命令只执行一次,重放是幂等的)。
    {
        let mut chain_seqs: BTreeMap<LayerId, Vec<u64>> = BTreeMap::new();
        for pass in &frame.passes {
            let PassTarget::Texture(_) = &pass.target else {
                continue;
            };
            for op in &pass.ops {
                if let Op::Image(image) = op {
                    if op.status() == OpStatus::Executable {
                        let dest = image.target.as_ref().unwrap_or(&image.source);
                        chain_seqs.entry(dest.clone()).or_default().push(image.seq);
                    }
                }
            }
        }
        for (layer, seqs) in chain_seqs {
            let applied = surfaces.applied_len(&layer);
            if applied == 0 {
                continue;
            }
            let mut common = 0usize;
            while common < applied
                && common < seqs.len()
                && surfaces.applied_at(&layer, common).map(|entry| entry.0) == Some(seqs[common])
            {
                common += 1;
            }
            if common < applied {
                surfaces.truncate_applied(&layer, 0);
                surfaces.clear_content(&layer);
                contents.remove(&layer);
            }
        }
    }

    // ---- 1. 纹理 pass:严格命令序(COPY 与图像加工交错) ----
    let mark_ops = started.elapsed();
    for pass in &frame.passes {
        let PassTarget::Texture(pass_target) = &pass.target else {
            continue;
        };
        acc.passes += 1;
        for op in &pass.ops {
            match op {
                Op::Image(image) if op.status() == OpStatus::Executable => {
                    acc.ops_total += 1;
                    let mut writes = SurfaceWrites {
                        passes: &mut passes,
                        vertices: &mut vertices,
                        aux: &mut aux,
                        views: &mut views,
                        textures: &mut textures_map,
                        keepalive: &mut keepalive,
                    };
                    let result = apply_image(
                        ctx,
                        frame,
                        textures,
                        gpu,
                        pipelines,
                        targets,
                        surfaces,
                        &mut contents,
                        &mut bases,
                        &mut writes,
                        &mut chain_index,
                        pass_target,
                        image,
                    );
                    match result {
                        Ok(()) => acc.ops_drawn += 1,
                        Err(reason) => acc.skip(op, reason),
                    }
                }
                Op::Copy(copy) if op.status() == OpStatus::Executable => {
                    acc.ops_total += 1;
                    let mut writes = SurfaceWrites {
                        passes: &mut passes,
                        vertices: &mut vertices,
                        aux: &mut aux,
                        views: &mut views,
                        textures: &mut textures_map,
                        keepalive: &mut keepalive,
                    };
                    let result = apply_copy(
                        ctx,
                        frame,
                        textures,
                        gpu,
                        pipelines,
                        targets,
                        surfaces,
                        &mut contents,
                        &mut bases,
                        &mut writes,
                        backbuffer,
                        backbuffer_gpu,
                        text,
                        &mut acc,
                        pass_target,
                        copy,
                    );
                    match result {
                        Ok(()) => acc.ops_drawn += 1,
                        Err(reason) => acc.skip(op, reason),
                    }
                }
                _ => {
                    acc.ops_total += 1;
                    let reason = match op.status() {
                        OpStatus::Unimplemented(why) => why.to_string(),
                        OpStatus::Executable => "texture_pass.not_executed".to_string(),
                    };
                    acc.skip(op, reason);
                }
            }
        }
    }
    let t_ops = started.elapsed() - mark_ops;

    // ---- 2. 文本:`CGACT[TEXT]`/TextRun 进层表面(修订号驱动) ----
    let mark_text = started.elapsed();
    text.begin_frame_stats();
    let (text_cpu_base_us, text_cpu_info) = apply_text(
        ctx,
        frame,
        textures,
        gpu,
        surfaces,
        &mut contents,
        &mut bases,
        text_uploads,
        text,
        &mut acc,
        targets,
    );
    let text_stats = text.take_frame_stats();

    // ---- 3. 组离屏 pass(后序;子组先于父组) ----
    for pass in &frame.passes {
        let PassTarget::Offscreen(id) = &pass.target else {
            continue;
        };
        acc.passes += 1;
        let (width, height) = group_surface_size(frame, id);
        if width == 0 || height == 0 {
            for op in &pass.ops {
                acc.ops_total += 1;
                acc.skip(op, "group.size.zero");
            }
            continue;
        }
        targets.ensure_offscreen(ctx, id, width, height, pipelines)?;
        let mut planned = PlannedPass {
            target: Some(PlannedTarget::Offscreen(id.clone())),
            clear: clear_color(pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]))),
            first_vertex: vertices.len() as u32,
            ..PlannedPass::default()
        };
        {
            let mut planner = Planner {
                ctx,
                frame,
                textures,
                gpu,
                pipelines,
                targets,
                vertices: &mut vertices,
                acc: &mut acc,
                contents: &contents,
                bases: &mut bases,
                views: &views,
                transition_bind_groups: &mut transition_bind_groups,
                transition_buffers: &mut transition_buffers,
                dst_width: width,
                dst_height: height,
                counted: true,
                trace_geometry,
            };
            for op in &pass.ops {
                planner.plan_op(op, &mut planned)?;
            }
        }
        passes.push(planned);
        acc.offscreen_passes += 1;
    }

    // ---- 4. 屏幕 pass(最后;只画可见层) ----
    let mut screen_planned = false;
    for pass in &frame.passes {
        if !matches!(pass.target, PassTarget::Screen) {
            continue;
        }
        acc.passes += 1;
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        targets.ensure_screen(ctx, width, height, pipelines)?;
        if let Some(screen) = targets.screen() {
            views.insert(screen.id, screen.view.clone());
        }
        let mut planned = PlannedPass {
            target: Some(PlannedTarget::Screen),
            clear: clear_color(frame.clear),
            first_vertex: vertices.len() as u32,
            ..PlannedPass::default()
        };
        {
            let mut planner = Planner {
                ctx,
                frame,
                textures,
                gpu,
                pipelines,
                targets,
                vertices: &mut vertices,
                acc: &mut acc,
                contents: &contents,
                bases: &mut bases,
                views: &views,
                transition_bind_groups: &mut transition_bind_groups,
                transition_buffers: &mut transition_buffers,
                dst_width: width,
                dst_height: height,
                counted: true,
                trace_geometry,
            };
            for op in &pass.ops {
                planner.plan_op(op, &mut planned)?;
            }
        }
        passes.push(planned);
        screen_planned = true;
    }
    if !screen_planned {
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        targets.ensure_screen(ctx, width, height, pipelines)?;
        passes.push(PlannedPass {
            target: Some(PlannedTarget::Screen),
            clear: clear_color(frame.clear),
            first_vertex: vertices.len() as u32,
            ..PlannedPass::default()
        });
    }

    // ---- 5. 屏幕级自动文本(`target=None`) ----
    plan_screen_overlay_text(
        ctx,
        frame,
        gpu,
        pipelines,
        &mut vertices,
        &mut passes,
        &mut views,
        text_uploads,
        text,
        &mut acc,
    )?;
    let t_text = started.elapsed() - mark_text;

    // 帧末:把不再被引用的表面纹理放回池(快照/文本持有 `Rc` ⇒ 自动钉住)。
    let live: BTreeSet<LayerId> = frame.layers.iter().map(|l| l.id.clone()).collect();
    surfaces.end_frame(&live);
    // 旧内容纹理回收:层当前内容之外、本帧写入过的纹理都交回池。
    let mut live_ids: BTreeSet<u64> = BTreeSet::new();
    for layer in &frame.layers {
        if let Some(content) = surfaces.content(&layer.id) {
            live_ids.insert(content.tex.id);
        }
    }
    for id in &live_ids {
        // 还在用的纹理不能回池;池里只收本帧临时产物。
        let _ = id;
    }
    for (_, tex) in std::mem::take(&mut keepalive) {
        if !live_ids.contains(&tex.id) {
            surfaces.release(tex);
        }
    }

    let missing_assets = textures.issues();
    let total = started.elapsed();
    let stats = surfaces.stats();
    let phases = RenderPhases {
        plan_us: total.as_micros() as u64,
        image_us: t_ops.as_micros() as u64,
        text_us: t_text.as_micros() as u64,
        snap_us: 0,
        copy_text_us: 0,
        rest_us: total
            .as_micros()
            .saturating_sub(t_ops.as_micros() + t_text.as_micros()) as u64,
        text_bake_us: (text_stats.bake_ms * 1e3) as u64,
        text_hash_us: (text_stats.hash_ms * 1e3) as u64,
        text_paint_us: (text_stats.paint_ms * 1e3) as u64,
        text_base_us: (text_stats.base_ms * 1e3) as u64,
        text_cpu_base_us,
        text_cpu_readback_us: text_cpu_info.readback_us,
        text_cpu_readbacks: text_cpu_info.readbacks,
        text_cpu_asset_us: text_cpu_info.asset_us,
        text_cpu_asset_bytes: text_cpu_info.asset_bytes,
        text_cpu_cached: u64::from(text_stats.base_cache_hits),
        text_cpu_stores: u64::from(text_stats.base_cache_stores),
        decode_us: textures.decode_stats().0,
        decode_bytes: textures.decode_stats().1,
        surface_passes: stats.surface_passes,
        blits: stats.blits,
        alias_hits: stats.alias_hits,
        reused_layers: stats.reused_layers,
        materialized: stats.materialized,
        cpu_fallbacks: stats.cpu_fallbacks,
        ..RenderPhases::default()
    };
    if timing {
        log::trace!(
            target: "yu-render",
            "[rt] frame={} plan={:.2}ms ops={:.2}ms text={:.2}ms layers={} passes={} ops={}/{} surfaces={} bytes={}MB",
            frame.frame,
            total.as_secs_f64() * 1e3,
            t_ops.as_secs_f64() * 1e3,
            t_text.as_secs_f64() * 1e3,
            frame.layers.len(),
            acc.passes,
            acc.ops_drawn,
            acc.ops_total,
            stats.live_surfaces,
            stats.live_bytes / (1024 * 1024),
        );
    }
    Ok((
        FramePlan {
            passes,
            vertices,
            transition_bind_groups,
            transition_buffers,
            aux_bind_groups: aux,
            views,
            textures: textures_map,
        },
        acc.finish(missing_assets),
        phases,
    ))
}

// ------------------------------------------------------------------ 图像加工

/// 一次图像加工在常驻表面上的应用。
#[allow(clippy::too_many_arguments)]
fn apply_image(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    writes: &mut SurfaceWrites<'_>,
    chain_index: &mut BTreeMap<LayerId, usize>,
    pass_target: &LayerId,
    image: &ImageOp,
) -> Result<(), String> {
    let target = image.target.as_ref().unwrap_or(&image.source);
    if target != pass_target {
        return Err("gfx.imageop.target.mismatch".to_string());
    }
    let token = base_token(frame, targets, target, None);
    if surfaces.sync_base(target, token) {
        contents.remove(target);
        chain_index.insert(target.clone(), 0);
    }
    // 源层/第二源层的基面身份:与目标同一规则(读路径也校验,不只是写路径)。
    {
        let mut sync = |id: &LayerId, fallback: Option<&TextureRef>| {
            let token = base_token(frame, targets, id, fallback);
            if surfaces.sync_base(id, token) {
                contents.remove(id);
                chain_index.insert(id.clone(), 0);
            }
        };
        sync(&image.source, image.source_texture.as_ref());
        if let Some(source2) = &image.source2 {
            sync(source2, None);
        }
    }
    let source = resolve_source(
        ctx, frame, textures, gpu, targets, surfaces, contents, bases, &image.source,
        image.source_texture.as_ref(),
        if image.action.is_scale() {
            "gfx.copy.source.missing"
        } else {
            "gfx.copy.source.gone"
        },
    )?;
    let source2 = match &image.source2 {
        Some(id) => Some(resolve_source(
            ctx, frame, textures, gpu, targets, surfaces, contents, bases, id, None,
            "gfx.copy.source.gone",
        )?),
        None => None,
    };
    // 自引用(`ID` == 目标)时结果由**已应用前缀**决定,而前缀本身由
    // `applied` 逐项核对;签名里必须用稳定的基面身份,不能用本帧内容的
    // 修订号(否则第一帧「基面→物化」会让签名漂移,前缀永远命中不了)。
    let source_hash = if &image.source == target {
        token
    } else {
        source.hash
    };
    let source2_hash = match &image.source2 {
        Some(id) if id == target => token,
        _ => source2.as_ref().map_or(0, |content| content.hash),
    };
    let signature = image_signature(image, source_hash, source2_hash);
    let index = *chain_index.entry(target.clone()).or_insert(0);
    if surfaces.applied_at(target, index) == Some((image.seq, signature)) {
        *chain_index.get_mut(target).expect("inserted above") += 1;
        surfaces.note_reused_layer();
        return Ok(());
    }
    if surfaces.applied_len(target) > index {
        // 结构核对已在帧首做过;走到这里说明签名在中间漂移(源内容变化),
        // 表面不再是「当前前缀」的结果 ⇒ 回到基面整条重放。
        surfaces.truncate_applied(target, 0);
        surfaces.clear_content(target);
        contents.remove(target);
        *chain_index.get_mut(target).expect("inserted above") = 0;
    }
    apply_image_pixels(
        ctx, frame, textures, gpu, pipelines, targets, surfaces, contents, bases, writes, target,
        image, source, source2,
    )?;
    surfaces.push_applied(target, (image.seq, signature));
    *chain_index.get_mut(target).expect("inserted above") += 1;
    Ok(())
}

/// 目标层的基面(像素动作的“下层”):组/空纹理按 soft 同一原因拒绝。
#[allow(clippy::too_many_arguments)]
fn resolve_target_base(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    id: &LayerId,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
) -> Result<Content, String> {
    let Some(layer) = frame.layers.iter().find(|layer| &layer.id == id) else {
        return Err("gfx.imageop.target.unsupported".to_string());
    };
    if layer.is_group() {
        return Err("gfx.imageop.target.group".to_string());
    }
    match &layer.texture {
        TextureRef::None => Err("gfx.imageop.texture.none".to_string()),
        TextureRef::Snapshot(copy_id) if targets.snapshot(*copy_id).is_none() => {
            Err("gfx.imageop.copy_source.unsupported".to_string())
        }
        texture => {
            let tex = resolve_ref(ctx, textures, gpu, targets, id, texture, bases)?;
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            fnv_bytes(&mut hash, id.as_bytes());
            fnv_mix(&mut hash, tex.id);
            Ok(Content::identity(tex, hash))
        }
    }
}

/// 源层的当前像素(物化内容优先,否则基面)。
#[allow(clippy::too_many_arguments)]
fn resolve_source(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    surfaces: &LayerSurfaces,
    contents: &Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    id: &LayerId,
    fallback: Option<&TextureRef>,
    missing: &'static str,
) -> Result<Content, String> {
    let layer = frame.layers.iter().find(|layer| &layer.id == id);
    if layer.is_none() && fallback.is_some() {
        // 源层已被本帧 `CGEND` 删除(`DIV-GFX-053`):常驻内容属于**上一个**
        // 图层实例(`CG[FILE]` 每按钮重建模板后 COPY 再删),必须按命令时刻
        // 记录的纹理引用重放,否则所有快照都会拿到最后一次的像素。
    } else if let Some(content) = contents.get(id) {
        return Ok(content.clone());
    }
    let _ = surfaces;
    let texture = match layer {
        Some(layer) if !layer.is_group() => &layer.texture,
        Some(_) => return Err("gfx.copy.source.group_unsupported".to_string()),
        None => fallback.ok_or_else(|| missing.to_string())?,
    };
    let tex = resolve_ref(ctx, textures, gpu, targets, id, texture, bases)?;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_bytes(&mut hash, id.as_bytes());
    fnv_mix(&mut hash, tex.id);
    Ok(Content::identity(tex, hash))
}

pub(crate) fn resolve_ref(
    ctx: &GpuContext,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    id: &LayerId,
    texture: &TextureRef,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
) -> Result<Rc<TexRef>, String> {
    let key = (id.clone(), texref_token(texture));
    if let Some(hit) = bases.get(&key) {
        return Ok(hit.clone());
    }
    let tex: Rc<TexRef> = match texture {
        TextureRef::Asset(key) => {
            let gpu_id = gpu
                .ensure_asset(ctx, textures, key)
                .map_err(|diag| diag.code.to_string())?;
            let uploaded = gpu
                .get(gpu_id)
                .ok_or_else(|| "gpu.asset_missing".to_string())?;
            Rc::new(TexRef {
                id: uploaded.id,
                width: uploaded.width,
                height: uploaded.height,
                texture: uploaded.texture.clone(),
                view: uploaded.view.clone(),
            })
        }
        TextureRef::SolidBlack1x1 => {
            let gpu_id = gpu
                .ensure_solid_black(ctx)
                .map_err(|diag| diag.code.to_string())?;
            let uploaded = gpu
                .get(gpu_id)
                .ok_or_else(|| "gpu.solid_missing".to_string())?;
            Rc::new(TexRef {
                id: uploaded.id,
                width: uploaded.width,
                height: uploaded.height,
                texture: uploaded.texture.clone(),
                view: uploaded.view.clone(),
            })
        }
        TextureRef::Snapshot(copy_id) => targets
            .snapshot(*copy_id)
            .cloned()
            .ok_or_else(|| "gfx.copy.source.snapshot_missing".to_string())?,
        TextureRef::Offscreen(group) => targets
            .offscreen(group)
            .map(TexRef::from_target)
            .map(Rc::new)
            .ok_or_else(|| "gfx.copy.source.offscreen_missing".to_string())?,
        TextureRef::None => return Err("gfx.copy.source.texture_none".to_string()),
    };
    bases.insert(key, tex.clone());
    Ok(tex)
}

/// 纹理引用的身份(基面缓存键的一部分;同一层可在同一帧内被 `CG[FILE]`
/// 换成不同资产,只按图层 id 缓存会串图)。
fn texref_token(texture: &TextureRef) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    match texture {
        TextureRef::Asset(key) => {
            fnv_mix(&mut hash, 1);
            fnv_bytes(&mut hash, key);
        }
        TextureRef::SolidBlack1x1 => fnv_mix(&mut hash, 2),
        TextureRef::Snapshot(id) => {
            fnv_mix(&mut hash, 3);
            fnv_mix(&mut hash, id.0);
        }
        TextureRef::Offscreen(id) => {
            fnv_mix(&mut hash, 4);
            fnv_bytes(&mut hash, id.as_bytes());
        }
        TextureRef::None => fnv_mix(&mut hash, 5),
    }
    hash
}

/// 把 UV 采样修饰物化成真实像素(像素动作/复制/文本需要“整块像素”)。
fn materialize(
    ctx: &GpuContext,
    pipelines: &mut PipelineCache,
    surfaces: &mut LayerSurfaces,
    writes: &mut SurfaceWrites<'_>,
    content: &Content,
    what: &'static str,
) -> Result<Content, String> {
    if content.is_identity() {
        return Ok(content.clone());
    }
    let (x0, y0, x1, y1) = content_region(content);
    let target = surfaces.acquire(ctx, content.width, content.height, what)?;
    pipelines.ensure_resample_pipeline(ctx).map_err(|d| d.code.to_string())?;
    let bind = pipelines
        .create_resample_bind_group(
            ctx,
            &content.tex.view,
            (x0, y0, x1, y1),
            (content.width, content.height),
            content.bilinear,
        )
        .map_err(|diag| diag.code.to_string())?;
    writes.fullscreen(&target, bind, AuxPipeline::Resample);
    surfaces.stats_mut().materialized += 1;
    let hash = surfaces.bump_hash();
    Ok(Content {
        tex: target,
        uv: [0.0, 0.0, 1.0, 1.0],
        width: content.width,
        height: content.height,
        bilinear: false,
        display: content.display,
        hash,
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_image_pixels(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    writes: &mut SurfaceWrites<'_>,
    target: &LayerId,
    image: &ImageOp,
    source: Content,
    source2: Option<Content>,
) -> Result<(), String> {
    let uses_region = image.action.is_scale()
        || image.action.is_resize()
        || matches!(
            image.action,
            ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3
        );
    let region = if uses_region {
        match (image.x, image.y, image.x2, image.y2) {
            (Some(x), Some(y), Some(x2), Some(y2)) => Some((x, y, x2, y2)),
            (None, None, None, None) => None,
            _ => return Err("gfx.imageop.region.partial".to_string()),
        }
    } else {
        None
    };
    let int_region = region.map(|(x, y, x2, y2)| {
        (
            x.round() as i64,
            y.round() as i64,
            x2.round() as i64,
            y2.round() as i64,
        )
    });

    // ---------------- 缩放族(纯采样修饰,零像素) ----------------
    if image.action.is_scale() {
        let (Some(dw), Some(dh)) = (image.sx, image.sy) else {
            return Err("gfx.imageop.size.missing".to_string());
        };
        if dw <= 0.0 || dh <= 0.0 || dw < 1.0 || dh < 1.0 {
            return Ok(());
        }
        let uv = match region {
            Some((x0, y0, x1, y1)) => {
                let sw = source.width.max(1) as f64;
                let sh = source.height.max(1) as f64;
                let outer = [
                    (x0 / sw) as f32,
                    (y0 / sh) as f32,
                    ((x1 + 1.0) / sw) as f32,
                    ((y1 + 1.0) / sh) as f32,
                ];
                LayerSurfaces::compose_uv(source.uv, outer)
            }
            None => source.uv,
        };
        let hash = surfaces.bump_hash();
        let content = Content {
            tex: source.tex,
            uv,
            width: dw.round() as u32,
            height: dh.round() as u32,
            bilinear: image.action.is_bilinear_scale(),
            display: Some((dw, dh)),
            hash,
        };
        surfaces.set_sampled(target, content.clone());
        contents.insert(target.clone(), content);
        surfaces.stats_mut().surface_passes += 0;
        return Ok(());
    }

    // ---------------- 镜像(纯采样修饰) ----------------
    if matches!(image.action, ImageAction::Horizontal | ImageAction::Vertical) {
        let mut uv = source.uv;
        match image.action {
            ImageAction::Horizontal => uv.swap(0, 2),
            _ => uv.swap(1, 3),
        }
        let hash = surfaces.bump_hash();
        let content = Content {
            tex: source.tex,
            uv,
            width: source.width,
            height: source.height,
            bilinear: false,
            display: source.display,
            hash,
        };
        surfaces.set_sampled(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- 双源过渡(lerp) ----------------
    if matches!(&image.action, ImageAction::Other("transition")) {
        let Some(to) = source2 else {
            return Err("gfx.imageop.source.missing".to_string());
        };
        let blend = image.set.as_ref().and_then(|value| value.as_int()).unwrap_or(0);
        let f = blend.clamp(0, 256) as u32;
        if to.width == 0 || to.height == 0 {
            return Ok(());
        }
        let out = surfaces.acquire(ctx, to.width, to.height, "transition surface")?;
        pipelines.ensure_lerp_pipeline(ctx).map_err(|d| d.code.to_string())?;
        let bind = pipelines
            .create_lerp_bind_group(ctx, &source.tex.view, &to.tex.view, f)
            .map_err(|diag| diag.code.to_string())?;
        writes.fullscreen(&out, bind, AuxPipeline::Lerp);
        surfaces.stats_mut().surface_passes += 1;
        let content = finish_content(surfaces, out, to.width, to.height, None, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- 两图层合成(MERGE/MERGE2/XOR/AND) ----------------
    if image.action.is_layer_composite() {
        let dx = image.x.unwrap_or(0.0);
        let dy = image.y.unwrap_or(0.0);
        if dx != 0.0 || dy != 0.0 {
            return Err("gfx.imageop.merge.offset.unsupported".to_string());
        }
        // 下层 = 目标层**当前**表面。官方合并执行体(`FUN_004405a4`,
        // REA 0.495 `0x4405a4`)只把上层的每部件纹理按下层的每部件位置
        // 画到当前渲染目标,不读下层旧像素;清空是否可见由 alpha 的混合
        // 口径决定(见 `merge.wgsl` 头注:同因子混合会写 alpha),不需要
        // 任何“冻结像素”特例。
        let lower = match contents.get(target).cloned() {
            Some(content) => content,
            None => resolve_target_base(ctx, frame, textures, gpu, targets, target, bases)?,
        };
        let lower = materialize(ctx, pipelines, surfaces, writes, &lower, "merge lower")?;
        let upper = materialize(ctx, pipelines, surfaces, writes, &source, "merge upper")?;
        let (width, height) = (lower.width, lower.height);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let out = surfaces.acquire(ctx, width, height, "merge surface")?;
        // `MERGE` 用已验证逐位一致的专用 shader(`merge.wgsl`);`MERGE2`/
        // `XOR`/`AND` 走 `composite.wgsl`(mode 0/1/2)。
        let (bind, pipeline) = if image.action == ImageAction::Merge {
            pipelines
                .ensure_layer_merge_pipeline(ctx)
                .map_err(|d| d.code.to_string())?;
            (
                pipelines
                    .create_layer_merge_bind_group(ctx, &lower.tex.view, &upper.tex.view)
                    .map_err(|diag| diag.code.to_string())?,
                AuxPipeline::LayerMerge,
            )
        } else {
            let mode = match image.action {
                ImageAction::Merge2 => 0,
                ImageAction::Other("xor") => 1,
                _ => 2,
            };
            pipelines
                .ensure_composite_pipeline(ctx)
                .map_err(|d| d.code.to_string())?;
            (
                pipelines
                    .create_composite_bind_group(ctx, &lower.tex.view, &upper.tex.view, mode)
                    .map_err(|diag| diag.code.to_string())?,
                AuxPipeline::Composite,
            )
        };
        writes.fullscreen(&out, bind, pipeline);
        surfaces.stats_mut().surface_passes += 1;
        let content = finish_content(surfaces, out, width, height, lower.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- RECTPAINT 族 ----------------
    if matches!(
        image.action,
        ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3
    ) {
        let Some(set) = image.set.as_ref().and_then(|value| value.as_int()) else {
            return Err("gfx.imageop.set.missing".to_string());
        };
        let current = match contents.get(target).cloned() {
            Some(content) => content,
            None => resolve_target_base(ctx, frame, textures, gpu, targets, target, bases)?,
        };
        let alpha_only = matches!(image.action, ImageAction::RectPaint2);
        let replaces = int_region.is_none() && !alpha_only;
        let base = materialize(ctx, pipelines, surfaces, writes, &current, "paint base")?;
        let (width, height) = (base.width, base.height);
        let out = surfaces.acquire(ctx, width, height, "paint surface")?;
        writes.remember(&out);
        if !replaces {
            let mut pass = PlannedPass {
                target: Some(PlannedTarget::Commands),
                ..PlannedPass::default()
            };
            pass.clear_targets.push(out.id);
            writes.passes.push(pass);
            push_blit(
                writes,
                "paint",
                &base.tex,
                &out,
                (0, 0),
                (0, 0),
                width,
                height,
            )?;
        }
        let (x0, y0, x1, y1) = match int_region {
            Some(region) => region,
            None => (0, 0, i64::from(width) - 1, i64::from(height) - 1),
        };
        let x0 = x0.max(0) as u32;
        let y0 = y0.max(0) as u32;
        let x1 = x1.min(i64::from(width) - 1);
        let y1 = y1.min(i64::from(height) - 1);
        if x1 >= i64::from(x0) && y1 >= i64::from(y0) {
            let (x1, y1) = (x1 as u32, y1 as u32);
            let set = set as u32;
            let color: [f32; 4] = match image.action {
                ImageAction::RectPaint => [
                    ((set >> 16) & 0xFF) as f32 / 255.0,
                    ((set >> 8) & 0xFF) as f32 / 255.0,
                    (set & 0xFF) as f32 / 255.0,
                    1.0,
                ],
                ImageAction::RectPaint2 => [0.0, 0.0, 0.0, (set & 0xFF) as f32 / 255.0],
                _ => [
                    ((set >> 16) & 0xFF) as f32 / 255.0,
                    ((set >> 8) & 0xFF) as f32 / 255.0,
                    (set & 0xFF) as f32 / 255.0,
                    ((set >> 24) & 0xFF) as f32 / 255.0,
                ],
            };
            pipelines
                .ensure_paint_pipeline(ctx, alpha_only)
                .map_err(|d| d.code.to_string())?;
            pipelines
                .ensure_paint_bind_group(ctx, alpha_only, color)
                .map_err(|diag| diag.code.to_string())?;
            let first_vertex = writes.vertices.len() as u32;
            push_quad(
                writes.vertices,
                width,
                height,
                f64::from(x0),
                f64::from(y0),
                x1 - x0 + 1,
                y1 - y0 + 1,
                1.0,
            );
            let mut pass = PlannedPass {
                target: Some(PlannedTarget::Commands),
                first_vertex,
                ..PlannedPass::default()
            };
            pass.paints.push(PaintCmd {
                target_id: out.id,
                alpha_only,
                color: color.map(f32::to_bits),
                first_vertex,
            });
            writes.passes.push(pass);
            surfaces.stats_mut().surface_passes += 1;
        } else {
            // 区域为空:保留拷贝结果(与 CPU 循环不写像素一致)。
            surfaces.stats_mut().surface_passes += 0;
        }
        let content = finish_content(surfaces, out, width, height, base.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- 其余:需要整块像素的原地动作 ----------------
    let current = match contents.get(target).cloned() {
        Some(content) => content,
        None => resolve_target_base(ctx, frame, textures, gpu, targets, target, bases)?,
    };
    let base = materialize(ctx, pipelines, surfaces, writes, &current, "image base")?;
    let mode = match &image.action {
        ImageAction::Reverse => Some(0i32),
        ImageAction::Vertical => Some(2),
        ImageAction::Other("meido") => Some(3),
        ImageAction::Other("saido") => Some(4),
        ImageAction::Other("akarusa") => Some(5),
        ImageAction::Other("mono") => Some(6),
        ImageAction::Other("sepia") => Some(7),
        ImageAction::Other("mosaic") => Some(8),
        ImageAction::Rotation | ImageAction::Other("rotation2") => Some(9),
        _ => None,
    };
    if let Some(mode) = mode {
        let (width, height) = (base.width, base.height);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let set = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0) as i32;
        let set2 = image.set2.as_ref().and_then(|v| v.as_int()).unwrap_or(0) as i32;
        let out = surfaces.acquire(ctx, width, height, "filter surface")?;
        pipelines.ensure_transform_pipeline(ctx).map_err(|d| format!("{}: {}", d.code, d.detail))?;
        let m = if mode == 9 {
            let deg = f64::from(set).rem_euclid(360.0);
            let (sin, cos) = deg.to_radians().sin_cos();
            let cx = (f64::from(width) - 1.0) / 2.0;
            let cy = (f64::from(height) - 1.0) / 2.0;
            [sin as f32, cos as f32, cx as f32, cy as f32]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        };
        let bind = pipelines
            .create_transform_bind_group(ctx, &base.tex.view, [mode, set, set2, 0], m)
            .map_err(|diag| format!("{}: {}", diag.code, diag.detail))?;
        writes.fullscreen(&out, bind, AuxPipeline::Transform);
        surfaces.stats_mut().surface_passes += 1;
        let content = finish_content(surfaces, out, width, height, base.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }
    if image.action.is_resize() {
        let (x, y, x2, y2) = (
            image.x.unwrap_or(0.0).round() as i64,
            image.y.unwrap_or(0.0).round() as i64,
            image.x2.unwrap_or(0.0).round() as i64,
            image.y2.unwrap_or(0.0).round() as i64,
        );
        let new_w = i64::from(base.width) + x + x2;
        let new_h = i64::from(base.height) + y + y2;
        if new_w < 1 || new_h < 1 {
            return Err("gfx.imageop.resize.invalid".to_string());
        }
        if new_w > 0x2000 {
            return Err("gfx.imageop.resize.too_large".to_string());
        }
        let (new_w, new_h) = (new_w as u32, new_h as u32);
        let out = surfaces.acquire(ctx, new_w, new_h, "resize surface")?;
        writes.remember(&out);
        let mut pass = PlannedPass {
            target: Some(PlannedTarget::Commands),
            ..PlannedPass::default()
        };
        pass.clear_targets.push(out.id);
        writes.passes.push(pass);
        let (sx0, sy0) = ((-x).max(0), (-y).max(0));
        let (dx0, dy0) = (x.max(0), y.max(0));
        let copy_w = (i64::from(base.width) - sx0).min(i64::from(new_w) - dx0);
        let copy_h = (i64::from(base.height) - sy0).min(i64::from(new_h) - dy0);
        if copy_w > 0 && copy_h > 0 {
            push_blit(
                writes,
                "resize",
                &base.tex,
                &out,
                (sx0 as u32, sy0 as u32),
                (dx0 as u32, dy0 as u32),
                copy_w as u32,
                copy_h as u32,
            )?;
        }
        surfaces.stats_mut().blits += 1;
        let content = finish_content(surfaces, out, new_w, new_h, base.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }
    if image.action.is_trim() {
        let Some(set) = image.set.as_ref().and_then(|value| value.as_int()) else {
            return Err("gfx.imageop.set.missing".to_string());
        };
        let (width, height) = (base.width, base.height);
        let surface = crate::readback::read_texture(ctx, &base.tex.texture, width, height)
            .map_err(|diag| diag.code.to_string())?;
        surfaces.stats_mut().cpu_fallbacks += 1;
        let Some((x0, y0, x1, y1)) = trim_box(&surface, width, height, set) else {
            return Ok(());
        };
        let (new_w, new_h) = ((x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
        let out = surfaces.acquire(ctx, new_w, new_h, "trim surface")?;
        writes.remember(&out);
        let mut pass = PlannedPass {
            target: Some(PlannedTarget::Commands),
            ..PlannedPass::default()
        };
        pass.clear_targets.push(out.id);
        writes.passes.push(pass);
        push_blit(
            writes,
            "trim",
            &base.tex,
            &out,
            (x0 as u32, y0 as u32),
            (0, 0),
            new_w,
            new_h,
        )?;
        surfaces.stats_mut().blits += 1;
        let content = finish_content(surfaces, out, new_w, new_h, None, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }
    Err("gfx.imageop.unsupported".to_string())
}

/// 记录一次纹理区域复制;越界时显式报错(带路径标签与尺寸),不发
/// wgpu 验证错误(那会让整条命令缓冲失效)。
#[allow(clippy::too_many_arguments)]
fn push_blit(
    writes: &mut SurfaceWrites<'_>,
    label: &'static str,
    src: &Rc<TexRef>,
    dst: &Rc<TexRef>,
    src_origin: (u32, u32),
    dst_origin: (u32, u32),
    width: u32,
    height: u32,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    if src_origin.0 + width > src.width
        || src_origin.1 + height > src.height
        || dst_origin.0 + width > dst.width
        || dst_origin.1 + height > dst.height
    {
        return Err(format!(
            "gpu.blit.oob {label}: src {}x{} origin {:?} extent {}x{} -> dst {}x{} origin {:?}",
            src.width, src.height, src_origin, width, height, dst.width, dst.height, dst_origin
        ));
    }
    writes.remember(src);
    writes.remember(dst);
    let mut pass = PlannedPass {
        target: Some(PlannedTarget::Commands),
        ..PlannedPass::default()
    };
    pass.blits.push(BlitCmd {
        label,
        source_id: src.id,
        dest_id: dst.id,
        src_origin,
        dst_origin,
        width,
        height,
    });
    writes.passes.push(pass);
    Ok(())
}

fn finish_content(
    surfaces: &mut LayerSurfaces,
    tex: Rc<TexRef>,
    width: u32,
    height: u32,
    display: Option<(f64, f64)>,
    bilinear: Option<bool>,
) -> Content {
    let hash = surfaces.bump_hash();
    Content {
        tex,
        uv: [0.0, 0.0, 1.0, 1.0],
        width,
        height,
        bilinear: bilinear.unwrap_or(false),
        display,
        hash,
    }
}

// ------------------------------------------------------------------ 快照

#[allow(clippy::too_many_arguments)]
fn apply_copy(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    writes: &mut SurfaceWrites<'_>,
    backbuffer: Option<&Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    text: &mut TextComposer,
    acc: &mut Accum,
    pass_target: &LayerId,
    copy: &CopyOp,
) -> Result<(), String> {
    if &copy.dest != pass_target {
        return Err("gfx.copy.target.mismatch".to_string());
    }
    if targets.snapshot(copy.id).is_some() {
        // 已冻结(EVID-T-12)。
        return Ok(());
    }
    if let CopySource::Layer(id) = &copy.source {
        let token = base_token(frame, targets, id, copy.source_texture.as_ref());
        if surfaces.sync_base(id, token) {
            contents.remove(id);
        }
    }
    let mut source = match &copy.source {
        CopySource::Layer(id) => {
            let content = resolve_source(
                ctx, frame, textures, gpu, targets, surfaces, contents, bases, id,
                copy.source_texture.as_ref(), "gfx.copy.source.gone",
            )?;
            if copy.texts.is_empty() {
                content
            } else {
                // 冻结文本:命令时刻已画的字形进快照(DIV-GFX-058)。
                let raw = surfaces.raw_content(id).unwrap_or_else(|| content.clone());
                let (cpu, _info) = content_cpu(ctx, frame, textures, contents, id, &raw)?;
                let (composed, stats) = text.compose_layer(&cpu, &copy.texts, None);
                merge_text_stats(acc, stats);
                match composed {
                    Some(composed) => {
                        let id_upload = gpu
                            .upload(
                                ctx,
                                composed.width,
                                composed.height,
                                &composed.rgba,
                                "copy text source",
                            )
                            .map_err(|diag| diag.code.to_string())?;
                        let uploaded = gpu
                            .get(id_upload)
                            .ok_or_else(|| "gpu.upload_missing".to_string())?;
                        Content {
                            tex: Rc::new(TexRef {
                                id: id_upload,
                                width: uploaded.width,
                                height: uploaded.height,
                                texture: uploaded.texture.clone(),
                                view: uploaded.view.clone(),
                            }),
                            uv: [0.0, 0.0, 1.0, 1.0],
                            width: composed.width,
                            height: composed.height,
                            bilinear: false,
                            display: None,
                            hash: 0,
                        }
                    }
                    None => raw,
                }
            }
        }
        CopySource::Group(group) => {
            let main_group = frame
                .windows
                .iter()
                .find(|w| w.number == 0)
                .and_then(|w| w.layer_set.as_ref());
            if Some(group) == main_group {
                backbuffer_content(
                    ctx, frame, gpu, backbuffer, backbuffer_gpu, writes,
                )?
            } else {
                let tex = compose_group_content(
                    ctx, frame, textures, gpu, pipelines, targets, surfaces, contents, bases,
                    writes, group,
                )?;
                Content::identity(tex, 0)
            }
        }
    };
    let (sw, sh) = (source.width, source.height);
    let (x0, y0, x1, y1) = copy.region.resolve(sw, sh);
    let mut width = (x1 - x0 + 1).max(0) as u32;
    let mut height = (y1 - y0 + 1).max(0) as u32;
    if width == 0 || height == 0 {
        width = sw;
        height = sh;
    }
    let full = x0 == 0
        && y0 == 0
        && x1 == i64::from(sw) - 1
        && y1 == i64::from(sh) - 1;
    // 显式区域可能超出源表面(脚本给 `X2=263` 而源只有 200 宽)。soft 的
    // `blit` 按源边界裁剪、落点仍在 (0,0);这里同样裁剪 —— 越界范围交给
    // `copy_texture_to_texture` 会让整条命令缓冲失效。
    let clip_x0 = x0.max(0);
    let clip_y0 = y0.max(0);
    let clip_x1 = x1.min(i64::from(sw) - 1);
    let clip_y1 = y1.min(i64::from(sh) - 1);
    let clip_w = (clip_x1 - clip_x0 + 1).max(0) as u32;
    let clip_h = (clip_y1 - clip_y0 + 1).max(0) as u32;
    let tex = if full && source.is_identity() && width == sw && height == sh {
        // 别名:快照 = 源内容句柄(零拷贝)。内容纹理永不原地改写
        // (surfaces 不变量 1)⇒ 冻结语义天然成立。
        surfaces.note_alias_hit();
        source.tex.clone()
    } else {
        let materialized = materialize(ctx, pipelines, surfaces, writes, &source, "copy source")?;
        let out = surfaces.acquire(ctx, width, height, "copy snapshot")?;
        writes.remember(&out);
        let mut pass = PlannedPass {
            target: Some(PlannedTarget::Commands),
            ..PlannedPass::default()
        };
        pass.clear_targets.push(out.id);
        writes.passes.push(pass);
        let (sx, sy) = if full { (0, 0) } else { (clip_x0 as u32, clip_y0 as u32) };
        push_blit(
            writes,
            "copy",
            &materialized.tex,
            &out,
            (sx, sy),
            (0, 0),
            if full { width } else { clip_w },
            if full { height } else { clip_h },
        )?;
        surfaces.stats_mut().blits += 1;
        out
    };
    targets.set_snapshot(copy.id, tex.clone());
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, copy.id.0);
    fnv_mix(&mut hash, tex.id);
    let content = Content::identity(tex, hash);
    contents.insert(copy.dest.clone(), content.clone());
    surfaces.set_content(&copy.dest, content);
    let _ = &mut source;
    Ok(())
}

/// 上一帧呈现面(`COPY ID=:/W0/`;EVID-J3-06)。内容未变时复用 GPU 纹理。
#[allow(clippy::too_many_arguments)]
fn backbuffer_content(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    gpu: &mut TextureCache,
    backbuffer: Option<&Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    writes: &mut SurfaceWrites<'_>,
) -> Result<Content, String> {
    let Some(buffer) = backbuffer else {
        // 首帧没有后缓冲(DIV-GFX-051):D3D9 设备创建后后缓冲即存在,
        // 内容是初始化清屏色(DIV-GFX-010)。
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        let rgba = frame
            .clear
            .rgba
            .iter()
            .copied()
            .cycle()
            .take(width as usize * height as usize * 4)
            .collect::<Vec<u8>>();
        let id = gpu
            .upload(ctx, width, height, &rgba, "backbuffer initial clear")
            .map_err(|diag| diag.code.to_string())?;
        let uploaded = gpu
            .get(id)
            .ok_or_else(|| "gpu.upload_missing".to_string())?;
        let tex = Rc::new(TexRef {
            id,
            width: uploaded.width,
            height: uploaded.height,
            texture: uploaded.texture.clone(),
            view: uploaded.view.clone(),
        });
        writes.remember(&tex);
        return Ok(Content::identity(tex, 0));
    };
    let hash = hash_rgba(buffer.width, buffer.height, &buffer.rgba);
    if let Some((cached, id)) = *backbuffer_gpu {
        if cached == hash {
            if let Some(uploaded) = gpu.get(id) {
                let tex = Rc::new(TexRef {
                    id,
                    width: uploaded.width,
                    height: uploaded.height,
                    texture: uploaded.texture.clone(),
                    view: uploaded.view.clone(),
                });
                writes.remember(&tex);
                return Ok(Content::identity(tex, hash));
            }
        }
    }
    let id = gpu
        .upload(
            ctx,
            buffer.width,
            buffer.height,
            &buffer.rgba,
            "backbuffer",
        )
        .map_err(|diag| diag.code.to_string())?;
    *backbuffer_gpu = Some((hash, id));
    let uploaded = gpu
        .get(id)
        .ok_or_else(|| "gpu.upload_missing".to_string())?;
    let tex = Rc::new(TexRef {
        id,
        width: uploaded.width,
        height: uploaded.height,
        texture: uploaded.texture.clone(),
        view: uploaded.view.clone(),
    });
    writes.remember(&tex);
    Ok(Content::identity(tex, hash))
}

/// 组源的“当前内容”:按组 pass 现绘到一个离屏目标(EVID-T-12)。
#[allow(clippy::too_many_arguments)]
fn compose_group_content(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    writes: &mut SurfaceWrites<'_>,
    group: &LayerId,
) -> Result<Rc<TexRef>, String> {
    let pass = frame
        .passes
        .iter()
        .find(|pass| matches!(&pass.target, PassTarget::Offscreen(target) if target == group))
        .ok_or_else(|| "gfx.copy.source.offscreen_missing".to_string())?;
    let (width, height) = group_surface_size(frame, group);
    if width == 0 || height == 0 {
        return Err("group.size.zero".to_string());
    }
    let clear = pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]));
    let target = targets
        .ensure_offscreen(ctx, group, width, height, pipelines)
        .map_err(|diag| diag.code.to_string())?;
    let target_id = target.id;
    writes.views.insert(target_id, target.view.clone());
    let mut planned = PlannedPass {
        target: Some(PlannedTarget::Offscreen(group.clone())),
        clear: clear_color(clear),
        first_vertex: writes.vertices.len() as u32,
        ..PlannedPass::default()
    };
    {
        // 组内 op 的计数**回滚**:这次现绘不是 CompositorFrame 的独立 pass
        // (soft `compose_group` 同口径)。
        let mut local = Accum::new(frame);
        let mut planner = Planner {
            ctx,
            frame,
            textures,
            gpu,
            pipelines,
            targets,
            vertices: writes.vertices,
            acc: &mut local,
            contents,
            bases,
            views: writes.views,
            transition_bind_groups: &mut Vec::new(),
            transition_buffers: &mut Vec::new(),
            dst_width: width,
            dst_height: height,
            counted: false,
            trace_geometry: false,
        };
        for op in &pass.ops {
            planner
                .plan_op(op, &mut planned)
                .map_err(|diag| diag.code.to_string())?;
        }
    }
    writes.passes.push(planned);
    let _ = surfaces;
    Ok(Rc::new(TexRef::from_target(
        targets.offscreen(group).expect("offscreen just ensured"),
    )))
}

// ------------------------------------------------------------------ 文本

/// 文本合成的上传缓存(内容哈希命中时不重传)。
#[derive(Default)]
pub(crate) struct TextUploads {
    layers: BTreeMap<LayerId, (u64, Rc<TexRef>)>,
    screen: Option<(u64, Rc<TexRef>)>,
}

impl TextUploads {
    pub fn clear(&mut self) {
        self.layers.clear();
        self.screen = None;
    }

    pub fn retain(&mut self, live: &BTreeSet<LayerId>) {
        self.layers.retain(|id, _| live.contains(id));
    }
}

/// 文本绘制的身份(内容 + 排版 + 修订号):不变则不重烤、不重传。
fn text_signature(draws: &[&yu_gfx::TextDraw], layout: Option<&yu_gfx::TextLayoutFrame>, dims: (u32, u32)) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, u64::from(dims.0));
    fnv_mix(&mut hash, u64::from(dims.1));
    for draw in draws {
        fnv_bytes(&mut hash, &draw.bytes);
        fnv_mix(&mut hash, draw.x as u64);
        fnv_mix(&mut hash, draw.y as u64);
        fnv_mix(&mut hash, draw.set as u64);
        fnv_mix(&mut hash, draw.bno.map_or(u64::MAX, |v| v as u64));
        fnv_mix(&mut hash, u64::from(draw.style.color));
        fnv_mix(&mut hash, draw.style.size_x as u64);
        fnv_mix(&mut hash, draw.style.size_y as u64);
        fnv_mix(&mut hash, u64::from(draw.style.bold));
        fnv_mix(&mut hash, u64::from(draw.style.antialias));
        fnv_mix(&mut hash, draw.style.draw_mode as u64);
        fnv_mix(&mut hash, draw.style.gradation as u64);
        if let Some(name) = &draw.style.name {
            fnv_bytes(&mut hash, name);
        }
        fnv_mix(&mut hash, draw.style.font_no.map_or(u64::MAX, |v| v as u64));
        fnv_mix(&mut hash, draw.style.font_bno.map_or(u64::MAX, |v| v as u64));
    }
    if let Some(layout) = layout {
        fnv_mix(&mut hash, layout.glyphs.len() as u64);
        fnv_mix(&mut hash, layout.reveal.map_or(u64::MAX, |value| value as u64));
        fnv_mix(&mut hash, layout.origin.0 as u64);
        fnv_mix(&mut hash, layout.origin.1 as u64);
        for glyph in &layout.glyphs {
            fnv_bytes(&mut hash, &glyph.bytes);
            fnv_mix(&mut hash, glyph.x as u64);
            fnv_mix(&mut hash, glyph.y as u64);
            fnv_mix(&mut hash, u64::from(glyph.full));
        }
        fnv_mix(&mut hash, layout.style.size_x as u64);
        fnv_mix(&mut hash, layout.style.size_y as u64);
        fnv_mix(&mut hash, u64::from(layout.style.color));
        fnv_mix(&mut hash, layout.style.draw_mode as u64);
    }
    hash
}

/// 层内容是否仍由它的资产纹理构成(本帧没有像素 op 改写)。
///
/// 判据是**底层纹理身份**:`raw.tex.id == asset.tex.id` ⇒ 当前像素就是
/// 资产本身(可能带 `SIZE` 族采样修饰)。这时直接复用资产 CPU 纹理:
/// 恒等窗原样返回;采样窗按 `content_region` 做一次 `scale_region`(与
/// `content_cpu` 读回后的缩放同式),两者都不需要同步 GPU 读回。
fn asset_identity_base(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    id: &LayerId,
    raw: &Content,
) -> Option<Rc<Texture>> {
    let layer = frame.layers.iter().find(|layer| &layer.id == id)?;
    let TextureRef::Asset(key) = &layer.texture else {
        return None;
    };
    let tex = resolve_ref(ctx, textures, gpu, targets, id, &layer.texture, bases).ok()?;
    if raw.tex.id != tex.id {
        // 像素 op 产出了新纹理:内容已不是资产,必须读回。
        return None;
    }
    let asset = textures.resolve_shared(key).ok()?;
    if raw.is_identity() {
        return Some(asset);
    }
    let region = content_region(raw);
    Some(Rc::new(yu_render_core::scale_region(
        &asset,
        raw.width,
        raw.height,
        Some(region),
        raw.bilinear,
    )))
}

/// CPU 基面取得的来源与耗时(诊断:资产克隆 vs 同步 GPU 读回)。
#[derive(Default, Clone, Copy)]
struct CpuBaseInfo {
    readback_us: u64,
    readbacks: u64,
    asset_us: u64,
    asset_bytes: u64,
}

/// 文本合成的 CPU 基面像素(纯基图且可 CPU 解析时零读回)。
fn content_cpu(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    contents: &Contents,
    id: &LayerId,
    content: &Content,
) -> Result<(Texture, CpuBaseInfo), String> {
    let mut info = CpuBaseInfo::default();
    let mut texture = if !contents.contains_key(id) {
        let layer = frame.layers.iter().find(|layer| &layer.id == id);
        match layer.map(|layer| &layer.texture) {
            Some(TextureRef::Asset(key)) => {
                let t = std::time::Instant::now();
                let texture = textures
                    .resolve(key)
                    .cloned()
                    .map_err(|diag| diag.code.to_string())?;
                info.asset_us = u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX);
                info.asset_bytes = texture.rgba.len() as u64;
                texture
            }
            Some(TextureRef::SolidBlack1x1) => Texture::solid_black_1x1(),
            _ => {
                let t = std::time::Instant::now();
                let texture = read_back(ctx, content)?;
                info.readback_us = u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX);
                info.readbacks = 1;
                texture
            }
        }
    } else {
        let t = std::time::Instant::now();
        let texture = read_back(ctx, content)?;
        info.readback_us = u64::try_from(t.elapsed().as_micros()).unwrap_or(u64::MAX);
        info.readbacks = 1;
        texture
    };
    if !content.is_identity() {
        let (x0, y0, x1, y1) = content_region(content);
        texture = yu_render_core::scale_region(
            &texture,
            content.width,
            content.height,
            Some((x0, y0, x1, y1)),
            content.bilinear,
        );
    } else if texture.width != content.width || texture.height != content.height {
        texture =
            yu_render_core::scale_region(&texture, content.width, content.height, None, false);
    }
    Ok((texture, info))
}

fn read_back(ctx: &GpuContext, content: &Content) -> Result<Texture, String> {
    let surface = crate::readback::read_texture(
        ctx,
        &content.tex.texture,
        content.tex.width,
        content.tex.height,
    )
    .map_err(|diag| diag.code.to_string())?;
    Ok(Texture {
        width: surface.width,
        height: surface.height,
        rgba: surface.rgba,
    })
}

/// 文本阶段:把 `CGACT[TEXT]`/`TextRun` 合成进图层常驻表面(W4)。
#[allow(clippy::too_many_arguments)]
fn apply_text(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    text_uploads: &mut TextUploads,
    text: &mut TextComposer,
    acc: &mut Accum,
    targets: &TargetSet,
) -> (u64, CpuBaseInfo) {
    let mut cpu_base_us = 0u64;
    let mut cpu_info = CpuBaseInfo::default();
    let mut ids: BTreeSet<LayerId> = frame.text_draws.iter().map(|d| d.target.clone()).collect();
    if let Some(layout) = &frame.text_layout {
        if let Some(target) = &layout.target {
            ids.insert(target.clone());
        }
    }
    for id in ids {
        let Some(layer) = frame.layers.iter().find(|layer| layer.id == id) else {
            note_text_diag(acc, format!("text.layer_missing: {}", id.display()));
            continue;
        };
        if !layer.visible {
            continue;
        }
        let draws: Vec<&yu_gfx::TextDraw> = frame
            .text_draws
            .iter()
            .filter(|d| d.target == id)
            .collect();
        let layout = frame
            .text_layout
            .as_ref()
            .filter(|l| l.target.as_ref() == Some(&id));
        let revision = frame.text_revisions.get(&id).copied().unwrap_or(0);
        let raw = match surfaces
            .raw_content(&id)
            .or_else(|| contents.get(&id).cloned())
        {
            Some(content) => content,
            None => match resolve_source(
                ctx, frame, textures, gpu, targets, surfaces, contents, bases, &id, None,
                "gfx.copy.source.gone",
            ) {
                Ok(content) => content,
                Err(reason) => {
                    note_text_diag(acc, format!("text.layer_base: {reason}"));
                    continue;
                }
            },
        };
        if raw.width == 0 || raw.height == 0 {
            note_text_diag(acc, "text.layer_base: text.layer_size_unknown".to_string());
            continue;
        }
        let dims = (raw.width, raw.height);
        let signature = text_signature(&draws, layout, dims);
        let (cached_rev, cached_sig, cached_raw) = surfaces.text_state(&id);
        if cached_sig == signature && cached_rev == revision && cached_raw == raw.hash {
            // 命中:常驻内容(基面 + 文本)直接回到绘制视图。
            if let Some(content) = surfaces.content(&id) {
                contents.insert(id, content.clone());
            }
            continue;
        }
        let cpu_started = std::time::Instant::now();
        // 缓存键用“基面像素来源”身份(基面 token + 操作前缀),不用混了
        // 纹理池 id 的 `raw.hash`:同一基面换池后仍应命中。
        let base_key = surfaces.raw_identity(&id).unwrap_or(raw.hash);
        let cpu_base: Result<Rc<Texture>, String> =
            if let Some(hit) = text.cached_base(&id, base_key) {
                Ok(hit)
            } else if let Some(asset) = asset_identity_base(
                ctx, frame, textures, gpu, targets, bases, &id, &raw,
            ) {
                text.store_base(&id, base_key, Rc::clone(&asset));
                Ok(asset)
            } else {
                match content_cpu(ctx, frame, textures, contents, &id, &raw) {
                    Ok((texture, info)) => {
                        if info.readbacks > 0 {
                            log::debug!(
                                target: "yu-render",
                                "[text-base] layer={} raw={}x{} base_key={:#x} raw_hash={:#x} readback={}us",
                                id.display(),
                                raw.width,
                                raw.height,
                                base_key,
                                raw.hash,
                                info.readback_us
                            );
                        }
                        cpu_info.readback_us = cpu_info.readback_us.saturating_add(info.readback_us);
                        cpu_info.readbacks = cpu_info.readbacks.saturating_add(info.readbacks);
                        cpu_info.asset_us = cpu_info.asset_us.saturating_add(info.asset_us);
                        cpu_info.asset_bytes = cpu_info.asset_bytes.saturating_add(info.asset_bytes);
                        let texture = Rc::new(texture);
                        text.store_base(&id, base_key, Rc::clone(&texture));
                        Ok(texture)
                    }
                    Err(reason) => Err(reason),
                }
            };
        cpu_base_us = cpu_base_us.saturating_add(
            u64::try_from(cpu_started.elapsed().as_micros()).unwrap_or(u64::MAX),
        );
        let composed = text.compose_layer_cached(
            &id,
            revision,
            raw.hash,
            dims,
            &draws,
            layout,
            || cpu_base,
        );
        let (composed, stats) = match composed {
            Ok(value) => value,
            Err(reason) => {
                note_text_diag(acc, format!("text.layer_base: {reason}"));
                continue;
            }
        };
        merge_text_stats(acc, stats);
        let Some(composed) = composed else {
            surfaces.note_text_baked(&id, revision, signature, raw.hash);
            continue;
        };
        let tex = match text_uploads.layers.get(&id) {
            Some((hash, tex))
                if *hash == composed.hash
                    && tex.width == composed.texture.width
                    && tex.height == composed.texture.height =>
            {
                tex.clone()
            }
            _ => {
                let id_upload = match gpu.upload(
                    ctx,
                    composed.texture.width,
                    composed.texture.height,
                    &composed.texture.rgba,
                    "text layer",
                ) {
                    Ok(id) => id,
                    Err(diag) => {
                        note_text_diag(
                            acc,
                            format!("text.layer_base: {} {}", diag.code, diag.detail),
                        );
                        continue;
                    }
                };
                let uploaded = gpu
                    .get(id_upload)
                    .expect("uploaded text texture exists");
                let tex = Rc::new(TexRef {
                    id: id_upload,
                    width: uploaded.width,
                    height: uploaded.height,
                    texture: uploaded.texture.clone(),
                    view: uploaded.view.clone(),
                });
                text_uploads
                    .layers
                    .insert(id.clone(), (composed.hash, tex.clone()));
                tex
            }
        };
        let content = finish_content(surfaces, tex, dims.0, dims.1, raw.display, None);
        surfaces.set_text_content(&id, content.clone(), raw.clone());
        surfaces.note_text_baked(&id, revision, signature, raw.hash);
        contents.insert(id, content);
    }
    (cpu_base_us, cpu_info)
}

/// 屏幕级自动文本(`target=None`):全视口透明覆盖 + 一次合成 quad。
#[allow(clippy::too_many_arguments)]
fn plan_screen_overlay_text(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    vertices: &mut Vec<Vertex>,
    passes: &mut Vec<PlannedPass>,
    views: &mut BTreeMap<u64, wgpu::TextureView>,
    text_uploads: &mut TextUploads,
    text: &mut TextComposer,
    acc: &mut Accum,
) -> Result<(), Diag> {
    let Some(layout) = frame.text_layout.as_ref().filter(|l| l.target.is_none()) else {
        return Ok(());
    };
    let Some(screen) = passes
        .iter_mut()
        .rev()
        .find(|p| matches!(p.target, Some(PlannedTarget::Screen)))
    else {
        return Ok(());
    };
    let (width, height) = (frame.viewport.width, frame.viewport.height);
    let signature = text_signature(&[], Some(layout), (width, height));
    let id = match text_uploads.screen.as_ref() {
        Some((hash, tex)) if *hash == signature && tex.width == width && tex.height == height => {
            tex.id
        }
        _ => {
            let mut overlay = Texture {
                width,
                height,
                rgba: vec![0; (width as usize) * (height as usize) * 4],
            };
            let stats = text.paint_layout_with(&mut overlay.rgba, width, height, layout, Some(true));
            let painted = stats.glyphs;
            merge_text_stats(acc, stats);
            if painted == 0 {
                return Ok(());
            }
            let id = gpu.upload(ctx, width, height, &overlay.rgba, "text screen overlay")?;
            let uploaded = gpu.get(id).expect("overlay exists");
            let tex = Rc::new(TexRef {
                id,
                width: uploaded.width,
                height: uploaded.height,
                texture: uploaded.texture.clone(),
                view: uploaded.view.clone(),
            });
            text_uploads.screen = Some((signature, tex));
            id
        }
    };
    pipelines.ensure_pipeline(ctx, BlendMode::Normal)?;
    let view = gpu
        .get(id)
        .ok_or_else(|| {
            Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                .with_detail("文本覆盖纹理上传后缺失")
        })?
        .view
        .clone();
    views.insert(id, view.clone());
    pipelines.ensure_bind_group(ctx, id, &view, FilterKind::Point)?;
    let expected = screen.first_vertex as usize + screen.draws.len() * 6;
    if expected == vertices.len() {
        push_quad(vertices, width, height, 0.0, 0.0, width, height, 1.0);
        screen.draws.push(Draw {
            blend: BlendMode::Normal,
            texture_id: id,
            filter: FilterKind::Point,
            kind: DrawKind::Quad,
        });
    } else {
        note_text_diag(acc, "text.screen_overlay_not_final".to_string());
    }
    Ok(())
}

// ------------------------------------------------------------------ 小工具

fn image_signature(image: &ImageOp, source: u64, source2: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_bytes(&mut hash, image.action.name().as_bytes());
    for value in [image.x, image.y, image.x2, image.y2, image.sx, image.sy] {
        fnv_mix(&mut hash, value.map_or(u64::MAX, f64::to_bits));
    }
    for value in [&image.set, &image.set2] {
        fnv_mix(
            &mut hash,
            value
                .as_ref()
                .and_then(|value| value.as_int())
                .map_or(u64::MAX, |value| value as u64),
        );
    }
    fnv_mix(&mut hash, source);
    fnv_mix(&mut hash, source2);
    fnv_mix(&mut hash, u64::from(image.region_from_fixed));
    hash
}

/// 内容 → 采样窗口在源纹理上的**闭区间像素矩形**(物化/复制用)。
fn content_region(content: &Content) -> (i64, i64, i64, i64) {
    let w = content.tex.width as f32;
    let h = content.tex.height as f32;
    let x0 = (content.uv[0] * w).round() as i64;
    let y0 = (content.uv[1] * h).round() as i64;
    let x1 = ((content.uv[2] * w).round() as i64 - 1).max(x0);
    let y1 = ((content.uv[3] * h).round() as i64 - 1).max(y0);
    (x0, y0, x1, y1)
}

/// `TRIM` 的 alpha 包围盒(与 soft `apply_trim` 同规则:位掩码 8=上/4=下/
/// 2=左/1=右,只裁完全透明的行/列)。
fn trim_box(surface: &Surface, width: u32, height: u32, mask: i64) -> Option<(i64, i64, i64, i64)> {
    if width == 0 || height == 0 {
        return None;
    }
    let alpha = |x: u32, y: u32| -> u8 {
        surface.rgba[((y as usize) * (width as usize) + x as usize) * 4 + 3]
    };
    let row_empty = |y: u32| (0..width).all(|x| alpha(x, y) == 0);
    let col_empty = |x: u32| (0..height).all(|y| alpha(x, y) == 0);
    let (mut x0, mut y0) = (0i64, 0i64);
    let (mut x1, mut y1) = (i64::from(width) - 1, i64::from(height) - 1);
    if mask & 8 != 0 {
        while y0 <= y1 && row_empty(y0 as u32) {
            y0 += 1;
        }
    }
    if mask & 4 != 0 {
        while y1 >= y0 && row_empty(y1 as u32) {
            y1 -= 1;
        }
    }
    if mask & 2 != 0 {
        while x0 <= x1 && col_empty(x0 as u32) {
            x0 += 1;
        }
    }
    if mask & 1 != 0 {
        while x1 >= x0 && col_empty(x1 as u32) {
            x1 -= 1;
        }
    }
    if x0 > x1 || y0 > y1 {
        return None;
    }
    Some((x0, y0, x1, y1))
}

fn fnv_mix(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(0x100_0000_01b3);
}

fn fnv_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        fnv_mix(hash, u64::from(*byte));
    }
}


/// 层基面的**身份**(只看纹理引用,不做 GPU 解析):基图或快照目标变化 ⇒
/// 旧内容与操作前缀作废。
fn base_token(
    frame: &CompositorFrame,
    targets: &TargetSet,
    id: &LayerId,
    fallback: Option<&TextureRef>,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_bytes(&mut hash, id.as_bytes());
    let texture = frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .map(|layer| &layer.texture)
        .filter(|texture| !matches!(texture, TextureRef::None))
        .or(fallback);
    match texture {
        Some(TextureRef::Asset(key)) => {
            fnv_mix(&mut hash, 1);
            fnv_bytes(&mut hash, key);
        }
        Some(TextureRef::SolidBlack1x1) => fnv_mix(&mut hash, 2),
        Some(TextureRef::Snapshot(copy_id)) => {
            fnv_mix(&mut hash, 3);
            fnv_mix(&mut hash, copy_id.0);
            fnv_mix(
                &mut hash,
                targets.snapshot(*copy_id).map_or(0, |target| target.id),
            );
        }
        Some(TextureRef::Offscreen(group)) => {
            fnv_mix(&mut hash, 4);
            fnv_bytes(&mut hash, group.as_bytes());
            fnv_mix(
                &mut hash,
                targets.offscreen(group).map_or(0, |target| target.id),
            );
        }
        Some(TextureRef::None) | None => fnv_mix(&mut hash, 5),
    }
    hash
}
