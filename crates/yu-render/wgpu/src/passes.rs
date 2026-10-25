//! `CompositorFrame.passes` → GPU 绘制计划。
//!
//! 规划阶段只做“读契约 + 解析纹理 + 建管线/bind group + 生成顶点”,
//! 不提交 GPU 命令;执行阶段严格按本模块输出的 pass 顺序录制。
//!
//! 计数与跳过原因逐项复刻 `yu-backend-soft::render`(字段直接用它的
//! [`RenderReport`])。与 J2 之前的差别:
//!
//! - `Op::Transition`:双纹理 pipeline(配对纹理 G 通道为 mask,
//!   `clamp((30+70*fade)*(mask-(1-2*fade)), 0, 1)`,`T>=256` 为满;EVID-T-05);
//! - `Op::Copy` / 快照上的 `Op::Image`:在组离屏 pass 之后、屏幕 pass 之前
//!   执行(GPU `copy_texture_to_texture` + 快照 RECTPAINT 渲染)。快照按
//!   `CopyId` 冻结(EVID-T-12),`CopyId` 只捕获一次。
//!
//! 执行顺序上的重排是契约允许的“不改变可见结果”的缓存/pass 合并:复制读的是
//! 组/图层当前内容,图像加工写的是快照纹理,都与屏幕绘制顺序无关。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use yu_backend_soft::{
    apply_horizontal, apply_meido, apply_rect_paint, apply_resize, apply_reverse, apply_trim,
    apply_vertical, blit, text_base_key, AssetIssue,
    RenderReport, SkippedOp, Surface, TextComposer, TextPaintStats, Texture, TextureStore,
};
use yu_gfx::{
    fixed_pipeline_alpha, shader_alpha, BlendMode, Clear, CompositorFrame, CopyId, CopyOp,
    CopySource, ImageAction, ImageOp, Layer, LayerId, Op, OpStatus, PassTarget, TextDraw,
    TextureRef,
};
use yu_log::Diag;

use crate::device::{code, GpuContext};
use crate::pipelines::{FilterKind, PipelineCache};
use crate::textures::{TargetSet, TextureCache, Vertex};

/// 计划中的一个 pass 目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlannedTarget {
    Screen,
    Offscreen(LayerId),
    /// 复制/快照填充命令段(无 draw,不改变渲染目标)。
    Commands,
}

/// 一次 draw:6 个顶点 + 管线/bind group 选择。
#[derive(Clone, Debug)]
pub(crate) enum DrawKind {
    Quad,
    /// 固定管线双纹理过渡:`transition_bind_groups[bind_index]`。
    Transition {
        bind_index: u32,
    },
    /// 引擎 psC(mask R)过渡;`_DXMODE≠0`。
    TransitionPsc {
        bind_index: u32,
    },
    /// 引擎 psD(交叉淡化)过渡;`_DXMODE≠0` 且 TA<1。
    TransitionPsd {
        bind_index: u32,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub blend: BlendMode,
    pub texture_id: u64,
    pub filter: FilterKind,
    pub kind: DrawKind,
}

/// 一次 GPU 纹理复制。
#[derive(Clone, Debug)]
pub(crate) struct CopyCmd {
    /// 源纹理 id(资产/1×1/离屏/快照)。
    pub source_id: u64,
    /// 目标快照。
    pub dest: CopyId,
    /// 源区域原点与尺寸(像素,含端点换算后的 extent)。
    pub origin: (u32, u32),
    pub extent: (u32, u32),
}

/// 快照上的一次矩形填充(RECTPAINT 族)。
#[derive(Clone, Debug)]
pub(crate) struct PaintCmd {
    pub target: CopyId,
    pub alpha_only: bool,
    /// 该填充使用的 6 个顶点在 `FramePlan.vertices` 里的起点。
    pub first_vertex: u32,
    /// 颜色 bits(`f32::to_bits`),与 `PipelineCache::paint_groups` 的键一致。
    pub color: [u32; 4],
}

/// 一个已规划 pass。
#[derive(Clone, Debug, Default)]
pub(crate) struct PlannedPass {
    pub target: Option<PlannedTarget>,
    pub clear: wgpu::Color,
    pub first_vertex: u32,
    pub draws: Vec<Draw>,
    pub copies: Vec<CopyCmd>,
    pub paints: Vec<PaintCmd>,
}

/// 一帧的 GPU 执行计划。
#[derive(Default)]
pub(crate) struct FramePlan {
    pub passes: Vec<PlannedPass>,
    pub vertices: Vec<Vertex>,
    /// 过渡 bind group(按 draw 索引)。
    pub transition_bind_groups: Vec<wgpu::BindGroup>,
    /// 过渡 uniform buffer(保活,执行期由 bind group 引用)。
    #[allow(dead_code)]
    pub transition_buffers: Vec<wgpu::Buffer>,
}

impl FramePlan {
    pub fn draw_calls(&self) -> usize {
        self.passes.iter().map(|p| p.draws.len()).sum()
    }
}

/// 规划一帧,同时产出与软后端字段一致的 [`RenderReport`]。
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    uploaded_overrides: &mut BTreeMap<LayerId, (u64, u32, u32, u64)>,
    image_cache: &mut BTreeMap<yu_backend_soft::ImageCacheKey, Override>,
    backbuffer: Option<&yu_backend_soft::Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    text: &mut TextComposer,
) -> Result<(FramePlan, RenderReport), Diag> {
    targets.prune_snapshots(&live_snapshots(frame));
    // 覆盖纹理缓存只保留本帧仍存在的层,防止临时层累积。
    uploaded_overrides.retain(|id, _| frame.layers.iter().any(|layer| &layer.id == id));
    let mut acc = Accum::new(frame);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut passes: Vec<PlannedPass> = Vec::new();
    let mut transition_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
    let mut transition_buffers: Vec<wgpu::Buffer> = Vec::new();

    // 1.4 快照 CPU 镜像预捕获(DIV-GFX-058,按命令序):1.5 的文本基面解析
    // 需要快照像素,而 GPU 快照在命令段才捕获;这里先把 CPU 可得源(资产/
    // 1×1 黑/另一快照镜像)按 soft `apply_copy_op` 的同一构造(目标尺寸 +
    // 区域裁剪)落成镜像。带冻结文本的 COPY 在 1.6 段连文本一起落。
    for pass in &frame.passes {
        let PassTarget::Texture(_) = &pass.target else {
            continue;
        };
        for op in &pass.ops {
            let Op::Copy(copy) = op else { continue };
            if copy.status != OpStatus::Executable
                || !copy.texts.is_empty()
                || targets.snapshot(copy.id).is_some()
                || targets.snapshot_cpu(copy.id).is_some()
            {
                continue;
            }
            capture_copy_mirror(frame, textures, targets, copy);
        }
    }

    // 1. RECTPAINT/REVERSE/HORIZONTAL:CPU 预执行到图层覆盖纹理。
    //    目标是快照的 RECTPAINT 族推迟到命令段(GPU 直接在快照纹理上画)。
    let mut overrides: BTreeMap<LayerId, Override> = BTreeMap::new();
    // 文本合成产生的覆盖层(DIV-GFX-058):COPY 取源面时跳过(快照只带
    // 命令时刻冻结的 `copy.texts`)。
    let mut text_override_ids: BTreeSet<LayerId> = BTreeSet::new();
    // COPY 冻结文本的源面(1.6 段合成;plan_copy 直接上传为 GPU 源)。
    let mut copy_text_sources: BTreeMap<CopyId, Rc<Texture>> = BTreeMap::new();
    let mut deferred: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (pass_index, pass) in frame.passes.iter().enumerate() {
        let PassTarget::Texture(target) = &pass.target else {
            continue;
        };
        acc.passes += 1;
        for (op_index, op) in pass.ops.iter().enumerate() {
            match op {
                Op::Image(image) if op.status() == OpStatus::Executable => {
                    let image_target = image.target.as_ref().unwrap_or(&image.source);
                    if snapshot_of(frame, image_target).is_some() {
                        deferred.insert((pass_index, op_index));
                        continue;
                    }
                    acc.ops_total += 1;
                    match apply_image_to_override(
                        frame,
                        textures,
                        targets,
                        target,
                        image,
                        &mut overrides,
                        image_cache,
                    ) {
                        Ok(()) => acc.ops_drawn += 1,
                        Err(reason) => acc.skip(op, reason),
                    }
                }
                // 可执行 COPY 在命令段计数(需要先有组离屏结果)。
                Op::Copy(_) if op.status() == OpStatus::Executable => {}
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

    // 1.5 文本:图层基图(或已加工的覆盖纹理)副本上合成,再按图层
    //     `A/MODE/尺寸` 走常规 quad。与 soft 同一 `TextComposer`(字节/字体
    //     /栅格一致 ⇒ 同帧像素一致)。
    {
        let mut text_ids: BTreeSet<LayerId> =
            frame.text_draws.iter().map(|d| d.target.clone()).collect();
        if let Some(layout) = &frame.text_layout {
            if let Some(target) = &layout.target {
                text_ids.insert(target.clone());
            }
        }
        for id in text_ids {
            let Some(layer) = frame.layers.iter().find(|l| l.id == id) else {
                note_text_diag(&mut acc, format!("text.layer_missing: {}", id.display()));
                continue;
            };
            if !layer.visible {
                continue;
            }
            let draws: Vec<TextDraw> = frame
                .text_draws
                .iter()
                .filter(|d| d.target == id)
                .cloned()
                .collect();
            let layout = frame
                .text_layout
                .as_ref()
                .filter(|l| l.target.as_ref() == Some(&id));
            let base: Rc<Texture> = match overrides.get(&id) {
                Some(hit) => hit.tex.clone(),
                None => match layer_base_cpu(textures, targets, layer) {
                    Ok(base) => Rc::new(base),
                    Err(reason) => {
                        note_text_diag(&mut acc, format!("text.layer_base: {reason}"));
                        continue;
                    }
                },
            };
            let revision = frame.text_revisions.get(&id).copied().unwrap_or(0);
            let override_hash = overrides.get(&id).map(|o| o.hash);
            let base_key = text_base_key(&layer.texture, override_hash);
            let (composed, stats) =
                text.compose_layer_cached(&id, revision, base_key, &base, &draws, layout);
            merge_text_stats(&mut acc, stats);
            if let Some(composed) = composed {
                overrides.insert(id.clone(), Override::new(composed));
                text_override_ids.insert(id);
            }
        }
    }

    // 1.6 COPY 冻结文本的源面(DIV-GFX-058):命令时刻已画文本进入快照,
    // 命令之后画的文本不得进入(与 soft 的 `compose_copy_texts` 同一语义;
    // 每帧只在快照未捕获时合成一次,冻结帧零成本)。
    for pass in &frame.passes {
        let PassTarget::Texture(_) = &pass.target else {
            continue;
        };
        for op in &pass.ops {
            let Op::Copy(copy) = op else { continue };
            if copy.status != OpStatus::Executable
                || copy.texts.is_empty()
                || !matches!(copy.source, CopySource::Layer(_))
                || copy_text_sources.contains_key(&copy.id)
                || targets.snapshot(copy.id).is_some()
            {
                continue;
            }
            let source_layer = match &copy.source {
                CopySource::Layer(id) => id,
                CopySource::Group(_) => continue,
            };
            // 基面优先级与 soft 的 layer_surface 同序:图像加工覆盖纹理 >
            // 层基图(文本覆盖纹理跳过 —— 只合成冻结的 `copy.texts`);
            // 源层已被 CGEND 删除时回退命令时刻的 `source_texture`。
            let base: Option<Result<Rc<Texture>, String>> =
                if let Some(hit) = overrides.get(source_layer) {
                    if text_override_ids.contains(source_layer) {
                        None
                    } else {
                        Some(Ok(hit.tex.clone()))
                    }
                } else {
                    match copy_source_base_cpu(frame, textures, targets, copy, source_layer) {
                        Ok(tex) => Some(Ok(Rc::new(tex))),
                        Err(reason) if reason == "gfx.copy.source.gone" => None,
                        Err(reason) => Some(Err(reason)),
                    }
                };
            let base = match base {
                Some(Ok(tex)) => tex,
                Some(Err(reason)) => {
                    note_text_diag(&mut acc, format!("text.copy_base: {reason}"));
                    continue;
                }
                None => continue,
            };
            let (composed, stats) = text.compose_layer(&base, &copy.texts, None);
            merge_text_stats(&mut acc, stats);
            let Some(source) = composed else {
                continue;
            };
            copy_text_sources.insert(copy.id, Rc::new(source.clone()));
            // 镜像 = 快照面(目标尺寸 + 区域裁剪;与 soft 的 apply_copy_op
            // 同一构造),不是整个源面。
            let (sw, sh) = (source.width, source.height);
            let (width, height) = frame
                .layers
                .iter()
                .find(|layer| layer.id == copy.dest)
                .and_then(|layer| layer.image_size())
                .map(|size| (size.width, size.height))
                .unwrap_or((sw, sh));
            let mut surface = Surface::new(width, height, [0, 0, 0, 0]);
            let (x0, y0, x1, y1) = copy.region.resolve(sw, sh);
            blit(
                &Surface {
                    width: source.width,
                    height: source.height,
                    rgba: source.rgba.clone(),
                },
                &mut surface,
                x0,
                y0,
                x1,
                y1,
            );
            targets.insert_snapshot_cpu(
                copy.id,
                Texture {
                    width: surface.width,
                    height: surface.height,
                    rgba: surface.rgba,
                },
            );
        }
    }

    // 2. 命令段(按 gfx pass 顺序):COPY 与快照 RECTPAINT。
    //
    // 必须在组离屏 pass 之前规划/执行:COPY 一旦执行,快照就可被组内层采样
    // (EVID-T-11/12:复制取命令执行时状态;soft 也是先执行 texture pass)。
    // 组源 COPY 需要该组“当前状态”的内容:若本帧尚未渲染该组,先插入一个
    // 不计入报告的按需组 pass(与 soft `compose_group` 同义),随后该组的
    // 正式 offscreen pass 仍按帧序正常执行并计数。这样两端的 ops/drawn/
    // skipped 与跳过原因才能逐字段一致。
    for (pass_index, pass) in frame.passes.iter().enumerate() {
        let PassTarget::Texture(target) = &pass.target else {
            continue;
        };
        for (op_index, op) in pass.ops.iter().enumerate() {
            match op {
                Op::Copy(copy) if op.status() == OpStatus::Executable => {
                    if let CopySource::Group(group_id) = &copy.source {
                        plan_group_compose(
                            ctx,
                            frame,
                            group_id,
                            textures,
                            gpu,
                            pipelines,
                            targets,
                            &mut vertices,
                            &mut acc,
                            &overrides,
                            &text_override_ids,
                            &copy_text_sources,
                            uploaded_overrides,
                            &mut transition_bind_groups,
                            &mut transition_buffers,
                            &mut passes,
                            backbuffer,
                            backbuffer_gpu,
                        )?;
                    }
                    let mut commands = PlannedPass {
                        target: Some(PlannedTarget::Commands),
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
                            overrides: &overrides,
                            text_override_ids: &text_override_ids,
                            copy_text_sources: &copy_text_sources,
                            uploaded_overrides,
                            counted: true,
                            backbuffer,
                            backbuffer_gpu,
                            transition_bind_groups: &mut transition_bind_groups,
                            transition_buffers: &mut transition_buffers,
                            dst_width: 0,
                            dst_height: 0,
                        };
                        planner.acc.ops_total += 1;
                        match planner.plan_copy(target, copy, &mut commands) {
                            Ok(()) => planner.acc.ops_drawn += 1,
                            Err(reason) => planner.acc.skip(op, reason),
                        }
                    }
                    if !commands.copies.is_empty() {
                        passes.push(commands);
                    }
                }
                Op::Image(image) if deferred.contains(&(pass_index, op_index)) => {
                    let mut commands = PlannedPass {
                        target: Some(PlannedTarget::Commands),
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
                            overrides: &overrides,
                            text_override_ids: &text_override_ids,
                            copy_text_sources: &copy_text_sources,
                            uploaded_overrides,
                            counted: true,
                            backbuffer,
                            backbuffer_gpu,
                            transition_bind_groups: &mut transition_bind_groups,
                            transition_buffers: &mut transition_buffers,
                            dst_width: 0,
                            dst_height: 0,
                        };
                        planner.acc.ops_total += 1;
                        match planner.plan_paint(target, image, &mut commands) {
                            Ok(()) => planner.acc.ops_drawn += 1,
                            Err(reason) => planner.acc.skip(op, reason),
                        }
                    }
                    if !commands.paints.is_empty() {
                        passes.push(commands);
                    }
                }
                _ => {}
            }
        }
    }

    // 3. 组离屏 pass(后序;子组先于父组)。
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
                overrides: &overrides,
                text_override_ids: &text_override_ids,
                copy_text_sources: &copy_text_sources,
                uploaded_overrides,
                counted: true,
                backbuffer,
                backbuffer_gpu,
                transition_bind_groups: &mut transition_bind_groups,
                transition_buffers: &mut transition_buffers,
                dst_width: width,
                dst_height: height,
            };
            for op in &pass.ops {
                planner.plan_op(op, &mut planned)?;
            }
        }
        passes.push(planned);
        acc.offscreen_passes += 1;
    }

    // 4. 屏幕 pass(最后)。
    let mut screen_planned = false;
    for pass in &frame.passes {
        if !matches!(pass.target, PassTarget::Screen) {
            continue;
        }
        acc.passes += 1;
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        targets.ensure_screen(ctx, width, height, pipelines)?;
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
                overrides: &overrides,
                text_override_ids: &text_override_ids,
                copy_text_sources: &copy_text_sources,
                uploaded_overrides,
                counted: true,
                backbuffer,
                backbuffer_gpu,
                transition_bind_groups: &mut transition_bind_groups,
                transition_buffers: &mut transition_buffers,
                dst_width: width,
                dst_height: height,
            };
            for op in &pass.ops {
                planner.plan_op(op, &mut planned)?;
            }
        }
        passes.push(planned);
        screen_planned = true;
    }

    if !screen_planned {
        // 与 soft 一致:没有 Screen pass 时仍输出一张按 frame.clear 清屏的表面。
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        targets.ensure_screen(ctx, width, height, pipelines)?;
        passes.push(PlannedPass {
            target: Some(PlannedTarget::Screen),
            clear: clear_color(frame.clear),
            first_vertex: vertices.len() as u32,
            ..PlannedPass::default()
        });
    }

    // 5. 屏幕级自动文本(`target=None`):全视口透明覆盖 + 一次合成 quad。
    //
    // 没有文本图层承载基图 ⇒ 必须合成(不能 Replace 清屏);与 soft 的
    // `paint_layout_with(.., Some(true))` 同规则。
    if let Some(layout) = frame.text_layout.as_ref().filter(|l| l.target.is_none()) {
        if let Some(screen) = passes
            .iter_mut()
            .rev()
            .find(|p| matches!(p.target, Some(PlannedTarget::Screen)))
        {
            let (width, height) = (frame.viewport.width, frame.viewport.height);
            let mut overlay = Texture {
                width,
                height,
                rgba: vec![0; (width as usize) * (height as usize) * 4],
            };
            let stats =
                text.paint_layout_with(&mut overlay.rgba, width, height, layout, Some(true));
            let painted = stats.glyphs;
            merge_text_stats(&mut acc, stats);
            if painted > 0 {
                let id = gpu.upload(ctx, width, height, &overlay.rgba, "text screen overlay")?;
                pipelines.ensure_pipeline(ctx, BlendMode::Normal)?;
                let view = gpu
                    .get(id)
                    .ok_or_else(|| {
                        Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                            .with_detail("文本覆盖纹理上传后缺失")
                    })?
                    .view
                    .clone();
                pipelines.ensure_bind_group(ctx, id, &view, FilterKind::Point)?;
                // 屏幕 pass 是最后规划的 pass:顶点必须位于缓冲末尾才能整段追加。
                let expected = screen.first_vertex as usize + screen.draws.len() * 6;
                if expected == vertices.len() {
                    push_quad(&mut vertices, width, height, 0, 0, width, height, 1.0);
                    screen.draws.push(Draw {
                        blend: BlendMode::Normal,
                        texture_id: id,
                        filter: FilterKind::Point,
                        kind: DrawKind::Quad,
                    });
                } else {
                    note_text_diag(&mut acc, "text.screen_overlay_not_last_pass".to_string());
                }
            }
        }
    }

    let missing_assets = textures.issues();
    Ok((
        FramePlan {
            passes,
            vertices,
            transition_bind_groups,
            transition_buffers,
        },
        acc.finish(missing_assets),
    ))
}

fn live_snapshots(frame: &CompositorFrame) -> BTreeSet<CopyId> {
    let mut live = BTreeSet::new();
    for layer in &frame.layers {
        if let TextureRef::Snapshot(id) = &layer.texture {
            live.insert(*id);
        }
    }
    for pass in &frame.passes {
        for op in &pass.ops {
            if let Op::Copy(copy) = op {
                live.insert(copy.id);
            }
        }
    }
    live
}

/// 快照 CPU 镜像预捕获(1.4 段;DIV-GFX-058)。
///
/// 与 soft `apply_copy_op` 同一构造:目标尺寸(目标层显式尺寸,否则源面
/// 尺寸)+ 源区域裁剪写 (0,0)。只覆盖 CPU 可得源;组/后缓冲源与命令后
/// 才可用的镜像组合不在此列(需要时显式报缺,不猜)。
fn capture_copy_mirror(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &mut TargetSet,
    copy: &CopyOp,
) {
    let CopySource::Layer(source_layer) = &copy.source else {
        return;
    };
    let source: Texture = {
        let texture_ref = frame
            .layers
            .iter()
            .find(|layer| layer.id == *source_layer)
            .map(|layer| &layer.texture)
            .filter(|t| !matches!(t, TextureRef::None))
            .or(copy.source_texture.as_ref());
        let Some(texture_ref) = texture_ref else {
            return;
        };
        match texture_ref {
            TextureRef::Asset(key) => match textures.resolve(key) {
                Ok(texture) => texture.clone(),
                Err(_) => return,
            },
            TextureRef::SolidBlack1x1 => Texture::solid_black_1x1(),
            TextureRef::Snapshot(pid) => match targets.snapshot_cpu(*pid) {
                Some(texture) => texture.clone(),
                None => return,
            },
            TextureRef::None | TextureRef::Offscreen(_) => return,
        }
    };
    let (sw, sh) = (source.width, source.height);
    let (width, height) = frame
        .layers
        .iter()
        .find(|layer| layer.id == copy.dest)
        .and_then(|layer| layer.image_size())
        .map(|size| (size.width, size.height))
        .unwrap_or((sw, sh));
    let mut surface = Surface::new(width, height, [0, 0, 0, 0]);
    let (x0, y0, x1, y1) = copy.region.resolve(sw, sh);
    blit(
        &Surface {
            width: source.width,
            height: source.height,
            rgba: source.rgba.clone(),
        },
        &mut surface,
        x0,
        y0,
        x1,
        y1,
    );
    targets.insert_snapshot_cpu(
        copy.id,
        Texture {
            width: surface.width,
            height: surface.height,
            rgba: surface.rgba,
        },
    );
}

/// 图层文本合成的 CPU 基面(wgpu 版;与 soft 的 `layer_base` 同一支持范围:
/// 资产/1×1 黑/无纹理/快照镜像;离屏组显式拒绝)。
fn layer_base_cpu(
    textures: &mut TextureStore,
    targets: &TargetSet,
    layer: &Layer,
) -> Result<Texture, String> {
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map_err(|d| d.code.to_string())
            .cloned(),
        TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
        TextureRef::None => {
            let size = layer
                .image_size()
                .ok_or_else(|| "text.layer_size_unknown".to_string())?;
            Ok(Texture {
                width: size.width,
                height: size.height,
                rgba: vec![0; (size.width as usize) * (size.height as usize) * 4],
            })
        }
        TextureRef::Snapshot(copy_id) => targets
            .snapshot_cpu(*copy_id)
            .cloned()
            .ok_or_else(|| "snapshot.cpu_mirror_missing".to_string()),
        TextureRef::Offscreen(_) => Err("text.layer_source_unsupported".to_string()),
    }
}

/// COPY 冻结文本的基面(源层可能已被 `CGEND` 删除,回退命令时刻的
/// `source_texture`;DIV-GFX-058)。
fn copy_source_base_cpu(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    copy: &CopyOp,
    source_layer: &LayerId,
) -> Result<Texture, String> {
    let texture_ref = frame
        .layers
        .iter()
        .find(|layer| layer.id == *source_layer)
        .map(|layer| &layer.texture)
        .filter(|t| !matches!(t, TextureRef::None))
        .or(copy.source_texture.as_ref())
        .ok_or_else(|| "gfx.copy.source.gone".to_string())?;
    match texture_ref {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map_err(|d| d.code.to_string())
            .cloned(),
        TextureRef::SolidBlack1x1 => Ok(Texture::solid_black_1x1()),
        TextureRef::Snapshot(copy_id) => targets
            .snapshot_cpu(*copy_id)
            .cloned()
            .ok_or_else(|| "snapshot.cpu_mirror_missing".to_string()),
        TextureRef::None | TextureRef::Offscreen(_) => {
            Err("text.copy_base.unsupported".to_string())
        }
    }
}

fn snapshot_of(frame: &CompositorFrame, id: &LayerId) -> Option<CopyId> {
    frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .and_then(|layer| match &layer.texture {
            TextureRef::Snapshot(copy) => Some(*copy),
            _ => None,
        })
}

/// 帧内图像加工结果 + **一次性**内容指纹。
///
/// 指纹只用于跨帧复用 GPU 纹理。若每次绘制都重算,一条 800×600 的覆盖纹理
/// 就是 1.9 MB 的 FNV 遍历;title 期脚本每帧对 51 个层重发 `CGACT[SIZE]`,
/// 逐帧重算 ≈ 100 MB 哈希(EVID-J3-08 性能勘误)。
#[derive(Clone)]
pub(crate) struct Override {
    pub(crate) tex: Rc<Texture>,
    pub(crate) hash: u64,
}

impl Override {
    fn new(tex: Rc<Texture>) -> Self {
        let hash = hash_rgba(tex.width, tex.height, &tex.rgba);
        Self { tex, hash }
    }
}

struct Planner<'a> {
    ctx: &'a GpuContext,
    frame: &'a CompositorFrame,
    textures: &'a mut TextureStore,
    gpu: &'a mut TextureCache,
    pipelines: &'a mut PipelineCache,
    targets: &'a mut TargetSet,
    vertices: &'a mut Vec<Vertex>,
    acc: &'a mut Accum,
    overrides: &'a BTreeMap<LayerId, Override>,
    /// 文本覆盖层集合(COPY 取源面时跳过;DIV-GFX-058)。
    text_override_ids: &'a BTreeSet<LayerId>,
    /// COPY 冻结文本源面(1.6 段合成;plan_copy 上传使用)。
    copy_text_sources: &'a BTreeMap<CopyId, Rc<Texture>>,
    uploaded_overrides: &'a mut BTreeMap<LayerId, (u64, u32, u32, u64)>,
    transition_bind_groups: &'a mut Vec<wgpu::BindGroup>,
    transition_buffers: &'a mut Vec<wgpu::Buffer>,
    dst_width: u32,
    dst_height: u32,
    /// COPY 源组的按需重绘不进入报告计数(soft 的同名路径同样回滚)。
    counted: bool,
    /// 上一帧呈现内容(`COPY ID=:/W0/` 在引擎里读后缓冲;EVID-J3-06)。
    backbuffer: Option<&'a yu_backend_soft::Surface>,
    /// 后缓冲上传缓存:`(content_hash, gpu_id)`。
    backbuffer_gpu: &'a mut Option<(u64, u64)>,
}

impl Planner<'_> {
    fn skip(&mut self, op: &Op, reason: impl Into<String>) {
        if self.counted {
            self.acc.skip(op, reason);
        }
    }

    fn drawn(&mut self) {
        if self.counted {
            self.acc.ops_drawn += 1;
        }
    }

    fn plan_op(&mut self, op: &Op, out: &mut PlannedPass) -> Result<(), Diag> {
        if self.counted {
            self.acc.ops_total += 1;
        }
        match op {
            Op::Layer(layer) => self.plan_layer(op, layer, out),
            Op::Group(group) => {
                if !group.layer.visible {
                    self.skip(op, "invisible");
                    return Ok(());
                }
                if let Some(why) = status_reason(op) {
                    self.skip(op, why);
                    return Ok(());
                }
                let Some(blend) = supported_blend(group.layer.blend) else {
                    self.skip(op, format!("blend.{}", group.layer.blend.name()));
                    return Ok(());
                };
                let Some(target) = self.targets.offscreen(&group.offscreen) else {
                    self.skip(op, "offscreen.missing");
                    return Ok(());
                };
                let (width, height) = (target.width, target.height);
                if width == 0 || height == 0 {
                    self.skip(op, "group.size.zero");
                    return Ok(());
                }
                let view = target.view.clone();
                let (x, y) = (group.layer.transform.x, group.layer.transform.y);
                let alpha = fixed_pipeline_alpha(group.layer.alpha) as f32;
                let filter = FilterKind::from_mode(self.frame.filter.mag);
                self.draw(
                    out,
                    blend,
                    target.id,
                    &view,
                    filter,
                    x,
                    y,
                    width,
                    height,
                    alpha,
                    DrawKind::Quad,
                )
            }
            Op::Transition(transition) => self.plan_transition(op, transition, out),
            Op::Mask(_) => {
                let reason = status_reason(op).unwrap_or("mask.unsupported");
                self.skip(op, reason);
                Ok(())
            }
            Op::Copy(_) => {
                let reason = status_reason(op).unwrap_or("copy.unsupported");
                self.skip(op, reason);
                Ok(())
            }
            Op::Image(_) => {
                let reason = status_reason(op).unwrap_or("imageop.unsupported");
                self.skip(op, reason);
                Ok(())
            }
        }
    }

    /// 双纹理过渡:本层纹理 + 配对 mask 的独立 pipeline。
    fn plan_transition(
        &mut self,
        op: &Op,
        transition: &yu_gfx::TransitionDraw,
        out: &mut PlannedPass,
    ) -> Result<(), Diag> {
        let layer = &transition.layer;
        if !layer.visible {
            self.skip(op, "invisible");
            return Ok(());
        }
        if let Some(why) = status_reason(op) {
            self.skip(op, why);
            return Ok(());
        }
        let Some(blend) = supported_blend(layer.blend) else {
            self.skip(op, format!("blend.{}", layer.blend.name()));
            return Ok(());
        };
        let Some(to) = transition.to.as_ref() else {
            return self.plan_layer(op, layer, out);
        };
        // 引擎取样图层**当前像素**(EVID-T-05);脚本可能先加工配对层,
        // 因此优先用该层的图像加工覆盖纹理。
        let from = match transition.from_layer.as_ref() {
            Some(id) => self.layer_image(id, None),
            None => self.image_ref(&transition.from),
        };
        let from = match from {
            Ok(image) => image,
            Err(reason) => {
                self.skip(op, reason);
                return Ok(());
            }
        };
        let mask = match transition.to_layer.as_ref() {
            Some(id) => self.layer_image(id, None),
            None => self.image_ref(to),
        };
        let mask = match mask {
            Ok(image) => image,
            Err(reason) => {
                self.skip(op, reason);
                return Ok(());
            }
        };
        let (width, height) = match layer.image_size() {
            Some(size) => (size.width, size.height),
            None => (from.1, from.2),
        };
        let filter = pick_filter(self.frame, from.1, from.2, width, height);
        // `_DXMODE=0`:固定管线 G-mask 模型(J3 基线)。
        // `_DXMODE≠0`:引擎 effect 的两个转场 pass(EVID-T-09/台账 §2):
        //   Pass1(psC,mask R)= TA>=1;Pass0(psD,交叉淡化)= TA<1。
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Variant {
            Fixed,
            Psc,
            Psd,
        }
        let variant = if self.frame.dx_mode == 0 {
            Variant::Fixed
        } else if transition.ta >= 1 {
            Variant::Psc
        } else {
            Variant::Psd
        };
        let ok = match variant {
            Variant::Fixed => self.pipelines.ensure_transition_pipeline(self.ctx, blend)?,
            Variant::Psc => self.pipelines.ensure_psc_pipeline(self.ctx, blend)?,
            Variant::Psd => self.pipelines.ensure_psd_pipeline(self.ctx, blend)?,
        };
        if !ok {
            self.skip(op, format!("blend.{}", layer.blend.name()));
            return Ok(());
        }
        let sa = shader_alpha(layer.alpha) as f32;
        let uniform_values = match variant {
            Variant::Fixed => [
                transition.fade as f32,
                if transition.complete() { 1.0 } else { 0.0 },
                0.0,
                0.0,
            ],
            // psC: YS_FadeVal, YS_FadeVal2(=TA), YS_AlphaVal(=A/256)
            Variant::Psc => [transition.fade as f32, f32::from(transition.ta), sa, 0.0],
            // psD: YS_FadeVal, YS_AlphaVal
            Variant::Psd => [transition.fade as f32, sa, 0.0, 0.0],
        };
        let uniform = wgpu::util::DeviceExt::create_buffer_init(
            &self.ctx.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("yu-backend-wgpu transition fade"),
                contents: bytemuck::cast_slice(&uniform_values),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let from_texture = view_by_id(self.gpu, self.targets, from.0).ok_or_else(|| {
            Diag::fatal(code::TARGET, yu_log::Boundary::Platform).with_detail("过渡源纹理缺失")
        })?;
        let mask_texture = view_by_id(self.gpu, self.targets, mask.0).ok_or_else(|| {
            Diag::fatal(code::TARGET, yu_log::Boundary::Platform).with_detail("过渡 mask 纹理缺失")
        })?;
        let group = self.pipelines.create_transition_bind_group(
            self.ctx,
            from_texture,
            mask_texture,
            filter,
            &uniform,
        )?;
        let bind_index = self.transition_bind_groups.len() as u32;
        self.transition_bind_groups.push(group);
        self.transition_buffers.push(uniform);
        let (x, y) = (layer.transform.x, layer.transform.y);
        // 固定管线用 min(A,255)/255;shader 路径用 A/256(pass1/0 的
        // YS_AlphaVal 由 uniform 给出,顶点 alpha 只保留同一缩放以便调试)。
        let alpha = match variant {
            Variant::Fixed => fixed_pipeline_alpha(layer.alpha) as f32,
            Variant::Psc | Variant::Psd => sa,
        };
        if width > 0 && height > 0 {
            push_quad(
                self.vertices,
                self.dst_width,
                self.dst_height,
                x,
                y,
                width,
                height,
                alpha,
            );
            let kind = match variant {
                Variant::Fixed => DrawKind::Transition { bind_index },
                Variant::Psc => DrawKind::TransitionPsc { bind_index },
                Variant::Psd => DrawKind::TransitionPsd { bind_index },
            };
            out.draws.push(Draw {
                blend,
                texture_id: from.0,
                filter,
                kind,
            });
        }
        self.drawn();
        Ok(())
    }

    fn plan_layer(&mut self, op: &Op, layer: &Layer, out: &mut PlannedPass) -> Result<(), Diag> {
        if !layer.visible {
            self.skip(op, "invisible");
            return Ok(());
        }
        if let Some(why) = status_reason(op) {
            self.skip(op, why);
            return Ok(());
        }
        if !layer.parts.is_empty() {
            self.skip(op, "gfx.parts.unsupported");
            return Ok(());
        }
        let Some(blend) = supported_blend(layer.blend) else {
            self.skip(op, format!("blend.{}", layer.blend.name()));
            return Ok(());
        };
        let explicit_size = layer.image_size();
        let (x, y) = (layer.transform.x, layer.transform.y);
        let alpha = fixed_pipeline_alpha(layer.alpha) as f32;

        if let Some(ov) = self.overrides.get(&layer.id) {
            let tex = &ov.tex;
            let hash = ov.hash;
            let gpu_tex = match self.uploaded_overrides.get(&layer.id).copied() {
                Some((id, w, h, cached)) if w == tex.width && h == tex.height && cached == hash => {
                    (id, w, h)
                }
                _ => {
                    let id = self.gpu.upload(
                        self.ctx,
                        tex.width,
                        tex.height,
                        &tex.rgba,
                        "imageop layer",
                    )?;
                    self.uploaded_overrides
                        .insert(layer.id.clone(), (id, tex.width, tex.height, hash));
                    (id, tex.width, tex.height)
                }
            };
            let texture = self
                .gpu
                .get(gpu_tex.0)
                .expect("imageop override uploaded above");
            let view = texture.view.clone();
            let (width, height) = match explicit_size {
                Some(size) => (size.width, size.height),
                None => (gpu_tex.1, gpu_tex.2),
            };
            let filter = pick_filter(self.frame, gpu_tex.1, gpu_tex.2, width, height);
            return self.draw(
                out,
                blend,
                gpu_tex.0,
                &view,
                filter,
                x,
                y,
                width,
                height,
                alpha,
                DrawKind::Quad,
            );
        }

        match &layer.texture {
            TextureRef::Asset(key) => {
                if explicit_size.is_some() {
                    // 资源图 + SX/SY 的缩放/裁剪未取证;gfx 已标 Unimplemented,
                    // 这里再显式挡一次(与 soft 同一理由码)。
                    self.skip(op, "asset.size.unsupported");
                    return Ok(());
                }
                let gpu = &mut *self.gpu;
                let textures = &mut *self.textures;
                match gpu.ensure_asset(self.ctx, textures, key) {
                    Ok(texture_id) => {
                        let texture = gpu.get(texture_id).expect("uploaded texture exists");
                        let (width, height) = (texture.width, texture.height);
                        let view = texture.view.clone();
                        let filter = pick_filter(self.frame, width, height, width, height);
                        self.draw(
                            out,
                            blend,
                            texture_id,
                            &view,
                            filter,
                            x,
                            y,
                            width,
                            height,
                            alpha,
                            DrawKind::Quad,
                        )
                    }
                    Err(diag) if diag.code.starts_with("asset.") => {
                        self.skip(op, diag.code.to_string());
                        Ok(())
                    }
                    Err(diag) => Err(diag),
                }
            }
            TextureRef::SolidBlack1x1 => {
                let texture_id = self.gpu.ensure_solid_black(self.ctx)?;
                let texture = self.gpu.get(texture_id).expect("solid texture exists");
                let (natural_width, natural_height) = (texture.width, texture.height);
                let view = texture.view.clone();
                let width = explicit_size.map_or(natural_width, |size| size.width);
                let height = explicit_size.map_or(natural_height, |size| size.height);
                let filter = pick_filter(self.frame, natural_width, natural_height, width, height);
                self.draw(
                    out,
                    blend,
                    texture_id,
                    &view,
                    filter,
                    x,
                    y,
                    width,
                    height,
                    alpha,
                    DrawKind::Quad,
                )
            }
            TextureRef::Offscreen(id) => {
                let Some(target) = self.targets.offscreen(id) else {
                    self.skip(op, "offscreen.missing");
                    return Ok(());
                };
                let (width, height) = (target.width, target.height);
                let view = target.view.clone();
                let filter = FilterKind::from_mode(self.frame.filter.mag);
                self.draw(
                    out,
                    blend,
                    target.id,
                    &view,
                    filter,
                    x,
                    y,
                    width,
                    height,
                    alpha,
                    DrawKind::Quad,
                )
            }
            TextureRef::Snapshot(copy_id) => {
                let Some(target) = self.targets.snapshot(*copy_id) else {
                    self.skip(op, "snapshot.unsupported");
                    return Ok(());
                };
                let (natural_width, natural_height) = (target.width, target.height);
                let (width, height) = match explicit_size {
                    Some(size) => (size.width, size.height),
                    None => (natural_width, natural_height),
                };
                let view = target.view.clone();
                let filter = pick_filter(self.frame, natural_width, natural_height, width, height);
                self.draw(
                    out,
                    blend,
                    target.id,
                    &view,
                    filter,
                    x,
                    y,
                    width,
                    height,
                    alpha,
                    DrawKind::Quad,
                )
            }
            TextureRef::None => {
                self.skip(op, "texture.none");
                Ok(())
            }
        }
    }

    /// 规划一次 COPY:确保快照目标存在,记录 GPU 复制命令。
    fn plan_copy(
        &mut self,
        pass_target: &LayerId,
        copy: &CopyOp,
        out: &mut PlannedPass,
    ) -> Result<(), String> {
        if &copy.dest != pass_target {
            return Err("gfx.copy.target.mismatch".to_string());
        }
        // `COPY ID=:/W0/` 在引擎里读**后缓冲**(上一帧呈现),不是本帧组内容
        // (EVID-J3-06);否则会把本帧新出现的 title 层截进快照。
        let main_group = self
            .frame
            .windows
            .iter()
            .find(|w| w.number == 0)
            .and_then(|w| w.layer_set.as_ref());
        let source = match &copy.source {
            CopySource::Group(id) if Some(id) == main_group => self.backbuffer_image()?,
            CopySource::Group(id) => self.group_image(id)?,
            CopySource::Layer(id) => match self.copy_text_sources.get(&copy.id) {
                // 冻结文本已合成(1.6 段):直接上传为 COPY 源。
                Some(texture) => {
                    let gpu_id = self
                        .gpu
                        .upload(
                            self.ctx,
                            texture.width,
                            texture.height,
                            &texture.rgba,
                            "copy frozen texts",
                        )
                        .map_err(|diag| diag.code.to_string())?;
                    (gpu_id, texture.width, texture.height)
                }
                None => self.layer_image(id, Some(copy))?,
            },
        };
        // 已捕获的快照**冻结**(EVID-T-12,与 soft 的 `snapshots.contains_key`
        // 早退同义):命令段每帧都会重放这条 CopyOp,若再推一次 GPU 复制,
        // 快照内容会跟着后缓冲滚动,CGS 盖屏/淡出的起点就错了
        // (EVID-J3-06:title 淡出等于没发生)。
        if self.targets.snapshot(copy.id).is_some() {
            return Ok(());
        }
        let dest_size = self
            .frame
            .layers
            .iter()
            .find(|layer| layer.id == copy.dest)
            .and_then(|layer| layer.image_size())
            .map(|size| (size.width, size.height));
        let (width, height) = dest_size.unwrap_or((source.1, source.2));
        if width == 0 || height == 0 {
            return Err("group.size.zero".to_string());
        }
        self.targets
            .ensure_snapshot(self.ctx, copy.id, width, height, self.pipelines)
            .map_err(|diag| diag.code.to_string())?;
        let (x0, y0, x1, y1) = copy.region.resolve(source.1, source.2);
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        let x0 = x0.max(0) as u32;
        let y0 = y0.max(0) as u32;
        let run = (x1 - i64::from(x0) + 1).max(0) as u32;
        let rows = (y1 - i64::from(y0) + 1).max(0) as u32;
        let extent = (
            run.min(source.1.saturating_sub(x0)),
            rows.min(source.2.saturating_sub(y0)),
        );
        let extent = (extent.0.min(width), extent.1.min(height));
        if extent.0 == 0 || extent.1 == 0 {
            return Ok(());
        }
        out.copies.push(CopyCmd {
            source_id: source.0,
            dest: copy.id,
            origin: (x0, y0),
            extent,
        });
        Ok(())
    }

    /// 规划快照上的 RECTPAINT 族(GPU 在快照纹理上原地画)。
    fn plan_paint(
        &mut self,
        pass_target: &LayerId,
        image: &ImageOp,
        out: &mut PlannedPass,
    ) -> Result<(), String> {
        let target = image.target.as_ref().unwrap_or(&image.source);
        if target != pass_target {
            return Err("gfx.imageop.target.mismatch".to_string());
        }
        let Some(copy_id) = snapshot_of(self.frame, target) else {
            return Err("gfx.imageop.copy_source.unsupported".to_string());
        };
        let Some(snapshot) = self.targets.snapshot(copy_id) else {
            return Err("snapshot.unsupported".to_string());
        };
        let (sw, sh) = (snapshot.width, snapshot.height);
        let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
            return Err("gfx.imageop.set.missing".to_string());
        };
        let set = set as u32;
        let (alpha_only, color) = match image.action {
            ImageAction::RectPaint => (
                false,
                [
                    f32::from(((set >> 16) & 0xFF) as u8) / 255.0,
                    f32::from(((set >> 8) & 0xFF) as u8) / 255.0,
                    f32::from((set & 0xFF) as u8) / 255.0,
                    1.0,
                ],
            ),
            ImageAction::RectPaint2 => {
                (true, [0.0, 0.0, 0.0, f32::from((set & 0xFF) as u8) / 255.0])
            }
            ImageAction::RectPaint3 => (
                false,
                [
                    f32::from(((set >> 16) & 0xFF) as u8) / 255.0,
                    f32::from(((set >> 8) & 0xFF) as u8) / 255.0,
                    f32::from((set & 0xFF) as u8) / 255.0,
                    f32::from(((set >> 24) & 0xFF) as u8) / 255.0,
                ],
            ),
            _ => return Err("gfx.imageop.unsupported".to_string()),
        };
        let (x0, y0, x1, y1) = match (image.x, image.y, image.x2, image.y2) {
            (Some(x), Some(y), Some(x2), Some(y2)) => (x, y, x2, y2),
            (None, None, None, None) => (0, 0, i64::from(sw) - 1, i64::from(sh) - 1),
            _ => return Err("gfx.imageop.region.partial".to_string()),
        };
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(i64::from(sw) - 1);
        let y1 = y1.min(i64::from(sh) - 1);
        if x1 < x0 || y1 < y0 {
            return Ok(());
        }
        self.pipelines
            .ensure_paint_pipeline(self.ctx, alpha_only)
            .map_err(|diag| diag.code.to_string())?;
        self.pipelines
            .ensure_paint_bind_group(self.ctx, alpha_only, color)
            .map_err(|diag| diag.code.to_string())?;
        let first_vertex = self.vertices.len() as u32;
        push_quad(
            self.vertices,
            sw,
            sh,
            x0,
            y0,
            (x1 - x0 + 1) as u32,
            (y1 - y0 + 1) as u32,
            1.0,
        );
        out.paints.push(PaintCmd {
            target: copy_id,
            alpha_only,
            first_vertex,
            color: color.map(f32::to_bits),
        });
        // GPU 原地加工后 CPU 镜像已过时:删除而不是保留旧像素(DIV-GFX-058;
        // gfx 侧已执行加工同时清掉该层文本,正常不会再用镜像,防呆)。
        self.targets.invalidate_snapshot_cpu(copy_id);
        Ok(())
    }

    /// 组源的当前内容(离屏目标由规划阶段 2 保证存在)。
    fn group_image(&self, id: &LayerId) -> Result<(u64, u32, u32), String> {
        let target = self
            .targets
            .offscreen(id)
            .ok_or_else(|| "gfx.copy.source.offscreen_missing".to_string())?;
        Ok((target.id, target.width, target.height))
    }

    /// 上一帧呈现内容作为复制源(EVID-J3-06;`COPY ID=:/W0/`)。
    ///
    /// 内容未变时复用同一 GPU 纹理,避免逐帧重传。
    fn backbuffer_image(&mut self) -> Result<(u64, u32, u32), String> {
        let Some(buffer) = self.backbuffer else {
            // 首帧没有后缓冲(DIV-GFX-051):D3D9 设备创建后后缓冲即存在,
            // 内容是初始化清屏色(DIV-GFX-010)。主组已直绘、没有离屏 RT,
            // 所以这里上传一张 `frame.clear` 的视口大小表面作为后缓冲初值,
            // 与官方“首帧截图 = 启动黑屏”一致。
            let (width, height) = (self.frame.viewport.width, self.frame.viewport.height);
            let rgba = self
                .frame
                .clear
                .rgba
                .iter()
                .copied()
                .cycle()
                .take(width as usize * height as usize * 4)
                .collect::<Vec<u8>>();
            let id = self
                .gpu
                .upload(self.ctx, width, height, &rgba, "backbuffer initial clear")
                .map_err(|diag| diag.code.to_string())?;
            return Ok((id, width, height));
        };
        let hash = hash_rgba(buffer.width, buffer.height, &buffer.rgba);
        if let Some((cached, id)) = *self.backbuffer_gpu {
            if cached == hash {
                return Ok((id, buffer.width, buffer.height));
            }
        }
        let id = self
            .gpu
            .upload(
                self.ctx,
                buffer.width,
                buffer.height,
                &buffer.rgba,
                "backbuffer",
            )
            .map_err(|diag| diag.code.to_string())?;
        *self.backbuffer_gpu = Some((hash, id));
        Ok((id, buffer.width, buffer.height))
    }

    /// 图层源的当前图像。
    ///
    /// 源层可能已被 `CGEND` 删除(`DIV-GFX-053`):此时回退到 `CopyOp` 在命令
    /// 时刻记录的 `source_texture` 重放。
    fn layer_image(
        &mut self,
        id: &LayerId,
        fallback: Option<&CopyOp>,
    ) -> Result<(u64, u32, u32), String> {
        // 文本覆盖纹理不作为 COPY 源(DIV-GFX-058):快照面 = 基图(+图像
        // 加工)+ 命令时刻冻结的 `copy.texts`,不带帧末实时文本。转场取样
        // (`plan_transition`)不受此限制 —— 引擎取样当前像素,含已画文本。
        if let Some(ov) = self
            .overrides
            .get(id)
            .filter(|_| !self.text_override_ids.contains(id))
        {
            // 加工后的图层:内容未变时复用跨帧 GPU 纹理,避免逐帧重传。
            let texture = &ov.tex;
            let hash = ov.hash;
            if let Some((gpu_id, w, h, cached)) = self.uploaded_overrides.get(id).copied() {
                if w == texture.width && h == texture.height && cached == hash {
                    return Ok((gpu_id, w, h));
                }
            }
            let gpu_id = self
                .gpu
                .upload(
                    self.ctx,
                    texture.width,
                    texture.height,
                    &texture.rgba,
                    "copy source",
                )
                .map_err(|diag| diag.code.to_string())?;
            self.uploaded_overrides
                .insert(id.clone(), (gpu_id, texture.width, texture.height, hash));
            return Ok((gpu_id, texture.width, texture.height));
        }
        let layer = self.frame.layers.iter().find(|layer| &layer.id == id);
        // 组没有自身纹理:它的“当前图像”就是离屏 RT(与 soft 的
        // `layer_surface` 对 `TextureRef::Offscreen` 的处理同义)。
        if layer.is_some_and(|layer| layer.is_group()) {
            return self.group_image(id);
        }
        let fallback_texture = fallback.and_then(|copy| copy.source_texture.as_ref());
        let texture = match layer {
            Some(layer) => &layer.texture,
            None => fallback_texture.ok_or_else(|| "gfx.copy.source.gone".to_string())?,
        };
        match texture {
            TextureRef::Asset(key) => {
                let gpu = &mut *self.gpu;
                let textures = &mut *self.textures;
                let gpu_id = gpu
                    .ensure_asset(self.ctx, textures, key)
                    .map_err(|diag| diag.code.to_string())?;
                let texture = gpu.get(gpu_id).expect("uploaded asset exists");
                Ok((gpu_id, texture.width, texture.height))
            }
            TextureRef::SolidBlack1x1 => {
                let gpu_id = self
                    .gpu
                    .ensure_solid_black(self.ctx)
                    .map_err(|diag| diag.code.to_string())?;
                Ok((gpu_id, 1, 1))
            }
            TextureRef::Offscreen(group) => self.group_image(group),
            TextureRef::Snapshot(copy_id) => {
                let target = self
                    .targets
                    .snapshot(*copy_id)
                    .ok_or_else(|| "snapshot.unsupported".to_string())?;
                Ok((target.id, target.width, target.height))
            }
            TextureRef::None => Err("gfx.copy.source.texture_none".to_string()),
        }
    }

    /// 纹理引用 → `(纹理 id, 宽, 高)`。
    fn image_ref(&mut self, texture: &TextureRef) -> Result<(u64, u32, u32), String> {
        match texture {
            TextureRef::Asset(_) | TextureRef::SolidBlack1x1 => {
                let layer_like = TextureRef::SolidBlack1x1;
                let _ = layer_like;
                match texture {
                    TextureRef::Asset(key) => {
                        let gpu = &mut *self.gpu;
                        let textures = &mut *self.textures;
                        let id = gpu
                            .ensure_asset(self.ctx, textures, key)
                            .map_err(|diag| diag.code.to_string())?;
                        let t = gpu.get(id).expect("uploaded asset exists");
                        Ok((id, t.width, t.height))
                    }
                    _ => {
                        let id = self
                            .gpu
                            .ensure_solid_black(self.ctx)
                            .map_err(|diag| diag.code.to_string())?;
                        Ok((id, 1, 1))
                    }
                }
            }
            TextureRef::Offscreen(id) => self.group_image(id),
            TextureRef::Snapshot(copy_id) => {
                let target = self
                    .targets
                    .snapshot(*copy_id)
                    .ok_or_else(|| "snapshot.unsupported".to_string())?;
                Ok((target.id, target.width, target.height))
            }
            TextureRef::None => Err("texture.none".to_string()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        out: &mut PlannedPass,
        blend: BlendMode,
        texture_id: u64,
        view: &wgpu::TextureView,
        filter: FilterKind,
        x: i64,
        y: i64,
        width: u32,
        height: u32,
        alpha: f32,
        kind: DrawKind,
    ) -> Result<(), Diag> {
        if !self.pipelines.ensure_pipeline(self.ctx, blend)? {
            // 调用方已用 `supported_blend` 过滤;走到这里说明计划器与管线
            // 能力表不一致,按环境错误上报而不是静默吞掉。
            return Err(Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                .with_detail(format!("没有为 blend {} 建立管线", blend.name())));
        }
        self.pipelines
            .ensure_bind_group(self.ctx, texture_id, view, filter)?;
        if width > 0 && height > 0 {
            push_quad(
                self.vertices,
                self.dst_width,
                self.dst_height,
                x,
                y,
                width,
                height,
                alpha,
            );
            out.draws.push(Draw {
                blend,
                texture_id,
                filter,
                kind,
            });
        }
        self.drawn();
        Ok(())
    }
}

fn view_by_id<'a>(
    gpu: &'a TextureCache,
    targets: &'a TargetSet,
    id: u64,
) -> Option<&'a wgpu::TextureView> {
    if let Some(texture) = gpu.get(id) {
        return Some(&texture.view);
    }
    targets.view_by_id(id)
}

/// 生成一个目标矩形对应的两个三角形(位置、UV、层 alpha)。
#[allow(clippy::too_many_arguments)]
fn push_quad(
    vertices: &mut Vec<Vertex>,
    dst_width: u32,
    dst_height: u32,
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    alpha: f32,
) {
    let target_w = f64::from(dst_width);
    let target_h = f64::from(dst_height);
    let left = 2.0 * (x as f64) / target_w - 1.0;
    let right = 2.0 * ((x as f64) + f64::from(width)) / target_w - 1.0;
    let top = 1.0 - 2.0 * (y as f64) / target_h;
    let bottom = 1.0 - 2.0 * ((y as f64) + f64::from(height)) / target_h;
    let (left, right) = (left as f32, right as f32);
    let (top, bottom) = (top as f32, bottom as f32);

    let tl = Vertex {
        position: [left, top],
        uv: [0.0, 0.0],
        alpha,
    };
    let tr = Vertex {
        position: [right, top],
        uv: [1.0, 0.0],
        alpha,
    };
    let bl = Vertex {
        position: [left, bottom],
        uv: [0.0, 1.0],
        alpha,
    };
    let br = Vertex {
        position: [right, bottom],
        uv: [1.0, 1.0],
        alpha,
    };
    vertices.extend_from_slice(&[tl, tr, bl, tr, br, bl]);
}

fn clear_color(clear: Clear) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(clear.rgba[0]) / 255.0,
        g: f64::from(clear.rgba[1]) / 255.0,
        b: f64::from(clear.rgba[2]) / 255.0,
        a: f64::from(clear.rgba[3]) / 255.0,
    }
}

fn apply_image_to_override(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    pass_target: &LayerId,
    image: &ImageOp,
    overrides: &mut BTreeMap<LayerId, Override>,
    image_cache: &mut BTreeMap<yu_backend_soft::ImageCacheKey, Override>,
) -> Result<(), String> {
    let target = image.target.as_ref().unwrap_or(&image.source);
    if target != pass_target {
        return Err("gfx.imageop.target.mismatch".to_string());
    }
    let region = match (image.x, image.y, image.x2, image.y2) {
        (Some(x), Some(y), Some(x2), Some(y2)) => Some((x, y, x2, y2)),
        (None, None, None, None) => None,
        _ => return Err("gfx.imageop.region.partial".to_string()),
    };
    if image.action.is_scale() {
        let (Some(dw), Some(dh)) = (image.sx, image.sy) else {
            return Err("gfx.imageop.size.missing".to_string());
        };
        // 缩放尺寸是浮点(实测 `SY=825.5`);`<= 0` 才是「无尺寸」。
        if dw <= 0.0 || dh <= 0.0 {
            return Ok(());
        }
        // 纹理像素只能是整数:四舍五入(与 `yu-gfx`/soft 后端同口径)。
        let (dw, dh) = (dw.round() as u32, dh.round() as u32);
        let source_key = if overrides.contains_key(&image.source) {
            None
        } else {
            frame
                .layers
                .iter()
                .find(|layer| layer.id == image.source)
                .and_then(|layer| match &layer.texture {
                    TextureRef::Asset(key) => Some(yu_backend_soft::SizeSource::Asset(key.clone())),
                    TextureRef::SolidBlack1x1 => Some(yu_backend_soft::SizeSource::Solid),
                    TextureRef::Snapshot(id) => Some(yu_backend_soft::SizeSource::Snapshot(id.0)),
                    _ => None,
                })
        };
        let key = source_key.map(|source| yu_backend_soft::ImageCacheKey {
            source,
            op: yu_backend_soft::ImageOpSig::Size {
                bilinear: image.action.is_bilinear_scale(),
                width: dw,
                height: dh,
            },
            region,
        });
        if let Some(key) = &key {
            if let Some(hit) = image_cache.get(key) {
                // 指纹随纹理一起缓存:同帧内脚本会对同一层重复发同一条
                // `CGACT[SIZE]`(title 期实测 51 次),深拷贝或重算哈希都会
                // 吃掉十几毫秒(EVID-J3-08)。
                overrides.insert(target.clone(), hit.clone());
                return Ok(());
            }
        }
        if region.is_none() {
            // 无区域 = 整幅源缩放到目标尺寸。绘制目标层时纹理已按
            // `layer.image_size()`(= `SX×SY`)拉伸,因此**像素完全不用碰**:
            // 直接把源纹理交给目标层即可(与引擎 StretchRect 等价)。
            // 源已在本帧加工过时连 Rc 都不新建。
            if let Some(src) = overrides.get(&image.source) {
                let shared = src.clone();
                overrides.insert(target.clone(), shared);
                return Ok(());
            }
            let source = resolve_cpu_source(frame, textures, targets, overrides, &image.source, 0)?;
            let shared = Override::new(source);
            if let Some(key) = key {
                image_cache.insert(key, shared.clone());
            }
            overrides.insert(target.clone(), shared);
            return Ok(());
        }
        let source = resolve_cpu_source(frame, textures, targets, overrides, &image.source, 0)?;
        let scaled = yu_backend_soft::scale_region(
            &source,
            dw as u32,
            dh as u32,
            region,
            image.action.is_bilinear_scale(),
        );
        let shared = Override::new(Rc::new(scaled));
        if let Some(key) = key {
            image_cache.insert(key, shared.clone());
        }
        overrides.insert(target.clone(), shared);
        return Ok(());
    }
    // `REVERSE`/`HORIZONTAL`:整层像素变换,按“源身份 + 操作”跨帧缓存。
    // 与 soft 的同名分支逐字对齐(`DIV-GFX-052`):源已在本帧加工过时基于
    // 当前像素算一次(不缓存);否则按源身份查缓存。
    if matches!(
        image.action,
        ImageAction::Reverse | ImageAction::Horizontal | ImageAction::Vertical
    ) {
        if let Some(src) = overrides.get(&image.source) {
            let mut texture = (*src.tex).clone();
            match image.action {
                ImageAction::Reverse => apply_reverse(&mut texture),
                ImageAction::Vertical => apply_vertical(&mut texture),
                _ => apply_horizontal(&mut texture),
            }
            overrides.insert(target.clone(), Override::new(Rc::new(texture)));
            return Ok(());
        }
        if let Some(layer) = frame.layers.iter().find(|l| l.id == image.source) {
            let sig = match image.action {
                ImageAction::Reverse => yu_backend_soft::ImageOpSig::Reverse,
                ImageAction::Vertical => yu_backend_soft::ImageOpSig::Vertical,
                _ => yu_backend_soft::ImageOpSig::Horizontal,
            };
            let source_id = match &layer.texture {
                TextureRef::Asset(key) => Some(yu_backend_soft::ImageSource::Asset(key.clone())),
                TextureRef::SolidBlack1x1 => Some(yu_backend_soft::ImageSource::Solid),
                TextureRef::Snapshot(id) => Some(yu_backend_soft::ImageSource::Snapshot(id.0)),
                _ => None,
            };
            let key = source_id.map(|source| yu_backend_soft::ImageCacheKey {
                source,
                op: sig,
                region: None,
            });
            if let Some(key) = &key {
                if let Some(hit) = image_cache.get(key) {
                    overrides.insert(target.clone(), hit.clone());
                    return Ok(());
                }
            }
            let mut texture = (*resolve_base_texture(frame, textures, target)?).clone();
            match image.action {
                ImageAction::Reverse => apply_reverse(&mut texture),
                ImageAction::Vertical => apply_vertical(&mut texture),
                _ => apply_horizontal(&mut texture),
            }
            let shared = Override::new(Rc::new(texture));
            if let Some(key) = key {
                image_cache.insert(key, shared.clone());
            }
            overrides.insert(target.clone(), shared);
            return Ok(());
        }
    }
    // `TRANSITION`:两源线性插值写成目标层(与 soft 同名分支逐字对齐)。
    // 源 = `image.source` / `image.source2`,不读目标旧像素。
    if matches!(&image.action, ImageAction::Other("transition")) {
        let to_id = image
            .source2
            .as_ref()
            .ok_or_else(|| "gfx.imageop.source.missing".to_string())?;
        let from = resolve_cpu_source(frame, textures, targets, overrides, &image.source, 0)?;
        let to = resolve_cpu_source(frame, textures, targets, overrides, to_id, 0)?;
        let blend = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
        let blended = yu_backend_soft::blend_transition(&from, &to, blend);
        overrides.insert(target.clone(), Override::new(Rc::new(blended)));
        return Ok(());
    }
    // `MERGE/MERGE2/XOR/AND`:`ID`(上层)合成到 `ID2`(下层),结果写 `ID2`。
    if image.action.is_layer_composite() {
        let mut lower = match overrides.remove(target) {
            Some(ov) => Rc::try_unwrap(ov.tex).unwrap_or_else(|rc| (*rc).clone()),
            None => (*resolve_base_texture(frame, textures, target)?).clone(),
        };
        let upper = resolve_cpu_source(frame, textures, targets, overrides, &image.source, 0)?;
        match &image.action {
            ImageAction::Merge => yu_backend_soft::apply_merge(&mut lower, &upper, false),
            ImageAction::Merge2 => yu_backend_soft::apply_merge(&mut lower, &upper, true),
            ImageAction::Other("xor") => yu_backend_soft::apply_bitwise(&mut lower, &upper, true),
            ImageAction::Other("and") => yu_backend_soft::apply_bitwise(&mut lower, &upper, false),
            _ => return Err("gfx.imageop.unsupported".to_string()),
        }
        overrides.insert(target.clone(), Override::new(Rc::new(lower)));
        return Ok(());
    }
    let mut texture = match overrides.remove(target) {
        Some(ov) => Rc::try_unwrap(ov.tex).unwrap_or_else(|rc| (*rc).clone()),
        None => (*resolve_base_texture(frame, textures, target)?).clone(),
    };
    match image.action {
        ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3 => {
            let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
                return Err("gfx.imageop.set.missing".to_string());
            };
            apply_rect_paint(&mut texture, image.action.clone(), set, region)?;
        }
        ImageAction::Reverse => {
            if region.is_some() {
                return Err("gfx.imageop.region.unsupported".to_string());
            }
            apply_reverse(&mut texture);
        }
        ImageAction::Horizontal => {
            if region.is_some() {
                return Err("gfx.imageop.region.unsupported".to_string());
            }
            apply_horizontal(&mut texture);
        }
        ImageAction::Vertical => {
            if region.is_some() {
                return Err("gfx.imageop.region.unsupported".to_string());
            }
            apply_vertical(&mut texture);
        }
        ImageAction::Resize => {
            let (x, y, x2, y2) = (
                image.x.unwrap_or(0),
                image.y.unwrap_or(0),
                image.x2.unwrap_or(0),
                image.y2.unwrap_or(0),
            );
            apply_resize(&mut texture, x, y, x2, y2)?;
        }
        ImageAction::Trim => {
            let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
                return Err("gfx.imageop.set.missing".to_string());
            };
            apply_trim(&mut texture, set);
        }
        ImageAction::Other("meido") => {
            let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
                return Err("gfx.imageop.set.missing".to_string());
            };
            apply_meido(&mut texture, set);
        }
        ImageAction::Rotation => {
            let deg = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
            yu_backend_soft::apply_rotation(&mut texture, deg);
        }
        ImageAction::Other(name @ ("saido" | "akarusa" | "mono" | "mosaic" | "blur" | "diffusion"
        | "whirlpool" | "raster" | "raster1" | "raster2" | "raster3" | "blur2" | "blur3" | "blur4"
        | "blur5" | "blur6" | "rotation2")) => {
            let set = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
            let set2 = image.set2.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
            match name {
                "saido" => yu_backend_soft::apply_saido(&mut texture, set),
                "akarusa" => yu_backend_soft::apply_akarusa(&mut texture, set),
                "mono" => yu_backend_soft::apply_mono(&mut texture, set),
                "mosaic" => yu_backend_soft::apply_mosaic(&mut texture, set),
                "blur" => yu_backend_soft::apply_blur(&mut texture, set),
                "blur2" => yu_backend_soft::apply_blur(&mut texture, set.max(1) * 2),
                "blur3" => yu_backend_soft::apply_blur(&mut texture, set.max(1) * 3),
                "blur4" => yu_backend_soft::apply_blur(&mut texture, set.max(1) * 4),
                "blur5" => yu_backend_soft::apply_blur(&mut texture, set.max(1) * 5),
                "blur6" => yu_backend_soft::apply_blur(&mut texture, set.max(1) * 6),
                "diffusion" => yu_backend_soft::apply_diffusion(&mut texture, set),
                "whirlpool" => yu_backend_soft::apply_whirlpool(&mut texture, set),
                "raster" => yu_backend_soft::apply_raster(&mut texture, set, set2, true, false),
                "raster1" => yu_backend_soft::apply_raster(&mut texture, set, set2, false, true),
                "raster2" | "raster3" => {
                    yu_backend_soft::apply_raster(&mut texture, set, set2, true, true)
                }
                "rotation2" => yu_backend_soft::apply_rotation(&mut texture, set),
                _ => return Err("gfx.imageop.unsupported".to_string()),
            }
        }
        ImageAction::Other("sepia") => yu_backend_soft::apply_sepia(&mut texture),
        _ => return Err("gfx.imageop.unsupported".to_string()),
    }
    overrides.insert(target.clone(), Override::new(Rc::new(texture)));
    Ok(())
}

/// CPU 侧解析图层当前像素(资产/1×1/快照/已加工覆盖)。
///
/// `CGACT[SIZE]` 是 CPU 缩放(与 soft 同一实现),因此 wgpu 需要源层
/// 的 RGBA;**快照优先取冻结的 CPU 镜像**(与 soft 的 `snapshots` 表同义)。
///
/// 证据(演出1 黑屏,f740..f2775):COPY 的源层会在同一帧稍后被 `CGEND`
/// 删除(ERIS 背景/按钮快照都是 `CG[SPLBUF401]` → `COPY` → `CGEND[SPLBUF401]`)。
/// soft 端 `apply_copy_op` 在命令段就把像素写进 `snapshots` 并冻结,之后
/// 只读该表;旧 wgpu 端每次都按 `CopyOp.source` 惰性重建,源层已死 ⇒
/// `gfx.imageop.source.missing` ⇒ 目标层保留 `SolidBlack1x1` ⇒ 整屏黑。
fn resolve_cpu_source(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    overrides: &BTreeMap<LayerId, Override>,
    id: &LayerId,
    depth: usize,
) -> Result<Rc<Texture>, String> {
    if depth > 4 {
        return Err("gfx.imageop.source.depth".to_string());
    }
    if let Some(ov) = overrides.get(id) {
        return Ok(ov.tex.clone());
    }
    let layer = frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .ok_or_else(|| "gfx.imageop.source.missing".to_string())?;
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|texture| {
                Rc::new(Texture {
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                })
            })
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(Rc::new(Texture::solid_black_1x1())),
        TextureRef::Snapshot(copy_id) => {
            // 冻结镜像优先:它由 1.4/1.6 段在**命令时刻**捕获,源层之后
            // 被 `CGEND` 删除也不影响(与 soft `snapshots` 表同一语义)。
            if let Some(mirror) = targets.snapshot_cpu(*copy_id) {
                return Ok(Rc::new(mirror.clone()));
            }
            for pass in &frame.passes {
                for op in &pass.ops {
                    let Op::Copy(copy) = op else { continue };
                    if copy.id != *copy_id {
                        continue;
                    }
                    let source = match &copy.source {
                        CopySource::Layer(src) => {
                            resolve_cpu_source(frame, textures, targets, overrides, src, depth + 1)?
                        }
                        CopySource::Group(_) => return Err("gfx.imageop.source.group".to_string()),
                    };
                    let (x0, y0, x1, y1) = copy.region.resolve(source.width, source.height);
                    let cw = (x1 - x0 + 1).max(1) as u32;
                    let ch = (y1 - y0 + 1).max(1) as u32;
                    return Ok(Rc::new(yu_backend_soft::scale_region(
                        &source,
                        cw,
                        ch,
                        Some((x0, y0, x1, y1)),
                        false,
                    )));
                }
            }
            Err("gfx.imageop.source.snapshot_missing".to_string())
        }
        TextureRef::Offscreen(_) => Err("gfx.imageop.source.group".to_string()),
        TextureRef::None => Err("gfx.imageop.source.texture_none".to_string()),
    }
}

fn resolve_base_texture(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    target: &LayerId,
) -> Result<Rc<Texture>, String> {
    let Some(layer) = frame.layers.iter().find(|layer| &layer.id == target) else {
        return Err("gfx.imageop.target.unsupported".to_string());
    };
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|texture| {
                Rc::new(Texture {
                    width: texture.width,
                    height: texture.height,
                    rgba: texture.rgba.clone(),
                })
            })
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(Rc::new(Texture::solid_black_1x1())),
        TextureRef::Snapshot(_) => Err("gfx.imageop.copy_source.unsupported".to_string()),
        TextureRef::Offscreen(_) => Err("gfx.imageop.target.group".to_string()),
        TextureRef::None => Err("gfx.imageop.texture.none".to_string()),
    }
}

/// 为组源 COPY 按需重绘组内容(EVID-T-12:复制取命令执行时状态)。
///
/// 与 soft 的 `compose_group` 同义:不增加 pass/op 报告计数,只把组 pass 的
/// 绘制排进 GPU 队列。该组稍后的正式 offscreen pass 仍会正常执行并计数。
#[allow(clippy::too_many_arguments)]
fn plan_group_compose(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    id: &LayerId,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    vertices: &mut Vec<Vertex>,
    acc: &mut Accum,
    overrides: &BTreeMap<LayerId, Override>,
    text_override_ids: &BTreeSet<LayerId>,
    copy_text_sources: &BTreeMap<CopyId, Rc<Texture>>,
    uploaded_overrides: &mut BTreeMap<LayerId, (u64, u32, u32, u64)>,
    transition_bind_groups: &mut Vec<wgpu::BindGroup>,
    transition_buffers: &mut Vec<wgpu::Buffer>,
    passes: &mut Vec<PlannedPass>,
    backbuffer: Option<&yu_backend_soft::Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
) -> Result<(), Diag> {
    let Some(pass) = frame
        .passes
        .iter()
        .find(|pass| matches!(&pass.target, PassTarget::Offscreen(target) if target == id))
    else {
        return Ok(());
    };
    let (width, height) = group_surface_size(frame, id);
    if width == 0 || height == 0 {
        return Ok(());
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
            vertices,
            acc,
            overrides,
            text_override_ids,
            copy_text_sources,
            uploaded_overrides,
            counted: false,
            backbuffer,
            backbuffer_gpu,
            transition_bind_groups,
            transition_buffers,
            dst_width: width,
            dst_height: height,
        };
        for op in &pass.ops {
            planner.plan_op(op, &mut planned)?;
        }
    }
    passes.push(planned);
    Ok(())
}

/// 覆盖纹理内容指纹(FNV-1a 64):尺寸 + RGBA 字节。
fn hash_rgba(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in width.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    for byte in height.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    for &byte in rgba {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn note_text_diag(acc: &mut Accum, why: String) {
    if !acc.text_diags.contains(&why) {
        acc.text_diags.push(why);
        acc.text_diags.sort();
    }
}

fn merge_text_stats(acc: &mut Accum, stats: TextPaintStats) {
    acc.text_draws += stats.draws;
    acc.text_glyphs += stats.glyphs;
    acc.text_missing += stats.missing;
    for why in stats.diags {
        note_text_diag(acc, why);
    }
}

fn group_surface_size(frame: &CompositorFrame, id: &LayerId) -> (u32, u32) {
    frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .and_then(|layer| layer.group_size())
        .map_or((frame.viewport.width, frame.viewport.height), |size| {
            (size.width, size.height)
        })
}

fn status_reason(op: &Op) -> Option<&'static str> {
    match op.status() {
        OpStatus::Executable => None,
        OpStatus::Unimplemented(why) => Some(why),
    }
}

fn supported_blend(mode: BlendMode) -> Option<BlendMode> {
    match mode {
        BlendMode::Normal
        | BlendMode::Add
        | BlendMode::Subtract
        | BlendMode::Multiply
        | BlendMode::Screen
        | BlendMode::Lighten
        | BlendMode::Darken
        | BlendMode::Exclusion => Some(mode),
        _ => None,
    }
}

fn pick_filter(
    frame: &CompositorFrame,
    source_width: u32,
    source_height: u32,
    dest_width: u32,
    dest_height: u32,
) -> FilterKind {
    let mode = if dest_width > source_width || dest_height > source_height {
        frame.filter.mag
    } else {
        frame.filter.min
    };
    FilterKind::from_mode(mode)
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

struct Accum {
    frame: u64,
    viewport_width: u32,
    viewport_height: u32,
    layers: usize,
    visible_layers: usize,
    passes: usize,
    offscreen_passes: usize,
    ops_total: usize,
    ops_drawn: usize,
    skipped_ops_total: u32,
    skipped: BTreeMap<(&'static str, String, String), u32>,
    skipped_text: usize,
    text_draws: usize,
    text_glyphs: usize,
    text_missing: usize,
    text_diags: Vec<String>,
    filter_mag: &'static str,
    filter_min: &'static str,
    filter_mip: &'static str,
    filter_source: &'static str,
    color_space: &'static str,
}

impl Accum {
    fn new(frame: &CompositorFrame) -> Self {
        Self {
            frame: frame.frame,
            viewport_width: frame.viewport.width,
            viewport_height: frame.viewport.height,
            layers: frame.layers.len(),
            visible_layers: frame.layers.iter().filter(|l| l.visible).count(),
            passes: 0,
            offscreen_passes: 0,
            ops_total: 0,
            ops_drawn: 0,
            skipped_ops_total: 0,
            skipped: BTreeMap::new(),
            skipped_text: frame.text.iter().filter(|t| t.flags.is_none()).count(),
            text_draws: 0,
            text_glyphs: 0,
            text_missing: 0,
            text_diags: Vec::new(),
            filter_mag: frame.filter.mag.as_str(),
            filter_min: frame.filter.min.as_str(),
            filter_mip: frame.filter.mip.as_str(),
            filter_source: frame.filter.source.as_str(),
            color_space: frame.color_space.as_str(),
        }
    }

    fn skip(&mut self, op: &Op, reason: impl Into<String>) {
        self.skipped_ops_total += 1;
        let key = (op_kind(op), reason.into(), op_target(op));
        *self.skipped.entry(key).or_insert(0) += 1;
    }

    fn finish(self, missing_assets: Vec<AssetIssue>) -> RenderReport {
        debug_assert_eq!(
            self.ops_total,
            self.ops_drawn + self.skipped_ops_total as usize,
            "每个 op 必须恰好被绘制或跳过"
        );
        RenderReport {
            frame: self.frame,
            viewport_width: self.viewport_width,
            viewport_height: self.viewport_height,
            layers: self.layers,
            visible_layers: self.visible_layers,
            passes: self.passes,
            offscreen_passes: self.offscreen_passes,
            ops_total: self.ops_total,
            ops_drawn: self.ops_drawn,
            skipped_ops_total: self.skipped_ops_total,
            skipped: self
                .skipped
                .into_iter()
                .map(|((kind, reason, target), count)| SkippedOp {
                    kind,
                    reason,
                    target,
                    count,
                })
                .collect(),
            missing_assets,
            skipped_text: self.skipped_text,
            text_draws: self.text_draws,
            text_glyphs: self.text_glyphs,
            text_missing: self.text_missing,
            text_diags: self.text_diags,
            filter_mag: self.filter_mag,
            filter_min: self.filter_min,
            filter_mip: self.filter_mip,
            filter_source: self.filter_source,
            color_space: self.color_space,
        }
    }
}
