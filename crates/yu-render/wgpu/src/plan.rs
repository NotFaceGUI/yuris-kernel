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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use yu_gfx::{
    BlendMode, Clear, CompositorFrame, CopyId, CopyOp, CopySource, ImageAction, ImageOp, Layer,
    LayerId, Op, OpStatus, Pass, PassTarget, ScreenSnapshot, TextureRef,
};
use yu_log::Diag;
use yu_render_core::{RenderPhases, RenderReport, Surface, TextComposer, Texture, TextureStore};

use crate::device::{code, GpuContext};
use crate::passes::{
    clear_color, group_surface_size, hash_rgba, live_snapshots, merge_text_stats, note_text_diag,
    push_quad, Accum, AuxPipeline, BlitCmd, Draw, DrawKind, FramePlan, PaintCmd, PlannedPass,
    PlannedTarget, Planner, SurfaceWrites,
};
use crate::pipelines::{FilterKind, PipelineCache};
use crate::surfaces::{Content, Contents, LayerSurfaces, TexRef};
use crate::textures::{DeferredSnapshot, TargetSet, TextureCache, Vertex};

/// 单条图像加工超过这个时长无条件留痕(约一帧,用于移动端现场)。
const SLOW_OP_WARN_US: u64 = 20_000;
/// `YURIS_TICK_SPLIT` 诊断档的低阈值。
const SLOW_OP_TRACE_US: u64 = 5_000;

/// 一帧的读者集合(延迟物化的门禁)。
///
/// - `layers`:被屏幕/离屏绘制或当作图像/复制源/目标的层;
/// - `snapshots`:被层纹理或复制源直接引用的快照。
///
/// 不在这里的 COPY2 目标 = 本帧没有任何读者,可以只登记不物化。
#[derive(Default)]
struct PlanReads {
    snapshots: BTreeSet<CopyId>,
    layers: BTreeSet<LayerId>,
}

/// 快照复制去重键:源纹理 + 内容版本 + 区域 + 输出尺寸。
///
/// 源内容纹理不可变(不变量 1),因此相同键的复制结果逐像素相同,
/// 可以共享同一张快照纹理。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SnapshotDedupKey {
    source_tex: u64,
    source_hash: u64,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    out_w: u32,
    out_h: u32,
    full: bool,
}

/// 收集本帧仍会被读取的层与快照(只按 frame 的声明/命令序,不猜运行时)。
fn plan_reads(frame: &CompositorFrame, layers: &HashMap<&LayerId, &Layer>) -> PlanReads {
    let mut reads = PlanReads {
        snapshots: BTreeSet::new(),
        layers: BTreeSet::new(),
    };
    for pass in &frame.passes {
        for op in &pass.ops {
            if op.status() != OpStatus::Executable {
                continue;
            }
            match op {
                Op::Layer(layer) => {
                    reads.layers.insert(layer.id.clone());
                    if let Some(mask) = &layer.mask {
                        reads.layers.insert(mask.clone());
                    }
                }
                Op::Group(group) => {
                    reads.layers.insert(group.layer.id.clone());
                    if let Some(mask) = &group.layer.mask {
                        reads.layers.insert(mask.clone());
                    }
                }
                Op::Transition(transition) => {
                    reads.layers.insert(transition.layer.id.clone());
                    if let Some(mask) = &transition.layer.mask {
                        reads.layers.insert(mask.clone());
                    }
                    for texture in std::iter::once(&transition.from).chain(transition.to.iter()) {
                        if let TextureRef::Snapshot(id) = texture {
                            reads.snapshots.insert(*id);
                        }
                    }
                }
                Op::Mask(mask) => {
                    reads.layers.insert(mask.layer.id.clone());
                    reads.layers.insert(mask.mask.clone());
                }
                Op::Copy(copy) => {
                    // `copy.dest` 是本 op 的**写**目标,不算读者;它若被屏幕
                    // 绘制/后续 op 当源,会由对应的 `Layer`/`source` 分支
                    // 收集进 `layers`。
                    if let CopySource::Layer(id) = &copy.source {
                        reads.layers.insert(id.clone());
                    }
                    if let Some(TextureRef::Snapshot(id)) = &copy.source_texture {
                        reads.snapshots.insert(*id);
                    }
                }
                Op::Image(image) => {
                    reads.layers.insert(image.source.clone());
                    if let Some(target) = &image.target {
                        reads.layers.insert(target.clone());
                    }
                    if let Some(TextureRef::Snapshot(id)) = &image.source_texture {
                        reads.snapshots.insert(*id);
                    }
                }
            }
        }
    }
    for id in frame.text_revisions.keys() {
        reads.layers.insert(id.clone());
    }
    // 被读取的层如果直接以快照为纹理,该快照同样必须物化。
    for (id, layer) in layers {
        if reads.layers.contains(*id) {
            if let TextureRef::Snapshot(snapshot) = &layer.texture {
                reads.snapshots.insert(*snapshot);
            }
        }
    }
    reads
}

/// 图像加工的**整层覆盖**判定(不读目标旧像素)。
///
/// 与 `apply_image_pixels` 的 `replaces` 同口径:过渡由两源决定;
/// `RECTPAINT/RECTPAINT3` 无区域时后端用 `LoadOp::Clear` 覆盖整张表面。
/// `RECTPAINT2`(只写 alpha)与区域填充/滤镜/MERGE 都读旧像素,不算覆盖。
fn image_full_overwrite(image: &ImageOp) -> bool {
    if matches!(&image.action, ImageAction::Other("transition")) {
        return image.source2.is_some();
    }
    if image.region_from_fixed
        || image.x.is_some()
        || image.y.is_some()
        || image.x2.is_some()
        || image.y2.is_some()
    {
        return false;
    }
    matches!(image.action, ImageAction::RectPaint | ImageAction::RectPaint3)
}

fn image_target(image: &ImageOp) -> &LayerId {
    image.target.as_ref().unwrap_or(&image.source)
}

/// 找出"被紧接着的整层覆盖完全冲掉"的整层写(清色/过渡)。
///
/// 只处理**纹理 pass 里相邻的两条可执行图像加工**,且后一条不读目标层:
/// 前一条的输出没有任何读者,跳过它不改变最终像素。保守到"必须紧邻",
/// 因此不会跨过 COPY/文本/屏幕读取;真实的立绘链正是
/// `RECTPAINT3(清色)` 紧接 `TRANSITION(写结果)`。
fn dead_full_writes(frame: &CompositorFrame) -> BTreeSet<(LayerId, u64)> {
    let mut flat: Vec<Option<&ImageOp>> = Vec::new();
    for pass in &frame.passes {
        let PassTarget::Texture(_) = &pass.target else {
            continue;
        };
        for op in &pass.ops {
            if op.status() != OpStatus::Executable {
                continue;
            }
            match op {
                Op::Image(image) => flat.push(Some(image)),
                // Copy/其他可执行 op 会读/写层,作为邻接屏障。
                _ => flat.push(None),
            }
        }
    }
    let mut dead = BTreeSet::new();
    for pair in flat.windows(2) {
        let (Some(current), Some(next)) = (pair[0], pair[1]) else {
            continue;
        };
        if !image_full_overwrite(current) || !image_full_overwrite(next) {
            continue;
        }
        let target = image_target(current);
        if target != image_target(next) {
            continue;
        }
        // 后一条若把目标层当源读(自过渡/原地),前一条的输出仍有读者。
        if matches!(&next.action, ImageAction::Other("transition"))
            && (next.source == *target || next.source2.as_ref() == Some(target))
        {
            continue;
        }
        dead.insert((target.clone(), current.seq));
    }
    dead
}

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
    text_gpu: &mut Option<crate::text_gpu::TextGpuRenderer>,
    force_bilinear_scale: bool,
    tuning: yu_render_core::RenderTuning,
) -> Result<(FramePlan, RenderReport, RenderPhases), Diag> {
    let started = std::time::Instant::now();
    surfaces.begin_frame(frame.frame);
    let materialized = targets.materialized_ids();
    targets.prune_snapshots(&live_snapshots(frame, &materialized), pipelines);
    // #74:存活层集合只建一次(借用 ID,零克隆),三处消费共用。
    let live_ids: rustc_hash::FxHashSet<&LayerId> =
        frame.layers.iter().map(|layer| &layer.id).collect();
    {
        targets.prune_composites(&live_ids, pipelines);
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
    for id in text_uploads.retain(&live_ids) {
        gpu.remove(id, pipelines);
    }
    let timing = tuning.render_timing;
    let trace_geometry = tuning.render_trace_geometry;

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
        // 免查找:这里已经持有 `&Layer`,再让 `base_token` 去 `frame.layers`
        // 里线性搜同一个 id 就是 O(层数²)。
        let token = base_token_of(frame, targets, &layer.id, Some(layer), None);
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

    // 命令序结构核对 + 消费点采集(一次遍历):
    //
    // 1. 同一层的 op **序号前缀**必须与已应用前缀一致。日志被折叠
    //    (`supersede_*`)或 `CG[FILE]` 重建时,表面反映的是旧序列,不能从
    //    中间续接 ⇒ 回到基面重放(官方命令只执行一次,重放是幂等的)。
    // 2. `first_read`:本帧第一次把某层当**源**读取的命令号(自读自不算)。
    //    若上一帧的加工链越过了这个命令点,常驻表面是端点像素,不能当中间
    //    前缀(否则槽位 1 的 MERGE2 会读到槽位 2 的滤镜结果 —— 缩略图全部
    //    变成最后一张)。只有这类层本帧放弃缓存逐条重放,避免把立绘缩放等
    //    「读源在所有加工之后」的链路整体退回逐帧重算。
    let mut first_read: BTreeMap<LayerId, u64> = BTreeMap::new();
    {
        let mut chain_seqs: BTreeMap<LayerId, Vec<u64>> = BTreeMap::new();
        for pass in &frame.passes {
            let PassTarget::Texture(_) = &pass.target else {
                continue;
            };
            for op in &pass.ops {
                if op.status() != OpStatus::Executable {
                    continue;
                }
                match op {
                    Op::Image(image) => {
                        let dest = image.target.as_ref().unwrap_or(&image.source);
                        chain_seqs.entry(dest.clone()).or_default().push(image.seq);
                        // 历史项已在之前的帧执行完,它的读取不再要求本帧从
                        // 基图重放中间像素;只有本帧新追加的读取才可能把
                        // 消费点卡在已应用前缀中间。
                        if surfaces
                            .last_applied_seq(dest)
                            .is_some_and(|last| image.seq <= last)
                        {
                            continue;
                        }
                        if &image.source != dest {
                            first_read.entry(image.source.clone()).or_insert(image.seq);
                        }
                        if let Some(source2) = &image.source2 {
                            if source2 != dest {
                                first_read.entry(source2.clone()).or_insert(image.seq);
                            }
                        }
                    }
                    Op::Copy(copy) => {
                        if let CopySource::Layer(id) = &copy.source {
                            if id != &copy.dest {
                                first_read.entry(id.clone()).or_insert(copy.seq);
                            }
                        }
                    }
                    _ => {}
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
    let force_materialize: BTreeSet<LayerId> = first_read
        .into_iter()
        .filter(|(id, seq)| {
            surfaces
                .last_applied_seq(id)
                .is_some_and(|last| last >= *seq)
        })
        .map(|(id, _)| id)
        .collect();

    // 层表索引(`LayerId` → `&Layer`),本帧不变,建一次。
    //
    // 绘制规划里多处要「按 ID 找活层」(`resolve_source`/`layer_image`/
    // `apply_text`/`screen_snapshot_content`…)。旧写法每次
    // `frame.layers.iter().find(...)`,是 `op 数 × 层数` 的 `LayerId`
    // (含 `Vec<u8>`)逐字节比较 —— 存档界面 541~1500 层 × 数十个 op,
    // 实测 `plan` 阶段 4.8ms,是 render 尖峰的最大单项。
    //
    // 用 `&Layer` 而不是 clone:`frame` 在 `plan()` 期间只读。
    let layer_index: HashMap<&LayerId, &Layer> =
        frame.layers.iter().map(|layer| (&layer.id, layer.as_ref())).collect();

    // ---- 0.5 延迟物化补账 ----
    //
    // 上一帧登记为"当前无读者"的快照,本帧一旦被层/复制/文本引用,就必须
    // 在**任何 op 执行之前**按登记时冻结的源内容补齐。源 `Content` 由登记
    // 项持有 ⇒ 版本固定,补出来的像素仍是 COPY2 命令时刻的版本。
    let reads = if targets.defer_enabled() {
        plan_reads(frame, &layer_index)
    } else {
        // 延迟物化关闭时不需要读者集合(去重不依赖它),省一遍 ops 扫描。
        PlanReads::default()
    };
    if targets.defer_enabled() {
        let pending = targets.deferred_ids();
        for id in pending {
            let needed = reads.snapshots.contains(&id)
                || targets
                    .deferred_snapshot(id)
                    .is_some_and(|snap| reads.layers.contains(&snap.dest));
            if !needed {
                continue;
            }
            let Some(snapshot) = targets.take_deferred(id) else {
                continue;
            };
            let mut writes = SurfaceWrites {
                passes: &mut passes,
                vertices: &mut vertices,
                aux: &mut aux,
                transition: &mut transition_bind_groups,
                transition_buffers: &mut transition_buffers,
                views: &mut views,
                textures: &mut textures_map,
                keepalive: &mut keepalive,
            };
            let tex = materialize_snapshot_copy(ctx, pipelines, surfaces, &mut writes, &snapshot)
                .map_err(|detail| {
                    Diag::fatal(code::TARGET, yu_log::Boundary::Platform).with_detail(format!(
                        "延迟快照 {} 物化失败: {detail}",
                        id.0
                    ))
                })?;
            let content = snapshot_content(id, tex.clone());
            // 只有目标层当前仍引用这张快照时才写层内容;层已重建/被删时
            // 只登记快照,避免覆盖新内容。
            let dest_uses_snapshot = layer_index
                .get(&snapshot.dest)
                .is_some_and(|layer| matches!(&layer.texture, TextureRef::Snapshot(sid) if *sid == id));
            targets.set_snapshot(id, tex);
            if dest_uses_snapshot {
                surfaces.set_content(&snapshot.dest, content.clone());
                contents.insert(snapshot.dest.clone(), content);
            }
        }
    }
    // 被紧邻整层覆盖冲掉的整层写(清色/过渡):跳过可省一次全表面写。
    let dead_writes = dead_full_writes(frame);
    // 同帧同源同区域的快照去重:源内容不可变 ⇒ 结果逐像素相同,可共享纹理。
    let mut snapshot_dedup: BTreeMap<SnapshotDedupKey, Rc<TexRef>> = BTreeMap::new();

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
                        transition: &mut transition_bind_groups,
                        transition_buffers: &mut transition_buffers,
                        views: &mut views,
                        textures: &mut textures_map,
                        keepalive: &mut keepalive,
                    };
                    let result = apply_image(
                        ctx,
                        frame,
                        &layer_index,
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
                        force_materialize.contains(pass_target),
                        image,
                        &dead_writes,
                        force_bilinear_scale,
                        tuning,
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
                        transition: &mut transition_bind_groups,
                        transition_buffers: &mut transition_buffers,
                        views: &mut views,
                        textures: &mut textures_map,
                        keepalive: &mut keepalive,
                    };
                    let result = apply_copy(
                        ctx,
                        frame,
                        &layer_index,
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
                        &reads,
                        &mut snapshot_dedup,
                        tuning,
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
    if let Some(gpu_text) = text_gpu.as_mut() {
        gpu_text.begin_frame();
    }
    let mut text_jobs: Vec<crate::text_gpu::TextGpuJob> = Vec::new();
    let (text_cpu_base_us, text_cpu_info, text_gpu_stats) = apply_text(
        ctx,
        frame,
        &layer_index,
        textures,
        gpu,
        pipelines,
        surfaces,
        &mut contents,
        &mut bases,
        text_uploads,
        text,
        &mut acc,
        targets,
        text_gpu,
        &mut text_jobs,
        tuning,
    );
    let text_stats = text.take_frame_stats();
    // 文本阶段到此为止。**必须在这里取边界**:旧实现在第 5 步(屏幕级
    // 文本)之后才算 `t_text`,于是组 pass、屏幕 pass、顶点生成全被算进
    // `phases.text_us` —— 真机日志里「角色出场 render=37ms 而 ops 只有 14」
    // 就是这么误读的。下面 `rest_us` 用 `plan_us - image - text` 求得。
    let mark_text_done = started.elapsed();

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
                layers: &layer_index,
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
                layers: &layer_index,
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
    // 文本阶段只算 `apply_text` 本身(`mark_text` → `mark_text_done`);
    // 组 pass / 屏幕 pass / 屏幕级文本的耗时归入 `rest_us`。
    let t_text = mark_text_done - mark_text;

    // 帧末:把不再被引用的表面纹理放回池(快照/文本持有 `Rc` ⇒ 自动钉住)。
    surfaces.end_frame(&live_ids);
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
        text_gpu_layers: text_gpu_stats.layers,
        text_gpu_glyphs: text_gpu_stats.glyphs,
        text_gpu_draws: text_gpu_stats.draws,
        text_gpu_cached: text_gpu_stats.cached,
        text_gpu_fallback: text_gpu_stats.fallback,
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
            text_jobs,
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
    layers: &HashMap<&LayerId, &Layer>,
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
    force_materialize: bool,
    image: &ImageOp,
    dead_writes: &BTreeSet<(LayerId, u64)>,
    force_bilinear_scale: bool,
    tuning: yu_render_core::RenderTuning,
) -> Result<(), String> {
    let target = image.target.as_ref().unwrap_or(&image.source);
    if target != pass_target {
        return Err("gfx.imageop.target.mismatch".to_string());
    }
    let token = base_token(
        frame,
        targets,
        layers,
        target,
        // 原地加工且目标层已被 `CGEND` 删除:基面身份按命令时刻纹理算,
        // 与重放时 `resolve_target_base` 的 fallback 保持一致。
        if target == &image.source {
            image.source_texture.as_ref()
        } else {
            None
        },
    );
    if surfaces.sync_base(target, token) {
        contents.remove(target);
        chain_index.insert(target.clone(), 0);
    }
    // 源层/第二源层的基面身份:与目标同一规则(读路径也校验,不只是写路径)。
    {
        let mut sync = |id: &LayerId, fallback: Option<&TextureRef>| {
            let token = base_token(frame, targets, layers, id, fallback);
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
    let resolve_started = std::time::Instant::now();
    let source = resolve_source(
        ctx,
        frame,
        layers,
        textures,
        gpu,
        targets,
        surfaces,
        contents,
        bases,
        &image.source,
        image.source_texture.as_ref(),
        image.seq,
        if image.action.is_scale() {
            "gfx.copy.source.missing"
        } else {
            "gfx.copy.source.gone"
        },
    )?;
    let source2 = match &image.source2 {
        Some(id) => Some(resolve_source(
            ctx,
            frame,
            layers,
            textures,
            gpu,
            targets,
            surfaces,
            contents,
            bases,
            id,
            None,
            image.seq,
            "gfx.copy.source.gone",
        )?),
        None => None,
    };
    let resolve_us = resolve_started.elapsed().as_micros() as u64;
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
    // 已应用前缀按 **命令序号** 命中即可,不再比签名。
    //
    // 官方 `CGACT` 是命令到达时立即写层表面(REA `0x42f809`):历史项的
    // 结果已经烘进表面。若源层内容之后变化,签名会漂移;此时用新源去重放
    // 旧项既不符合官方语义,也会让文字窗口的 `MERGE2` 链每帧整条重算
    // (实测 `MESW.TX` 累积 197 条后 render 线性上涨)。基图变化由
    // `sync_base` 清空前缀,结构变化由下面的 seq 不匹配分支兜底。
    if !force_materialize
        && surfaces
            .applied_at(target, index)
            .is_some_and(|(seq, sig)| seq == image.seq && sig == signature)
    {
        *chain_index.get_mut(target).expect("inserted above") += 1;
        surfaces.note_reused_layer();
        return Ok(());
    }
    // 整层写被紧接着的整层覆盖冲掉:不落任何像素,只保留前缀记账。
    // 任何中间读者都会让 `dead_full_writes` 不再把两条判为相邻,因此
    // 这里不会吞掉被消费的中间态。
    if !force_materialize && dead_writes.contains(&(target.clone(), image.seq)) {
        if surfaces.applied_len(target) > index {
            surfaces.truncate_applied(target, index);
        }
        surfaces.push_applied(target, (image.seq, signature));
        *chain_index.get_mut(target).expect("inserted above") += 1;
        surfaces.stats_mut().dead_writes += 1;
        return Ok(());
    }
    if surfaces.applied_len(target) > index {
        // 结构核对已在帧首做过;走到这里说明命令序号在中间漂移(日志被
        // 折叠/CG 重建),表面不再是「当前前缀」的结果 ⇒ 回到基面整条重放。
        surfaces.truncate_applied(target, 0);
        surfaces.clear_content(target);
        contents.remove(target);
        *chain_index.get_mut(target).expect("inserted above") = 0;
    }
    let source_dims = (source.width, source.height, source.tex.id);
    let cpu_before = surfaces.stats().cpu_fallbacks;
    let op_started = std::time::Instant::now();
    apply_image_pixels(
        ctx, frame, textures, gpu, pipelines, targets, surfaces, contents, bases, writes, target,
        image, source, source2, force_bilinear_scale, tuning,
    )?;
    let op_us = op_started.elapsed().as_micros() as u64;
    // ≥20ms 无条件留痕(移动端首帧/首次 TRIM/大资产解码的现场);5ms 以上
    // 仅在 `YURIS_TICK_SPLIT` 诊断档打印。
    if op_us >= SLOW_OP_WARN_US
        || resolve_us >= SLOW_OP_WARN_US
        || (op_us >= SLOW_OP_TRACE_US && tuning.tick_split)
    {
        log::warn!(
            target: "yu-kernel",
            "[slowop] action={} target={} src={}x{} tex={} resolve={}us cpu_fb={} total={}us",
            image.action.name(),
            target.display(),
            source_dims.0,
            source_dims.1,
            source_dims.2,
            resolve_us,
            surfaces.stats().cpu_fallbacks.saturating_sub(cpu_before),
            op_us
        );
    }
    surfaces.push_applied(target, (image.seq, signature));
    *chain_index.get_mut(target).expect("inserted above") += 1;
    Ok(())
}

/// 目标层的基面(像素动作的“下层”):组/空纹理按 soft 同一原因拒绝。
///
/// `fallback` 只在原地加工(源=目标)且目标层已被 `CGEND` 删除时使用:
/// 官方命令到达时已写进层表面,删除不撤销;重放用命令时刻的基图。
#[allow(clippy::too_many_arguments)]
fn resolve_target_base(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    id: &LayerId,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    fallback: Option<&TextureRef>,
) -> Result<Content, String> {
    let layer = frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .or_else(|| {
            // 目标层已被本帧 `CGEND` 删除,但命令时刻屏幕快照里还留着它
            // (存档缩略图的 `MERGE2` 写 `BT...THUMB`,随后 `BT.END`):
            // 用快照里的图层表恢复命令时刻的基图,才能重放出缩略图。
            frame
                .screen_snapshots
                .values()
                .rev()
                .find_map(|snapshot| snapshot.layers.iter().find(|layer| &layer.id == id))
        });
    if let Some(layer) = layer {
        if layer.is_group() {
            return Err("gfx.imageop.target.group".to_string());
        }
    }
    // 与 `resolve_source` 同一口径:活层纹理与命令时刻不一致时以命令时刻
    // 为准;一致时沿用活层,让同帧加工链继续生效。
    let live = layer
        .map(|layer| &layer.texture)
        .filter(|texture| !matches!(texture, TextureRef::None));
    let texture = match (live, fallback) {
        (Some(live), Some(fallback)) if *live != *fallback => fallback,
        (Some(live), _) => live,
        (None, Some(fallback)) => fallback,
        (None, None) => {
            return Err(if layer.is_some() {
                "gfx.imageop.texture.none"
            } else {
                "gfx.imageop.target.unsupported"
            }
            .to_string())
        }
    };
    match texture {
        TextureRef::Snapshot(copy_id) if targets.snapshot(*copy_id).is_none() => {
            // 区域切片是零拷贝 UV 别名(无物化纹理):直接返回采样窗。
            if let Some(content) = targets.snapshot_content(*copy_id) {
                return Ok(content.clone());
            }
            Err("gfx.imageop.copy_source.unsupported".to_string())
        }
        texture => {
            let tex = resolve_ref(
                ctx,
                textures,
                gpu,
                targets,
                id,
                texture,
                layer.and_then(|layer| layer.fill),
                bases,
            )?;
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
    layers: &HashMap<&LayerId, &Layer>,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    surfaces: &LayerSurfaces,
    contents: &Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    id: &LayerId,
    fallback: Option<&TextureRef>,
    consumer_seq: u64,
    missing: &'static str,
) -> Result<Content, String> {
    let layer = layers.get(id).copied();
    // 命令时刻的纹理与活层不一致(被 `CGEND` 删除,或被同名 `CG[X]`
    // 重建成空层):官方 COPY 已经读走旧像素 ⇒ 必须按 fallback 重放。
    let replaced = match (layer, fallback) {
        (Some(layer), Some(fallback)) => layer.texture != *fallback,
        _ => false,
    };
    if !replaced {
        if let Some(content) = contents.get(id) {
            return Ok(content.clone());
        }
        // 上一帧已应用的同一条加工链(seq+签名命中)不会重写本帧 `contents`,
        // 但消费点位于整条链之后时,常驻表面就是命令时刻像素,从这里取回;
        // 若加工链越过消费点,帧首的 `force_materialize` 会改为逐条重建,
        // 这里的 `consumer_seq` 只是最后一道防线(DIV-GFX-053 的读侧)。
        let base = base_token(frame, targets, layers, id, fallback);
        if let Some(content) = surfaces.applied_content_before(id, base, consumer_seq) {
            return Ok(content.clone());
        }
    }
    let texture = match (layer, fallback) {
        // 命令时刻纹理与活层不一致 ⇒ 用命令时刻的;组源保持原有拒绝。
        (Some(layer), Some(fallback)) if !layer.is_group() && layer.texture != *fallback => {
            fallback
        }
        (Some(layer), _) if !layer.is_group() => &layer.texture,
        (Some(_), _) => return Err("gfx.copy.source.group_unsupported".to_string()),
        (None, Some(fallback)) => fallback,
        (None, None) => return Err(missing.to_string()),
    };
    if let TextureRef::Snapshot(copy_id) = texture {
        // 区域切片是零拷贝 UV 别名:消费者直接拿采样窗。
        if let Some(content) = targets.snapshot_content(*copy_id) {
            return Ok(content.clone());
        }
    }
    let tex = resolve_ref(
        ctx,
        textures,
        gpu,
        targets,
        id,
        texture,
        layer.and_then(|layer| layer.fill),
        bases,
    )?;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_bytes(&mut hash, id.as_bytes());
    fnv_mix(&mut hash, tex.id);
    // `CG[FILE=""]` 的 1×1 黑在引擎里按 `SX/SY` 铺成显式尺寸的表面;
    // 文本基面也必须按这块表面计算,否则 `CGACT[TEXT]` 的字符坐标
    // (x=50/y=50 等)会落到 1×1 图外,字体预览整块空白。
    let (width, height) = match texture {
        TextureRef::SolidBlack1x1 => layer
            .and_then(|layer| layer.image_size())
            .map(|size| (size.width, size.height))
            .unwrap_or((tex.width, tex.height)),
        _ => (tex.width, tex.height),
    };
    Ok(Content {
        tex,
        uv: [0.0, 0.0, 1.0, 1.0],
        width,
        height,
        bilinear: false,
        display: None,
        hash,
    })
}

pub(crate) fn resolve_ref(
    ctx: &GpuContext,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    targets: &TargetSet,
    id: &LayerId,
    texture: &TextureRef,
    fill: Option<[u8; 4]>,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
) -> Result<Rc<TexRef>, String> {
    let key = (id.clone(), texref_token(texture) ^ fill_token(fill));
    if let Some(hit) = bases.get(&key) {
        return Ok(hit.clone());
    }
    let tex: Rc<TexRef> = match texture {
        TextureRef::Movie { asset, frame } => {
            let gpu_id = gpu
                .ensure_movie(ctx, textures, asset, *frame)
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
                .ensure_solid_color(ctx, fill.unwrap_or([0, 0, 0, 255]))
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

/// `Layer.fill` 的身份 token(与 `texref_token` 一起组成基面缓存键)。
fn fill_token(fill: Option<[u8; 4]>) -> u64 {
    let Some(px) = fill else { return 0 };
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in px {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 纹理引用的身份(基面缓存键的一部分;同一层可在同一帧内被 `CG[FILE]`
/// 换成不同资产,只按图层 id 缓存会串图)。
fn texref_token(texture: &TextureRef) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    match texture {
        TextureRef::Movie { asset, frame } => {
            fnv_mix(&mut hash, 6);
            fnv_bytes(&mut hash, asset);
            fnv_mix(&mut hash, u64::from(*frame));
        }
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
    pipelines
        .ensure_resample_pipeline(ctx)
        .map_err(|d| d.code.to_string())?;
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
    force_bilinear_scale: bool,
    tuning: yu_render_core::RenderTuning,
) -> Result<(), String> {
    // 原地加工(源=目标)的目标层可能已被 `CGEND` 删除;官方命令到达时
    // 已把结果写进层表面,删除不撤销 ⇒ 用命令时刻记录的基图重放。
    let target_fallback = if target == &image.source {
        image.source_texture.as_ref()
    } else {
        None
    };
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
            bilinear: image.action.is_bilinear_scale() || force_bilinear_scale,
            display: Some((dw, dh)),
            hash,
        };
        // 先摘掉 `contents` 的旧 clone,`set_sampled` 才能把被替换的
        // 清色表面按引用计数放回池(下一帧整层清色直接复用)。
        contents.remove(target);
        surfaces.set_sampled(target, content.clone());
        contents.insert(target.clone(), content);
        surfaces.stats_mut().surface_passes += 0;
        return Ok(());
    }

    // ---------------- 镜像(纯采样修饰) ----------------
    if matches!(
        image.action,
        ImageAction::Horizontal | ImageAction::Vertical
    ) {
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
        contents.remove(target);
        surfaces.set_sampled(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- 双源过渡(lerp) ----------------
    if matches!(&image.action, ImageAction::Other("transition")) {
        let Some(to) = source2 else {
            return Err("gfx.imageop.source.missing".to_string());
        };
        let blend = image
            .set
            .as_ref()
            .and_then(|value| value.as_int())
            .unwrap_or(0);
        // 权重 = SET+1(官方执行体 `0x43bad0` 的 `iVar6 + 1`,范围 1..=256)。
        // 见 shaders/lerp.wgsl 与 soft 的 `blend_transition`。
        let f = (blend.clamp(0, 255) as u32) + 1;
        if to.width == 0 || to.height == 0 {
            return Ok(());
        }
        // 端点快路径:SET=255(k=256)时结果逐位等于 `to`;两源指向同一张
        // 纹理时任何 k 都等于该纹理。`to` 为恒等采样窗时可直接别名,
        // 省掉一次"读两源 + 写整张表面"的 lerp。非恒等 `to` 不能走
        // 这条(过渡是整数坐标直采,不是重采样),继续走 lerp。
        if to.is_identity() && (f >= 256 || source.tex.id == to.tex.id) {
            let content = finish_content(
                surfaces,
                to.tex.clone(),
                to.width,
                to.height,
                None,
                None,
            );
            surfaces.set_content(target, content.clone());
            contents.insert(target.clone(), content);
            surfaces.stats_mut().transitions_elided += 1;
            return Ok(());
        }
        let out = surfaces.acquire(ctx, to.width, to.height, "transition surface")?;
        // 取证:两源尺寸与内容 hash。`from_h == to_h` 说明脚本给的 ID2
        // 与 ID 是同一张纹理(过渡恒等),`to` 尺寸异常则输出画布会错。
        if tuning.trans_debug {
            log::warn!(
                target: "yu-render",
                "[trans] id={} from={}x{} h={} to={}x{} h={} set={} k={}",
                target.display(),
                source.width,
                source.height,
                source.hash,
                to.width,
                to.height,
                to.hash,
                blend,
                f
            );
        }
        pipelines
            .ensure_lerp_pipeline(ctx)
            .map_err(|d| d.code.to_string())?;
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
        // `X/Y` 是**落点**语义,不参与源区域解析:合并只覆盖「上层尺寸」那块
        // 矩形(与 CPU 参考实现 `apply_merge_at` 的循环上界一致)。
        //
        // **勘误(2026-10-31)**:此处曾写「官方合并执行体 `FUN_004405a4`
        // (0x4405a4)按部件位置画、X/Y 不参与对齐」——那是**误记**。
        // `0x4405a4` 是 `EMOTE` 域的 opcode 处理体(见
        // `crates/yu-rt/docs/domain-event-contracts.md:177`):入口第一句
        // 检查 `DAT_00801750`(YSEmote.dll 加载成功标志),它调用的
        // `DAT_00801458` 是运行时 `GetProcAddress(YSEmote.dll, 0xe9)` 绑定的
        // E-mote 绘制函数(绑定点 `FUN_004661ec`,PE 0.495)。**CGACT 的
        // MERGE 不在那里**。CGACT 合并族的 `X/Y` 与 alpha 口径目前**都没有
        // RE 级证据**,依据只有 ①像素实测(见 `merge.wgsl` 头注的残色泄漏
        // 约束)②手册 `cg/cgact.html` 例 23/24 的文字。
        //
        // 2b53786 之前 wgpu 的 CPU 合并路径**完全不看 `X/Y`**,直接
        // `apply_merge` 整面合成。2b53786 引入了「非零即跳过」
        // (`gfx.imageop.merge.offset.unsupported`)——而语料
        // `<game>`/`<game-b>` 的合并普遍带非零 `X/Y`(如 `X1*100+2*1`
        // 这类表达式),于是整条合并被丢弃:前一步 `RECTPAINT3 SET=0`
        // 清掉的矩形再也没人填回 ⇒ 立绘沿轮廓出现缝隙、眨眼帧眼部留下
        // 条带。这里恢复 2b 之前的行为:忽略 `X/Y`,整面合成。
        let _ = (image.x, image.y);
        // 下层 = 目标层**当前**表面。清空是否可见由 alpha 的混合口径决定
        // (见 `merge.wgsl` 头注)。
        let lower_content = match contents.get(target).cloned() {
            Some(content) => content,
            None => resolve_target_base(
                ctx,
                frame,
                textures,
                gpu,
                targets,
                target,
                bases,
                target_fallback,
            )?,
        };
        let upper_content = materialize(ctx, pipelines, surfaces, writes, &source, "merge upper")?;
        let lower = materialize(
            ctx,
            pipelines,
            surfaces,
            writes,
            &lower_content,
            "merge lower",
        )?;
        let (width, height) = (lower.width, lower.height);
        if width == 0 || height == 0 {
            return Ok(());
        }
        // `CG[MID]` 的官方执行体要求两层表面宽高相等(REA
        // `0x42a862..0x42a88c`);不等时不得把 mask 拉伸到目标尺寸。
        // soft 侧在 `apply_image_op` 同样拒绝,两后端跳过口径必须一致。
        if image.action == ImageAction::Other("mask_alpha")
            && (lower.tex.width != upper_content.tex.width
                || lower.tex.height != upper_content.tex.height)
        {
            return Err("gfx.mask.size.mismatch".to_string());
        }
        let out = surfaces.acquire(ctx, width, height, "merge surface")?;
        // 层不透明度(`[层+0x80]`,0..256),命令时刻快照。REA 0.495:
        // - `MERGE`  `FUN_0043b33c`:`blend(dst, src, srcLayerAlpha, span)`
        //   ⇒ **只用上层的**;这里给下层传 256(恒等),不让它影响结果。
        // - `MERGE2` `FUN_0043b8ec`:`blend(dst, src, dstLayerAlpha, srcLayerAlpha, span)`
        //   ⇒ 两层都用。
        let src_la = u32::try_from(image.source_alpha.clamp(0, 256)).unwrap_or(256);
        let dst_la = u32::try_from(image.target_alpha.clamp(0, 256)).unwrap_or(256);
        // `MERGE` 用已验证逐位一致的专用 shader(`merge.wgsl`);`MERGE2`/
        // `XOR`/`AND` 走 `composite.wgsl`(mode 0/1/2)。
        let (bind, pipeline) = if image.action == ImageAction::Merge {
            pipelines
                .ensure_layer_merge_pipeline(ctx)
                .map_err(|d| d.code.to_string())?;
            (
                pipelines
                    .create_layer_merge_bind_group(
                        ctx,
                        &lower.tex.view,
                        &upper_content.tex.view,
                        src_la,
                        256,
                    )
                    .map_err(|diag| diag.code.to_string())?,
                AuxPipeline::LayerMerge,
            )
        } else {
            let mode = match image.action {
                ImageAction::Merge2 => 0,
                ImageAction::Other("xor") => 1,
                // `CG[MID]`:target.a = mask.r,RGB 不动(REA `0x42a88c`)。
                ImageAction::Other("mask_alpha") => 3,
                _ => 2,
            };
            pipelines
                .ensure_composite_pipeline(ctx)
                .map_err(|d| d.code.to_string())?;
            let bg = pipelines
                .create_composite_bind_group(
                    ctx,
                    &lower.tex.view,
                    &upper_content.tex.view,
                    mode,
                    src_la,
                    dst_la,
                )
                .map_err(|diag| diag.code.to_string())?;
            (bg, AuxPipeline::Composite)
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
            None => resolve_target_base(
                ctx,
                frame,
                textures,
                gpu,
                targets,
                target,
                bases,
                target_fallback,
            )?,
        };
        let alpha_only = matches!(image.action, ImageAction::RectPaint2);
        let replaces = int_region.is_none() && !alpha_only;
        let display = current.display;
        // 整层替换会被 clear 全覆盖:基面像素不参与结果,不必物化
        // (非恒等采样时这里原本会白跑一次重采样 pass)。
        let base = if replaces {
            None
        } else {
            Some(materialize(
                ctx,
                pipelines,
                surfaces,
                writes,
                &current,
                "paint base",
            )?)
        };
        let (width, height) = base
            .as_ref()
            .map(|base| (base.width, base.height))
            .unwrap_or((current.width, current.height));
        let out = surfaces.acquire(ctx, width, height, "paint surface")?;
        writes.remember(&out);
        if !replaces {
            let base = base.as_ref().expect("非整层替换必须有基面");
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
            if replaces {
                // **全图替换 = 整层填 SET 颜色** ⇒ 用硬件 `LoadOp::Clear`。
                //
                // 与画一个全屏 quad 逐像素等价(颜色相同、写入相同),但 clear
                // 走 GPU 快速路径,不占采样/混合单元。立绘过渡链每帧要清
                // 4 张 1332×3351(每张 17.8MB),旧写法是纯带宽浪费:实测
                // `queue.submit` 被背压拖到 0.6~1.1ms,正是 `render` 抖动来源。
                let mut pass = PlannedPass {
                    target: Some(PlannedTarget::Commands),
                    ..PlannedPass::default()
                };
                pass.clear_colors.push((out.id, color));
                writes.passes.push(pass);
                surfaces.stats_mut().surface_passes += 1;
            } else {
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
            }
        } else {
            // 区域为空:保留拷贝结果(与 CPU 循环不写像素一致)。
            surfaces.stats_mut().surface_passes += 0;
        }
        let content = finish_content(surfaces, out, width, height, display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }

    // ---------------- 其余:需要整块像素的原地动作 ----------------
    let current = match contents.get(target).cloned() {
        Some(content) => content,
        None => resolve_target_base(
            ctx,
            frame,
            textures,
            gpu,
            targets,
            target,
            bases,
            target_fallback,
        )?,
    };
    // `DIFFUSION` 的 `SET/2 <= 0` 在官方只做一次整面 memcpy ⇒ 恒等;
    // 提前返回,连基面都不必物化。
    if image.action.name() == "diffusion"
        && image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0) / 2 <= 0
    {
        return Ok(());
    }
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
        // RASTER 族与 WHIRLPOOL 走 `transform.wgsl` 的 16..19。
        ImageAction::Other("raster") => Some(16),
        ImageAction::Other("raster2") => Some(17),
        ImageAction::Other("raster3") => Some(18),
        ImageAction::Other("whirlpool") => Some(19),
        _ => None,
    };
    // `DIFFUSION`(`FUN_004358d4`):每轮两遍(正向 + 反向),每遍一次 fullscreen
    // pass;`SET/2` 轮。每块消耗一个 `rand()`,着色器按块号直跳第 k 个。
    if image.action.name() == "diffusion" {
        let (width, height) = (base.width, base.height);
        let passes = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0) / 2;
        if passes <= 0 || width < 2 || height < 2 {
            return Ok(());
        }
        let (x1, y1) = (i64::from(width) - 1, i64::from(height) - 1);
        let rows = (y1 + 1) / 2;
        pipelines
            .ensure_transform_pipeline(ctx)
            .map_err(|d| format!("{}: {}", d.code, d.detail))?;
        let mut current = base.tex.clone();
        for it in 0..passes {
            let nx = (x1 - it + 1) / 2;
            if nx <= 0 {
                break;
            }
            for dir in 0..2u32 {
                let out = surfaces.acquire(ctx, width, height, "diffusion pass")?;
                let base_k = surfaces.diffusion_blocks();
                let bind = pipelines
                    .create_transform_bind_group_n(
                        ctx,
                        &current.view,
                        [20, 0, 0, 0],
                        [0.0; 4],
                        [dir, it as u32, base_k, nx as u32],
                    )
                    .map_err(|diag| format!("{}: {}", diag.code, diag.detail))?;
                writes.fullscreen(&out, bind, AuxPipeline::Transform);
                surfaces.stats_mut().surface_passes += 1;
                surfaces.advance_diffusion_blocks((rows * nx) as u32);
                current = out;
            }
        }
        let content = finish_content(surfaces, current, width, height, base.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }
    // `BLUR`/`BLUR2..6`:两遍盒式(先 X 再 Y),逐条对齐官方 0.495
    // `FUN_00431d28`(action 0x02)与 `FUN_00434b7c`(action 0x07):
    // 半径 = max(1, SET)、R/G/B 整数平均、除数 = 实际窗口像素数、alpha 保留。
    let blur = matches!(
        &image.action,
        ImageAction::Other("blur" | "blur2" | "blur3" | "blur4" | "blur5" | "blur6")
    );
    if blur {
        let (width, height) = (base.width, base.height);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let radius = image
            .set
            .as_ref()
            .and_then(|v| v.as_int())
            .unwrap_or(0)
            .max(1) as i32;
        pipelines
            .ensure_transform_pipeline(ctx)
            .map_err(|d| format!("{}: {}", d.code, d.detail))?;
        let mid = surfaces.acquire(ctx, width, height, "blur horizontal")?;
        let out = surfaces.acquire(ctx, width, height, "blur vertical")?;
        let bind_x = pipelines
            .create_transform_bind_group(ctx, &base.tex.view, [10, radius, 0, 0], [0.0; 4])
            .map_err(|diag| format!("{}: {}", diag.code, diag.detail))?;
        writes.fullscreen(&mid, bind_x, AuxPipeline::Transform);
        let bind_y = pipelines
            .create_transform_bind_group(ctx, &mid.view, [11, radius, 0, 0], [0.0; 4])
            .map_err(|diag| format!("{}: {}", diag.code, diag.detail))?;
        writes.fullscreen(&out, bind_y, AuxPipeline::Transform);
        surfaces.stats_mut().surface_passes += 2;
        let content = finish_content(surfaces, out, width, height, base.display, None);
        surfaces.set_content(target, content.clone());
        contents.insert(target.clone(), content);
        return Ok(());
    }
    if let Some(mode) = mode {
        let (width, height) = (base.width, base.height);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let set = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0) as i32;
        let set2 = image.set2.as_ref().and_then(|v| v.as_int()).unwrap_or(0) as i32;
        let out = surfaces.acquire(ctx, width, height, "filter surface")?;
        pipelines
            .ensure_transform_pipeline(ctx)
            .map_err(|d| format!("{}: {}", d.code, d.detail))?;
        // 旋涡的半径表长度 trunc((hypot(w,h)+1)*0.5+1)(着色器无 f64)。
        let n = if mode == 19 {
            let hyp = (f64::from(width)).powi(2) + (f64::from(height)).powi(2);
            let maxr = ((hyp.sqrt() + 1.0) * 0.5 + 1.0).max(1.0) as u32;
            [maxr, 0, 0, 0]
        } else {
            [0; 4]
        };
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
            .create_transform_bind_group_n(ctx, &base.tex.view, [mode, set, set2, 0], m, n)
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
        // **优先走资产 alpha 缓存**:`TRIM` 只裁四周全透明的行/列,边界是
        // 资产的固有属性(解码时 CPU 像素就在手上)。整图读回在移动端
        // TBDR 上等于「全部 tile 刷回主存」,真机实测 100~291ms/次
        // (`phases.image=291740us`) —— 这是「角色一出来就卡」的直接原因。
        //
        // 取不到资产 key(快照/离屏/合成结果)时先看**内容纹理本身**是否
        // 就是一张资产纹理:整幅 COPY2 是零拷贝别名,快照图层拿到的
        // `base.tex` 仍是资产纹理。这种情况直接用已解码的 CPU 像素算边界
        // (免 GPU 读回);只有区域复制/合成结果才走四边窄带读回。
        // 资产 CPU 像素已被预算回收时(`resident == false`)不重新解码,
        // 改读 GPU —— 避免在渲染线程上再付一次解码成本。
        let asset_key = source_asset_key(frame, target).or_else(|| {
            let key = gpu.asset_key_for_id(base.tex.id)?;
            textures.resident(key).then_some(key)
        });
        let bounds = match asset_key {
            Some(key) => match textures.trim_bounds(key, set) {
                Some(b) => (
                    i64::from(b.0),
                    i64::from(b.1),
                    i64::from(b.2),
                    i64::from(b.3),
                ),
                // 资产已解析但整体全透明:官方保持原样(同 soft `apply_trim`)。
                None => return Ok(()),
            },
            None => {
                // 非资产源没有 CPU 像素:旧路径每次整图读回。像素内容不可变
                // ⇒ 按 `(纹理 id, 内容 hash, mask)` 缓存边界,首次之后零读回。
                let key = (base.tex.id, base.hash, set);
                if let Some(hit) = surfaces.content_trim_bounds(key) {
                    let Some(b) = hit else {
                        return Ok(());
                    };
                    b
                } else {
                    let bounds = crate::readback::trim_bounds(
                        ctx,
                        &base.tex.texture,
                        width,
                        height,
                        set,
                    )
                    .map_err(|diag| diag.code.to_string())?;
                    surfaces.stats_mut().cpu_fallbacks += 1;
                    surfaces.set_content_trim_bounds(
                        key,
                        bounds.map(|(x0, y0, x1, y1)| {
                            (
                                i64::from(x0),
                                i64::from(y0),
                                i64::from(x1),
                                i64::from(y1),
                            )
                        }),
                    );
                    let Some(b) = bounds else {
                        return Ok(());
                    };
                    (
                        i64::from(b.0),
                        i64::from(b.1),
                        i64::from(b.2),
                        i64::from(b.3),
                    )
                }
            }
        };
        let (x0, y0, x1, y1) = bounds;
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
    layers: &HashMap<&LayerId, &Layer>,
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
    reads: &PlanReads,
    dedup: &mut BTreeMap<SnapshotDedupKey, Rc<TexRef>>,
    tuning: yu_render_core::RenderTuning,
) -> Result<(), String> {
    if &copy.dest != pass_target {
        return Err("gfx.copy.target.mismatch".to_string());
    }
    if targets.has_snapshot(copy.id) {
        // 已冻结(EVID-T-12)。
        return Ok(());
    }
    if let CopySource::Layer(id) = &copy.source {
        let token = base_token(frame, targets, layers, id, copy.source_texture.as_ref());
        if surfaces.sync_base(id, token) {
            contents.remove(id);
        }
    }
    let source = match &copy.source {
        CopySource::Layer(id) => {
            let content = resolve_source(
                ctx,
                frame,
                layers,
                textures,
                gpu,
                targets,
                surfaces,
                contents,
                bases,
                id,
                copy.source_texture.as_ref(),
                copy.seq,
                "gfx.copy.source.gone",
            )?;
            if copy.texts.is_empty() {
                content
            } else {
                // 冻结文本:命令时刻已画的字形进快照(DIV-GFX-058)。
                let raw = surfaces.raw_content(id).unwrap_or_else(|| content.clone());
                let (cpu, _info) = content_cpu(ctx, layers, textures, contents, id, &raw)?;
                let refs: Vec<&yu_gfx::TextDraw> = copy.texts.iter().collect();
                let images = text_images_for_draws(textures, &refs);
                let (composed, stats) = text.compose_layer_assets(&cpu, &copy.texts, None, &images);
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
                        let tex = Rc::new(TexRef {
                            id: id_upload,
                            width: uploaded.width,
                            height: uploaded.height,
                            texture: uploaded.texture.clone(),
                            view: uploaded.view.clone(),
                        });
                        // 只在本次复制里消费;像素由 `TexRef` 持有,缓存项
                        // 与 bind group 不必跨帧保留。
                        gpu.remove(id_upload, pipelines);
                        Content {
                            tex,
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
                // 主窗口截屏读**命令执行时**的画面(同帧两次截屏可以不同:
                // CGTSS 旧屏 → 隐藏 UI → CGSSS 新屏)。gfx 记录了命令时刻
                // 的绘制契约;没有快照时(首帧/旧帧)回退后缓冲。
                if let Some(snapshot) = frame.screen_snapshots.get(&copy.id) {
                    Content::identity(
                        screen_snapshot_content(
                            ctx, frame, layers, snapshot, textures, gpu, pipelines, targets,
                            surfaces,
                            contents, bases, writes, text, copy.id, tuning,
                        )?,
                        0,
                    )
                } else {
                    backbuffer_content(
                        ctx,
                        frame,
                        gpu,
                        pipelines,
                        backbuffer,
                        backbuffer_gpu,
                        writes,
                    )?
                }
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
    let full = x0 == 0 && y0 == 0 && x1 == i64::from(sw) - 1 && y1 == i64::from(sh) - 1;
    // 显式区域可能超出源表面(脚本给 `X2=263` 而源只有 200 宽)。soft 的
    // `blit` 按源边界裁剪、落点仍在 (0,0);这里同样裁剪 —— 越界范围交给
    // `copy_texture_to_texture` 会让整条命令缓冲失效。
    let clip_x0 = x0.max(0);
    let clip_y0 = y0.max(0);
    let clip_x1 = x1.min(i64::from(sw) - 1);
    let clip_y1 = y1.min(i64::from(sh) - 1);
    let clip_w = (clip_x1 - clip_x0 + 1).max(0) as u32;
    let clip_h = (clip_y1 - clip_y0 + 1).max(0) as u32;
    // 局部复制的快照表面保持**源表面尺寸**(EVID-T-11:区域块写到 (0,0),
    // 其余清空),不能把区域放大到目标层尺寸。按钮状态层是典型:
    // `COPY2 X=36 X2=53` 取 18×18 状态标,随后 `SIZE` 按 50×18 裁剪
    // `X=0 X2=50`;若快照只有 18×18,`SIZE` 会把状态标横向拉伸 ≈2.8 倍,
    // 盖住背景图里烤好的 “on/off” 标签(2026-11-05 商業包 環境設定)。
    //
    // 组源(主窗口截屏)保持区域尺寸,避免改变既有截图裁剪语义。
    // 因此图层源的局部复制不再走 UV 零拷贝别名(别名只有区域尺寸);
    // 区域外像素由下面的清屏保证透明。全幅复制的零拷贝快路径保持不变。
    let layer_source = matches!(copy.source, CopySource::Layer(_));
    let (out_w, out_h) = if layer_source {
        (sw, sh)
    } else {
        (width, height)
    };
    let snapshot = DeferredSnapshot {
        dest: copy.dest.clone(),
        source: source.clone(),
        full,
        x0,
        y0,
        x1,
        y1,
        out_w,
        out_h,
        clip_x0,
        clip_y0,
        clip_w,
        clip_h,
    };
    // 同帧同源同区域去重:源内容不可变 ⇒ 结果逐像素相同,可共享纹理。
    let dedup_key = copy.texts.is_empty().then(|| SnapshotDedupKey {
        source_tex: source.tex.id,
        source_hash: source.hash,
        x0,
        y0,
        x1,
        y1,
        out_w,
        out_h,
        full,
    });
    // 延迟物化只在当前帧**没有任何读者**时登记:目标层既不绘制也不被
    // 后续 op 当源,快照本身也不被层/复制引用。后续帧真正用到时,由
    // `plan` 头部按登记时冻结的源内容补齐(版本固定)。
    let may_defer = copy.texts.is_empty()
        && !reads.snapshots.contains(&copy.id)
        && !reads.layers.contains(&copy.dest);
    let tex = if full && source.is_identity() && out_w == sw && out_h == sh {
        // 别名:快照 = 源内容句柄(零拷贝)。内容纹理永不原地改写
        // (surfaces 不变量 1)⇒ 冻结语义天然成立。
        surfaces.note_alias_hit();
        source.tex.clone()
    } else if let Some(hit) = dedup_key
        .as_ref()
        .and_then(|key| dedup.get(key).cloned())
    {
        surfaces.note_alias_hit();
        surfaces.stats_mut().snapshot_deduped += 1;
        hit
    } else if targets.defer_enabled() && may_defer {
        targets.defer_snapshot(copy.id, snapshot);
        surfaces.stats_mut().snapshots_deferred += 1;
        return Ok(());
    } else {
        let tex = materialize_snapshot_copy(ctx, pipelines, surfaces, writes, &snapshot)?;
        if let Some(key) = dedup_key {
            dedup.insert(key, tex.clone());
        }
        tex
    };
    targets.set_snapshot(copy.id, tex.clone());
    let content = snapshot_content(copy.id, tex);
    contents.insert(copy.dest.clone(), content.clone());
    surfaces.set_content(&copy.dest, content);
    Ok(())
}

/// 快照内容身份(与既有 `COPY` 落点一致):`CopyId + 纹理 id`。
fn snapshot_content(id: CopyId, tex: Rc<TexRef>) -> Content {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, id.0);
    fnv_mix(&mut hash, tex.id);
    Content::identity(tex, hash)
}

/// 按登记的复制参数物化一张快照(对 WGPU 和延迟补账共用)。
fn materialize_snapshot_copy(
    ctx: &GpuContext,
    pipelines: &mut PipelineCache,
    surfaces: &mut LayerSurfaces,
    writes: &mut SurfaceWrites<'_>,
    snapshot: &DeferredSnapshot,
) -> Result<Rc<TexRef>, String> {
    let materialized = materialize(
        ctx,
        pipelines,
        surfaces,
        writes,
        &snapshot.source,
        "copy source",
    )?;
    let out = surfaces.acquire(ctx, snapshot.out_w, snapshot.out_h, "copy snapshot")?;
    writes.remember(&out);
    // 区域未铺满目标表面时先清屏,保证区域外像素为透明。
    if snapshot.clip_w < snapshot.out_w || snapshot.clip_h < snapshot.out_h {
        let mut pass = PlannedPass {
            target: Some(PlannedTarget::Commands),
            ..PlannedPass::default()
        };
        pass.clear_targets.push(out.id);
        writes.passes.push(pass);
    }
    let (sx, sy) = if snapshot.full {
        (0, 0)
    } else {
        (snapshot.clip_x0 as u32, snapshot.clip_y0 as u32)
    };
    let width = (snapshot.x1 - snapshot.x0 + 1).max(0) as u32;
    let height = (snapshot.y1 - snapshot.y0 + 1).max(0) as u32;
    push_blit(
        writes,
        "copy",
        &materialized.tex,
        &out,
        (sx, sy),
        (0, 0),
        if snapshot.full { width } else { snapshot.clip_w },
        if snapshot.full { height } else { snapshot.clip_h },
    )?;
    surfaces.stats_mut().blits += 1;
    Ok(out)
}

/// 上一帧呈现面(`COPY ID=:/W0/`;EVID-J3-06)。内容未变时复用 GPU 纹理。
#[allow(clippy::too_many_arguments)]
fn backbuffer_content(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
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
        // 只在首帧兜底使用;像素由 `TexRef` 持有。
        gpu.remove(id, pipelines);
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
        .upload(ctx, buffer.width, buffer.height, &buffer.rgba, "backbuffer")
        .map_err(|diag| diag.code.to_string())?;
    // 后缓冲内容每变一次就换一张上传纹理;旧 id 必须从 GPU 缓存与 bind
    // group 里移除,否则逐帧变化的画面(对话/菜单淡入)会累积整屏纹理。
    if let Some((_, old_id)) = *backbuffer_gpu {
        gpu.remove(old_id, pipelines);
    }
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
/// 合成命令时刻的主窗口屏幕快照(`COPY2 ID=:/W0/`)。
///
/// 快照自带命令时刻的图层表/组 pass/屏幕 pass;这里把它包成一个临时
/// 离屏组合,走与普通组相同的 `compose_group_content` 路径(报告计数由
/// 该函数回滚)。
#[allow(clippy::too_many_arguments)]
fn screen_snapshot_content(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    layers: &HashMap<&LayerId, &Layer>,
    snapshot: &ScreenSnapshot,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    targets: &mut TargetSet,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    writes: &mut SurfaceWrites<'_>,
    text: &mut TextComposer,
    copy_id: yu_gfx::CopyId,
    tuning: yu_render_core::RenderTuning,
) -> Result<Rc<TexRef>, String> {
    let mut snap_frame = CompositorFrame::new(frame.frame, frame.viewport);
    snap_frame.dx_mode = frame.dx_mode;
    snap_frame.clear = frame.clear;
    snap_frame.color_space = frame.color_space;
    snap_frame.filter = frame.filter;
    snap_frame.image_probe_order = frame.image_probe_order.clone();
    snap_frame.layers = snapshot.layers.clone();
    snap_frame.passes = snapshot.passes.clone();
    snap_frame.text_draws = snapshot.text_draws.clone();
    snap_frame.text_revisions = snapshot.text_revisions.clone();
    snap_frame.text_layout = snapshot.text_layout.clone();
    let screen_ops = snapshot
        .passes
        .iter()
        .find(|pass| matches!(pass.target, PassTarget::Screen))
        .map(|pass| pass.ops.clone())
        .ok_or_else(|| "gfx.copy.screen_snapshot.pass_missing".to_string())?;
    // 合成目标用内部 ID;不会与脚本图层冲突(路径以 `__` 开头且只在
    // 本函数内使用)。
    // 合成组用内部 ID;若脚本恰好有同名层,追加下划线直到不撞名。
    let mut synth_name = format!("__screen_snapshot_{}", copy_id.0);
    while snapshot
        .layer_ids
        .iter()
        .any(|layer| layer.display() == synth_name)
        || frame
            .layers
            .iter()
            .any(|layer| layer.id.display() == synth_name)
    {
        synth_name.push('_');
    }
    let synth = LayerId::new(synth_name.into_bytes());
    snap_frame.passes.push(Pass {
        target: PassTarget::Offscreen(synth.clone()),
        ops: screen_ops,
        // `:/W0/` 拷贝的是窗口后缓冲,包含引擎默认清屏色(黑)。若用透明底,
        // 没有满屏图层的场景(选择支/黑背景)会得到 alpha=0 的缩略图,在存档
        // 界面显示成“没有背景”(2026-11-03 实测:黑屏场景缩略图 6970/7500
        // 像素 alpha=0;官方同场景 alpha=254/255)。软后端同处用
        // `frame.clear.rgba` 建屏幕面,语义一致。
        clear: Some(frame.clear),
    });
    // 快照里的文本要按**命令时刻**的 `text_draws` 重新烘焙:实时
    // `contents` 只含本帧最终状态;被弹窗盖住的背景按钮文字若在本帧
    // 被隐藏(`apply_text` 跳过),快照会只剩按钮底图(字体空白)。
    // 用独立的 contents/bases 副本,避免污染本帧的真实绘制。
    let mut snap_contents = contents.clone();
    // 快照层在同一帧里被 `CG[FILE]`/新 COPY 重新绑定后,`contents` 里已是
    // **新层**的像素;命令时刻旧层的像素不能再用它。纹理身份不一致的层
    // 一律退回快照自己记录的纹理/快照像素,禁止把新场景内容灌进旧快照
    // (2026-11-03 真机:旧 CGS 幕布闪出当前场景不存在的立绘)。
    for snap_layer in &snapshot.layers {
        if let Some(live) = layers.get(&snap_layer.id).copied() {
            if live.texture != snap_layer.texture {
                snap_contents.remove(&snap_layer.id);
            }
        }
    }
    let mut snap_bases = bases.clone();
    // 命令时刻的屏幕快照里,有些层本帧的图像加工排在截图命令之后/被
    // 本帧的系统命令作废(存档缩略图的 `MERGE2` 就是这种):`contents`
    // 没有它们,但 `surfaces` 里还留着上一帧的成品像素。快照必须用
    // 这份成品,否则缩略图会在淡出时先消失(只剩按钮底图)。
    for id in &snapshot.layer_ids {
        if snap_contents.contains_key(id) {
            continue;
        }
        // 同名层已换图/重建 ⇒ 常驻表面/旧内容属于新基面,不能补给旧快照。
        let Some(snap_layer) = snapshot.layers.iter().find(|layer| &layer.id == id) else {
            continue;
        };
        if layers
            .get(id)
            .is_some_and(|live| live.texture != snap_layer.texture)
        {
            continue;
        }
        if let Some(raw) = surfaces.raw_content(id) {
            snap_contents.insert(id.clone(), raw);
        } else if let Some(content) = surfaces.content(id) {
            snap_contents.insert(id.clone(), content.clone());
        }
    }
    // 快照合成必须使用**私有**的图层表面账:快照图层带的是捕获时刻的
    // alpha/texture,若写进活 `LayerSurfaces`,`sync_base` 会让同名活层
    // 在两套 base token 之间反复清空(读档丢背景的直接根因)。私有账按
    // `CopyId` 跨帧保留,快照自己的文本/加工缓存仍然生效。
    let mut snap_surfaces = targets.take_snapshot_surfaces(copy_id);
    {
        let mut local = Accum::new(&snap_frame);
        let mut uploads = TextUploads::default();
        let mut no_gpu: Option<crate::text_gpu::TextGpuRenderer> = None;
        let mut jobs = Vec::new();
        // `snap_frame` 自己的层表(与活帧不同:含快照时刻已删除的层)。
        let snap_layers: HashMap<&LayerId, &Layer> = snap_frame
            .layers
            .iter()
            .map(|layer| (&layer.id, layer.as_ref()))
            .collect();
        apply_text(
            ctx,
            &snap_frame,
            &snap_layers,
            textures,
            gpu,
            pipelines,
            &mut snap_surfaces,
            &mut snap_contents,
            &mut snap_bases,
            &mut uploads,
            text,
            &mut local,
            targets,
            &mut no_gpu,
            &mut jobs,
            tuning,
        );
        // 快照文本的纹理只服务本次合成;`TextUploads` 是临时表,
        // 不清理会把它上传的整层/整屏纹理永久留在 GPU 缓存。
        for id in uploads.clear() {
            gpu.remove(id, pipelines);
        }
    }
    let result = compose_group_content(
        ctx,
        &snap_frame,
        textures,
        gpu,
        pipelines,
        targets,
        &mut snap_surfaces,
        &mut snap_contents,
        &mut snap_bases,
        writes,
        &synth,
    );
    targets.put_snapshot_surfaces(copy_id, snap_surfaces);
    result
}

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
        // 本函数可能被传入 `snap_frame`(屏幕快照的临时帧),所以索引必须
        // 按**当前 `frame`** 建,不能借用外层的。
        let group_layers: HashMap<&LayerId, &Layer> =
            frame.layers.iter().map(|layer| (&layer.id, layer.as_ref())).collect();
        let mut planner = Planner {
            ctx,
            frame,
            layers: &group_layers,
            textures,
            gpu,
            pipelines,
            targets,
            vertices: writes.vertices,
            acc: &mut local,
            contents,
            bases,
            views: writes.views,
            transition_bind_groups: writes.transition,
            transition_buffers: writes.transition_buffers,
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
    /// 清空并返回被释放的纹理 id(调用方需要作废 GPU 缓存与 bind group)。
    pub fn clear(&mut self) -> Vec<u64> {
        let mut removed = Vec::new();
        removed.extend(self.layers.values().map(|(_, tex)| tex.id));
        removed.extend(self.screen.iter().map(|(_, tex)| tex.id));
        self.layers.clear();
        self.screen = None;
        removed
    }

    /// 只保留仍存活的层,返回被释放的纹理 id。
    pub fn retain(&mut self, live: &rustc_hash::FxHashSet<&LayerId>) -> Vec<u64> {
        let mut removed = Vec::new();
        self.layers.retain(|id, (_, tex)| {
            if live.contains(id) {
                true
            } else {
                removed.push(tex.id);
                false
            }
        });
        removed
    }

    /// 当前文本上传纹理 id(供帧末 bind group 存活集合使用)。
    pub fn live_texture_ids(&self, out: &mut BTreeSet<u64>) {
        out.extend(self.layers.values().map(|(_, tex)| tex.id));
        out.extend(self.screen.iter().map(|(_, tex)| tex.id));
    }
}

/// GPU 文本内容身份:修订 + 排版签名 + 基面像素 + 基面稳定身份。
fn text_gpu_identity(revision: u64, signature: u64, raw_hash: u64, base_key: u64) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, revision);
    fnv_mix(&mut hash, signature);
    fnv_mix(&mut hash, raw_hash);
    fnv_mix(&mut hash, base_key);
    hash
}

/// 文本绘制的身份(内容 + 排版 + 修订号):不变则不重烤、不重传。
fn text_signature(
    draws: &[&yu_gfx::TextDraw],
    layout: Option<&yu_gfx::TextLayoutFrame>,
    dims: (u32, u32),
) -> u64 {
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
        if let Some(image) = &draw.image {
            fnv_bytes(&mut hash, &image.asset);
            fnv_mix(&mut hash, u64::from(image.width));
            fnv_mix(&mut hash, u64::from(image.height));
            fnv_mix(&mut hash, image.tint.map_or(u64::MAX, u64::from));
        }
    }
    if let Some(layout) = layout {
        fnv_mix(&mut hash, layout.glyphs.len() as u64);
        fnv_mix(
            &mut hash,
            layout.reveal.map_or(u64::MAX, |value| value as u64),
        );
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
    let tex = resolve_ref(ctx, textures, gpu, targets, id, &layer.texture, None, bases).ok()?;
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

/// GPU 文本 pass 记账(逐帧)。
#[derive(Default, Clone, Copy)]
struct TextGpuStats {
    layers: u64,
    glyphs: u64,
    draws: u64,
    cached: u64,
    fallback: u64,
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
    layers: &HashMap<&LayerId, &Layer>,
    textures: &mut TextureStore,
    contents: &Contents,
    id: &LayerId,
    content: &Content,
) -> Result<(Texture, CpuBaseInfo), String> {
    let mut info = CpuBaseInfo::default();
    let mut texture = if !contents.contains_key(id) {
        let layer = layers.get(id).copied();
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
            Some(TextureRef::Movie { asset, frame }) => {
                let t = std::time::Instant::now();
                let texture = textures
                    .resolve_movie(asset, *frame)
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

/// 解析文本绘制项里的图像字形资产(外字图)。缺资产时留空,合成器按
/// `text.image_missing` 诊断,不猜。
fn text_images_for_draws(
    textures: &mut TextureStore,
    draws: &[&yu_gfx::TextDraw],
) -> BTreeMap<Vec<u8>, Texture> {
    let mut images = BTreeMap::new();
    for draw in draws {
        let Some(image) = &draw.image else {
            continue;
        };
        if images.contains_key(&image.asset) {
            continue;
        }
        if let Ok(texture) = textures.resolve(&image.asset) {
            images.insert(image.asset.clone(), texture.clone());
        }
    }
    images
}

/// 文本阶段:把 `CGACT[TEXT]`/`TextRun` 合成进图层常驻表面(W4)。
#[allow(clippy::too_many_arguments)]
fn apply_text(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    layers: &HashMap<&LayerId, &Layer>,
    textures: &mut TextureStore,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    surfaces: &mut LayerSurfaces,
    contents: &mut Contents,
    bases: &mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    text_uploads: &mut TextUploads,
    text: &mut TextComposer,
    acc: &mut Accum,
    targets: &TargetSet,
    text_gpu: &mut Option<crate::text_gpu::TextGpuRenderer>,
    text_jobs: &mut Vec<crate::text_gpu::TextGpuJob>,
    tuning: yu_render_core::RenderTuning,
) -> (u64, CpuBaseInfo, TextGpuStats) {
    let mut cpu_base_us = 0u64;
    let mut cpu_info = CpuBaseInfo::default();
    let mut gpu_stats = TextGpuStats::default();
    let mut ids: BTreeSet<LayerId> = frame.text_draws.iter().map(|d| d.target.clone()).collect();
    if let Some(layout) = &frame.text_layout {
        if let Some(target) = &layout.target {
            ids.insert(target.clone());
        }
    }
    // 文本绘制项按目标层分桶一次(#77):旧写法在每个文本层上
    // `text_draws.iter().filter(...)` 全扫(`文本层数 × draws 数`)。
    // 层内顺序 = `frame.text_draws` 命令序(分桶只做移动,不改序)。
    let mut draws_by_target: HashMap<&LayerId, Vec<&yu_gfx::TextDraw>> = HashMap::new();
    for draw in frame.text_draws.iter() {
        draws_by_target
            .entry(&draw.target)
            .or_default()
            .push(draw);
    }
    for id in ids {
        let Some(layer) = layers.get(&id).copied() else {
            note_text_diag(acc, format!("text.layer_missing: {}", id.display()));
            continue;
        };
        if !layer.visible {
            continue;
        }
        let draws: &[&yu_gfx::TextDraw] = draws_by_target
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let layout = frame
            .text_layout
            .as_ref()
            .filter(|l| l.target.as_ref() == Some(&id));
        let revision = frame.text_revisions.get(&id).copied().unwrap_or(0);
        let mut raw = match surfaces
            .raw_content(&id)
            .or_else(|| contents.get(&id).cloned())
        {
            Some(content) => content,
            None => match resolve_source(
                ctx,
                frame,
                layers,
                textures,
                gpu,
                targets,
                surfaces,
                contents,
                bases,
                &id,
                None,
                u64::MAX,
                "gfx.copy.source.gone",
            ) {
                Ok(content) => content,
                Err(reason) => {
                    note_text_diag(acc, format!("text.layer_base: {reason}"));
                    continue;
                }
            },
        };
        // 1×1 黑层(`CG[FILE=""]`)的文本基面是 `SX/SY` 大小的表面,
        // 而不是那 1 个像素;绘制阶段缓存的 1×1 content 在这里纠正。
        if matches!(layer.texture, yu_gfx::TextureRef::SolidBlack1x1) {
            if let Some(size) = layer.image_size() {
                if size.width > 0
                    && size.height > 0
                    && (raw.width != size.width || raw.height != size.height)
                {
                    raw.width = size.width;
                    raw.height = size.height;
                    raw.bilinear = false;
                }
            }
        }
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
        // 缓存键用“基面像素来源”身份(基面 token + 操作前缀),不用混了
        // 纹理池 id 的 `raw.hash`:同一基面换池后仍应命中。
        let base_key = surfaces.raw_identity(&id).unwrap_or(raw.hash);
        // GPU 文本:恒等基面 + 非 layout 时,字形直接 compute 写进层表面。
        //
        // 图像字形(外字)不能走这条路径:compute 只收字形,图像项会被静默
        // 跳过并把“无图”结果写进缓存。带图像项时一律走 CPU 合成。
        let has_image_glyph = draws.iter().any(|draw| draw.image.is_some());
        if let Some(gpu_text) = text_gpu.as_mut() {
            if !has_image_glyph && layout.is_none() && raw.is_identity() && !draws.is_empty() {
                let mut placements: Vec<yu_render_core::GlyphPlacement> = Vec::new();
                let mut collected = true;
                let mut gpu_glyphs = 0u32;
                let mut gpu_missing = 0usize;
                for draw in draws {
                    // 缺字形按 CPU 路径同口径跳过(只记 missing),不整体失败:
                    // 整层回退到 CPU 合成会同步读回整层像素(实测 25~30ms)。
                    match text.collect_glyphs_skipping_missing(draw) {
                        Ok((mut items, missing)) => {
                            gpu_missing = gpu_missing.saturating_add(missing);
                            let passes = draw.composite_passes().max(1) as usize;
                            gpu_glyphs = gpu_glyphs.saturating_add((items.len() / passes) as u32);
                            placements.append(&mut items);
                        }
                        Err(reason) => {
                            if tuning.text_gpu_debug {
                                log::warn!(
                                    target: "yu-render",
                                    "[text-gpu] collect failed layer={} err={reason} \
                                     bytes={} set={:?} bno={:?} font_no={:?}",
                                    id.display(),
                                    String::from_utf8_lossy(&draw.bytes),
                                    draw.set,
                                    draw.bno,
                                    draw.style.font_no
                                );
                            }
                            collected = false;
                            break;
                        }
                    }
                }
                if gpu_missing > 0 {
                    merge_text_stats(
                        acc,
                        yu_render_core::TextPaintStats {
                            missing: gpu_missing,
                            ..yu_render_core::TextPaintStats::default()
                        },
                    );
                }
                if collected && !placements.is_empty() {
                    let identity = text_gpu_identity(revision, signature, raw.hash, base_key);
                    if let Some(target) = gpu_text.cached(&id, identity) {
                        merge_text_stats(
                            acc,
                            yu_render_core::TextPaintStats {
                                draws: draws.len(),
                                glyphs: gpu_glyphs as usize,
                                ..yu_render_core::TextPaintStats::default()
                            },
                        );
                        gpu_stats.cached += 1;
                        let content = finish_content(
                            surfaces,
                            target,
                            dims.0,
                            dims.1,
                            raw.display,
                            Some(raw.bilinear),
                        );
                        surfaces.set_text_content(&id, content.clone(), raw.clone());
                        surfaces.note_text_baked(&id, revision, signature, raw.hash);
                        contents.insert(id.clone(), content);
                        continue;
                    }
                    match gpu_text.build(
                        ctx,
                        surfaces,
                        &id,
                        identity,
                        Rc::clone(&raw.tex),
                        &placements,
                    ) {
                        Ok((target, jobs)) => {
                            merge_text_stats(
                                acc,
                                yu_render_core::TextPaintStats {
                                    draws: draws.len(),
                                    glyphs: gpu_glyphs as usize,
                                    ..yu_render_core::TextPaintStats::default()
                                },
                            );
                            gpu_stats.layers += 1;
                            for job in jobs {
                                gpu_stats.glyphs += u64::from(job.glyphs);
                                gpu_stats.draws += u64::from(job.draw_count);
                                text_jobs.push(job);
                            }
                            let content = finish_content(
                                surfaces,
                                target,
                                dims.0,
                                dims.1,
                                raw.display,
                                Some(raw.bilinear),
                            );
                            surfaces.set_text_content(&id, content.clone(), raw.clone());
                            surfaces.note_text_baked(&id, revision, signature, raw.hash);
                            contents.insert(id.clone(), content);
                            continue;
                        }
                        Err(reason) => {
                            // 回退到 CPU 文本合成 = 一次**同步**整层读回
                            // (`content_cpu` → `read_back` → `read_now`),
                            // 实测可达 25~30ms/帧。这里把原因留痕:回退本身
                            // 是允许的(能力不足时必须回退),但静默回退会让
                            // 「GPU 文本没生效」无从归因。
                            if tuning.text_gpu_debug {
                                log::warn!(
                                    target: "yu-render",
                                    "[text-gpu] fallback layer={} reason={reason} \
                                     draws={} glyphs={} raw={}x{}",
                                    id.display(),
                                    draws.len(),
                                    gpu_glyphs,
                                    raw.width,
                                    raw.height
                                );
                            }
                            gpu_stats.fallback += 1;
                        }
                    }
                } else {
                    if tuning.text_gpu_debug {
                        let why = if has_image_glyph {
                            "image_glyph"
                        } else if layout.is_some() {
                            "layout"
                        } else if !raw.is_identity() {
                            "raw_not_identity"
                        } else if draws.is_empty() {
                            "no_draws"
                        } else if !collected {
                            "collect_failed"
                        } else {
                            "no_placements"
                        };
                        log::warn!(
                            target: "yu-render",
                            "[text-gpu] skip layer={} reason={why} \
                             draws={} glyphs={} raw={}x{}",
                            id.display(),
                            draws.len(),
                            gpu_glyphs,
                            raw.width,
                            raw.height
                        );
                    }
                    gpu_stats.fallback += 1;
                }
            }
        }
        let cpu_started = std::time::Instant::now();
        let cpu_base: Result<Rc<Texture>, String> = if let Some(hit) =
            text.cached_base(&id, base_key)
        {
            Ok(hit)
        } else if let Some(asset) =
            asset_identity_base(ctx, frame, textures, gpu, targets, bases, &id, &raw)
        {
            text.store_base(&id, base_key, Rc::clone(&asset));
            Ok(asset)
        } else {
            match content_cpu(ctx, layers, textures, contents, &id, &raw) {
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
        cpu_base_us = cpu_base_us
            .saturating_add(u64::try_from(cpu_started.elapsed().as_micros()).unwrap_or(u64::MAX));
        let images = text_images_for_draws(textures, &draws);
        // 缺图时不记 baked 签名:下一帧会重新合成,资产晚到时还能补上。
        let image_missing = draws.iter().any(|draw| {
            draw.image
                .as_ref()
                .is_some_and(|image| !images.contains_key(&image.asset))
        });
        let image_draws = draws.iter().filter(|draw| draw.image.is_some()).count();
        if image_draws > 0 {
            log::debug!(
                target: "yu-render",
                "[text-image] layer={} draws={} image_draws={} assets={}",
                id.display(),
                draws.len(),
                image_draws,
                images.len()
            );
        }
        let composed = text.compose_layer_cached_assets(
            &id,
            revision,
            raw.hash,
            dims,
            &draws,
            layout,
            &images,
            || cpu_base,
        );
        let (composed, stats) = match composed {
            Ok(value) => value,
            Err(reason) => {
                note_text_diag(acc, format!("text.layer_base: {reason}"));
                continue;
            }
        };
        if image_draws > 0 {
            log::debug!(
                target: "yu-render",
                "[text-image] composed={} draws={} glyphs={} missing={} diags={:?}",
                composed.is_some(),
                stats.draws,
                stats.glyphs,
                stats.missing,
                stats.diags
            );
        }
        merge_text_stats(acc, stats);
        let Some(composed) = composed else {
            if !image_missing {
                surfaces.note_text_baked(&id, revision, signature, raw.hash);
            }
            continue;
        };
        // 先取上传缓存条目的 owned 快照,`match` 期间不再借用 map,
        // 便于增量分支安全写回身份。
        let existing = text_uploads
            .layers
            .get(&id)
            .map(|(hash, tex)| (*hash, Rc::clone(tex)));
        let old_upload = existing.as_ref().map(|(_, tex)| tex.id);
        // `(使用中的 GPU 纹理, 需要写回的缓存身份)`。
        let (tex, cache_hash): (Rc<TexRef>, Option<u64>) = match existing {
            Some((hash, tex))
                if hash == composed.hash
                    && tex.width == composed.texture.width
                    && tex.height == composed.texture.height =>
            {
                (tex, None)
            }
            Some((hash, tex))
                if composed.dirty.is_some()
                    && composed.prev_hash == Some(hash)
                    && tex.width == composed.texture.width
                    && tex.height == composed.texture.height =>
            {
                // 逐字增量:只把新写到的像素矩形写进**现有的** GPU 纹理。
                // 旧实现每个字符都新建整层纹理、整张上传、再删除旧纹理
                // (推进剧情时的主要尖峰)。这里不新建/不删除纹理,
                // bind group 也不失效;像素等价性由合成器的脏区保证。
                let rect = composed.dirty.expect("dirty checked above");
                let stride = composed.texture.width as usize * 4;
                let offset = rect.y0 as usize * stride + rect.x0 as usize * 4;
                let width = rect.x1 - rect.x0;
                let height = rect.y1 - rect.y0;
                ctx.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: rect.x0,
                            y: rect.y0,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &composed.texture.rgba[offset..],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride as u32),
                        rows_per_image: Some(height),
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
                (tex, Some(composed.hash))
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
                let uploaded = gpu.get(id_upload).expect("uploaded text texture exists");
                let tex = Rc::new(TexRef {
                    id: id_upload,
                    width: uploaded.width,
                    height: uploaded.height,
                    texture: uploaded.texture.clone(),
                    view: uploaded.view.clone(),
                });
                // 整体重烤/换基面:换一张上传纹理;旧的必须从 GPU 缓存与
                // bind group 里移除,否则一局游戏会累积数千张整层纹理。
                if let Some(old_id) = old_upload {
                    gpu.remove(old_id, pipelines);
                }
                (tex, Some(composed.hash))
            }
        };
        if let Some(hash) = cache_hash {
            text_uploads
                .layers
                .insert(id.clone(), (hash, Rc::clone(&tex)));
        }
        let content = finish_content(surfaces, tex, dims.0, dims.1, raw.display, None);
        surfaces.set_text_content(&id, content.clone(), raw.clone());
        if !image_missing {
            surfaces.note_text_baked(&id, revision, signature, raw.hash);
        }
        contents.insert(id, content);
    }
    (cpu_base_us, cpu_info, gpu_stats)
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
            let old_upload = text_uploads.screen.as_ref().map(|(_, tex)| tex.id);
            let mut overlay = Texture {
                width,
                height,
                rgba: vec![0; (width as usize) * (height as usize) * 4],
            };
            let stats =
                text.paint_layout_with(&mut overlay.rgba, width, height, layout, Some(true));
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
            // 逐字显示时每次 reveal 都换一整张视口纹理;旧上传必须释放。
            if let Some(old_id) = old_upload {
                gpu.remove(old_id, pipelines);
            }
            text_uploads.screen = Some((signature, tex));
            id
        }
    };
    pipelines.ensure_pipeline(ctx, BlendMode::Normal, false)?;
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
    // 层不透明度参与签名:`CG[A=…]` 改它必须让前缀复用失效,否则合并结果
    // 会停在旧值(实测:同一 `<game>` 场景里改下层 A 后画面不更新)。
    fnv_mix(&mut hash, image.source_alpha as u16 as u64);
    fnv_mix(&mut hash, image.target_alpha as u16 as u64);
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

/// 该层的当前纹理是否直接来自一个**资产**(`TextureRef::Asset`)。
///
/// 只有这种情况能用 `TextureStore` 里那份 CPU 像素算 `TRIM` 边界;
/// 快照/离屏/影片的像素只存在于 GPU,必须回退读回(见
/// `readback::trim_bounds` 的四边窄带扫描)。
fn source_asset_key<'a>(frame: &'a CompositorFrame, id: &LayerId) -> Option<&'a [u8]> {
    let layer = frame.layers.iter().find(|layer| &layer.id == id)?;
    match &layer.texture {
        TextureRef::Asset(key) if !key.is_empty() => Some(key.as_slice()),
        _ => None,
    }
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
    layers: &HashMap<&LayerId, &Layer>,
    id: &LayerId,
    fallback: Option<&TextureRef>,
) -> u64 {
    base_token_of(frame, targets, id, layers.get(id).copied(), fallback)
}

/// [`base_token`] 的**免查找**版本:调用方已经持有该层的 `&Layer`。
///
/// `plan()` 开头逐层取 token 时用这个 —— 否则每次都要在 `frame.layers`
/// 里线性搜同一个 `LayerId`,退化成 O(层数²)(存档界面 541~1500 层,
/// 实测是 `plan` 阶段的主要成本)。
fn base_token_of(
    frame: &CompositorFrame,
    targets: &TargetSet,
    id: &LayerId,
    layer: Option<&yu_gfx::Layer>,
    fallback: Option<&TextureRef>,
) -> u64 {
    let _ = frame;
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_bytes(&mut hash, id.as_bytes());
    // 图层不透明度是基面身份的一部分:`CG[A=…]` 只改 alpha、不改纹理,若
    // 不入 token,`sync_base` 会认为基面没变而跳过快照 ⇒ 该层的像素动作
    // 全部停在旧 alpha 下算出的结果。
    fnv_mix(
        &mut hash,
        layer.map_or(256, |layer| layer.alpha as u16 as u64),
    );
    // 活层纹理与命令时刻不一致(删除后重建/换图)⇒ 以命令时刻的为准;
    // 否则沿用活层,让同帧加工结果(`chain_index`)继续链式生效。
    let live = layer
        .map(|layer| &layer.texture)
        .filter(|texture| !matches!(texture, TextureRef::None));
    let texture = match (live, fallback) {
        (Some(live), Some(fallback)) if *live != *fallback => Some(fallback),
        (Some(live), _) => Some(live),
        (None, fallback) => fallback,
    };
    match texture {
        Some(TextureRef::Movie { asset, frame }) => {
            fnv_mix(&mut hash, 6);
            fnv_bytes(&mut hash, asset);
            fnv_mix(&mut hash, u64::from(*frame));
        }
        Some(TextureRef::Asset(key)) => {
            fnv_mix(&mut hash, 1);
            fnv_bytes(&mut hash, key);
        }
        Some(TextureRef::SolidBlack1x1) => {
            fnv_mix(&mut hash, 2);
            if let Some(px) = layer.and_then(|layer| layer.fill) {
                fnv_mix(&mut hash, 7);
                for b in px {
                    fnv_mix(&mut hash, u64::from(b));
                }
            }
        }
        Some(TextureRef::Snapshot(copy_id)) => {
            fnv_mix(&mut hash, 3);
            fnv_mix(&mut hash, copy_id.0);
            // 零拷贝 UV 别名没有自己的纹理:身份取源纹理 id。缺了它,
            // `sync_base` 每帧都会把别名内容判成「基面变了」并丢掉
            // (屏幕上按钮整片消失,只在淡出合成里暴露)。
            let snapshot_tex = targets
                .snapshot(*copy_id)
                .map(|target| target.id)
                .or_else(|| {
                    targets
                        .snapshot_content(*copy_id)
                        .map(|content| content.tex.id)
                });
            fnv_mix(&mut hash, snapshot_tex.unwrap_or(0));
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
