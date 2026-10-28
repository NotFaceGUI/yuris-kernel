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

// 共享契约与平台设施(两个后端同源)。
use yu_render_core::{
    blit, text_base_key, AssetIssue, RenderPhases, RenderReport, SkippedOp, Surface, TextComposer,
    TextPaintStats, Texture, TextureStore,
};
// 仅剩的 CPU 像素算法依赖(**在册待清**,见 `docs/architecture/backends.md` §4):
// 这些动作目前在 GPU 后端里调用软后端实现,须逐个 GPU 化。
use yu_backend_soft::{
    apply_horizontal, apply_meido, apply_rect_paint, apply_resize, apply_reverse, apply_trim,
    apply_vertical,
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

/// COPY 目标表面的尺寸,按**命令序**预先定稿(见 [`plan`] 步骤 0)。
pub(crate) type SnapshotDims = BTreeMap<CopyId, (u32, u32)>;

/// 计划中的一个 pass 目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlannedTarget {
    Screen,
    Offscreen(LayerId),
    /// 图层级 GPU 合成目标(`CGACT[TRANSITION]`/`MERGE` 的结果面;按纹理 id)。
    Composite(u64),
    /// 直接写快照纹理(`MERGE` 原地写快照表面的官方语义)。
    Snapshot(CopyId),
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
    /// `CGACT[TRANSITION]` 双源整数 lerp:`lerp_bind_groups[bind_index]`,
    /// 无顶点缓冲(全屏三角形),整面写入目标。
    Lerp {
        bind_index: u32,
    },
    /// `CGACT[MERGE]` 双纹理整数合成:`merge_bind_groups[bind_index]`。
    Merge {
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

/// 快照上的一次矩形填充(RECTPAINT 族)或纹理合成(MERGE 族)。
#[derive(Clone, Debug)]
pub(crate) struct PaintCmd {
    pub target: CopyId,
    pub kind: PaintKind,
    /// 该绘制使用的 6 个顶点在 `FramePlan.vertices` 里的起点。
    pub first_vertex: u32,
}

#[derive(Clone, Debug)]
pub(crate) enum PaintKind {
    /// 纯色填充;`alpha_only` = 只写 alpha。`color` 是 `f32::to_bits`,
    /// 与 `PipelineCache::paint_groups` 的键一致。
    Solid {
        alpha_only: bool,
        color: [u32; 4],
    },
    /// `CGACT[MERGE/MERGE2]`:把上层表面按 `Normal` 混合画到目标表面。
    /// `write_alpha=false`(MERGE)只写 RGB,保留目标 alpha。
    Texture { source_id: u64, write_alpha: bool },
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
    /// 通用纹理 → 纹理复制(整面;`MERGE` 写回快照前把「下层」拷到临时槽)。
    pub blits: Vec<BlitCmd>,
}

/// 一次整面纹理复制:源是**执行期才存在**的快照(COPY 命令段创建),
/// 目标是普通纹理 id。
#[derive(Clone, Debug)]
pub(crate) struct BlitCmd {
    pub source: CopyId,
    pub dest_id: u64,
    pub width: u32,
    pub height: u32,
}

/// CPU 图像加工的细分计数(`YU_RENDER_TIMING` 门控)。
///
/// 用来回答「产品路径还剩多少 CPU 像素工作」:带区域 `SIZE` 与镜像已改成
/// 纯采样修饰(零像素),剩下的是 `RECTPAINT`(填充)与 `REVERSE`(取反)。
#[derive(Default)]
struct ImageOpStats {
    /// 采样修饰(`SIZE`/`SIZE2`/镜像):不碰像素。
    sampled: u64,
    /// `RECTPAINT` 族:CPU 填充(快照上的已推迟到 GPU)。
    paint: u64,
    paint_ms: f64,
    /// `REVERSE` 等逐像素变换。
    pixel: u64,
    pixel_ms: f64,
    /// `MERGE/MERGE2/XOR/AND/TRANSITION`:整面 CPU 合成(逐像素)。
    composite: u64,
    composite_ms: f64,
    /// 其中纯 `MERGE` / `MERGE2` 的条数(取证:哪些动作在吃 CPU)。
    merge: u64,
    merge2: u64,
    /// 双源过渡 / 位运算合成(同属整面 CPU 合成,单独计数便于归因)。
    transition: u64,
    bitwise: u64,
}

impl ImageOpStats {
    fn record(&mut self, action: &yu_gfx::op::ImageAction, elapsed: std::time::Duration) {
        let ms = elapsed.as_secs_f64() * 1e3;
        if action.is_scale() {
            self.sampled += 1;
        } else if matches!(
            action,
            yu_gfx::op::ImageAction::RectPaint
                | yu_gfx::op::ImageAction::RectPaint2
                | yu_gfx::op::ImageAction::RectPaint3
        ) {
            self.paint += 1;
            self.paint_ms += ms;
        } else if matches!(
            action,
            yu_gfx::op::ImageAction::Reverse
                | yu_gfx::op::ImageAction::Horizontal
                | yu_gfx::op::ImageAction::Vertical
        ) {
            self.pixel += 1;
            self.pixel_ms += ms;
        } else if action.is_layer_composite()
            || matches!(action, yu_gfx::op::ImageAction::Other("transition"))
        {
            self.composite += 1;
            self.composite_ms += ms;
            match action {
                yu_gfx::op::ImageAction::Merge => self.merge += 1,
                yu_gfx::op::ImageAction::Merge2 => self.merge2 += 1,
                yu_gfx::op::ImageAction::Other("transition") => self.transition += 1,
                yu_gfx::op::ImageAction::Other("xor" | "and") => self.bitwise += 1,
                _ => {}
            }
        }
    }
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
    /// `CGACT[TRANSITION]` GPU lerp 的 bind group(按 draw 索引)。
    pub lerp_bind_groups: Vec<wgpu::BindGroup>,
    /// `CGACT[MERGE]` GPU 合成的 bind group(按 draw 索引)。
    pub merge_bind_groups: Vec<wgpu::BindGroup>,
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
    image_cache: &mut BTreeMap<yu_render_core::ImageCacheKey, Override>,
    composite_cache: &mut BTreeMap<LayerId, (u64, Rc<Texture>)>,
    backbuffer: Option<&yu_render_core::Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    text: &mut TextComposer,
) -> Result<(FramePlan, RenderReport, RenderPhases), Diag> {
    targets.prune_snapshots(&live_snapshots(frame));
    {
        // 合成目标按「图层本帧是否存活」剪枝(否则跨场景累积 ⇒ 显存 OOM)。
        let live_layers: BTreeSet<LayerId> =
            frame.layers.iter().map(|layer| layer.id.clone()).collect();
        targets.prune_composites(&live_layers, pipelines);
        // 组离屏:存活集合 = 组 pass 目标 + 图层引用的 Offscreen。
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
    // 覆盖纹理缓存只保留本帧仍存在的层,防止临时层累积。
    uploaded_overrides.retain(|id, _| frame.layers.iter().any(|layer| &layer.id == id));
    let timing = std::env::var("YU_RENDER_TIMING").is_ok();
    // 几何留痕是**逐层**日志(长会话可达成百上千行/帧),与「每帧一行」
    // 的阶段耗时分开门控:性能采集只要 `YU_RENDER_TIMING`,做视觉对齐
    // 取证时才开 `YU_RENDER_TRACE_GEOMETRY`。
    let trace_geometry = std::env::var("YU_RENDER_TRACE_GEOMETRY").is_ok();
    let started = std::time::Instant::now();
    let t_snap;
    let t_image;
    let t_text;
    let t_copy_text;
    let mut acc = Accum::new(frame);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut passes: Vec<PlannedPass> = Vec::new();
    let mut transition_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
    let mut transition_buffers: Vec<wgpu::Buffer> = Vec::new();

    // 0. 快照表面尺寸,按**命令序**预先定稿。
    //
    // 官方 `FUN_0042f809` 在 COPY 命令执行时建目标表面、把区域尺寸写回层
    // 并且立即生效;所以「先 COPY、后 SIZE」的同一帧里,后面的加工读到的
    // 就是已经建好的表面尺寸。本内核把图像加工提前到命令段之前预执行,
    // 若那时才想从 GPU 目标反查尺寸,目标还不存在 ⇒
    // `gfx.imageop.source.snapshot_missing` ⇒ 整层跳一帧
    // (`es_SPExec.yst` 立绘归位:COPY 351 之后紧跟 SIZE2,实测那一帧立绘
    // 消失,2026-10-27)。
    //
    // 尺寸只由「目标层声明尺寸 / 复制区域」决定,与源像素无关,因此在
    // 命令序扫描里就能定稿;镜像捕获、命令段建纹理、后续加工统一引用这
    // 一份值,避免三处各算一遍。
    let mut snapshot_dims: SnapshotDims = BTreeMap::new();
    for pass in &frame.passes {
        for op in &pass.ops {
            let Op::Copy(copy) = op else { continue };
            if copy.status != OpStatus::Executable {
                continue;
            }
            if let Ok(dims) = copy_output_dims(frame, textures, targets, &snapshot_dims, copy) {
                snapshot_dims.insert(copy.id, dims);
            }
        }
    }

    // 1.4 快照 CPU 镜像预捕获(DIV-GFX-058,按命令序):1.5 的文本基面解析
    //     需要快照像素,而 GPU 快照在命令段才捕获;这里先把 CPU 可得源(资产/
    //     1×1 黑/另一快照镜像)按 soft `apply_copy_op` 的同一构造(目标尺寸 +
    //     区域裁剪)落成镜像。带冻结文本的 COPY 在 1.6 段连文本一起落。
    //
    // **迭代到不动点**:`es_SPExec.yst` 的立绘换图是链式快照
    // (`SPLBUFa601` → 快照 349 → `SPLBUF601` → 快照 351),后者的源是前者的
    // 镜像。单遍顺序扫描时若上游镜像在同遍更晚才建,下游就永远拿不到
    // (`gfx.imageop.source.snapshot_missing` ⇒ 该帧整层被 skip ⇒ 归位闪)。
    // 每轮只要有新增就再扫一遍;链长不会超过本帧 op 数,故上限取 op 数。
    let mark = started.elapsed();
    let copy_count = frame
        .passes
        .iter()
        .filter(|pass| matches!(pass.target, PassTarget::Texture(_)))
        .map(|pass| pass.ops.len())
        .sum::<usize>();
    for _ in 0..=copy_count {
        let mut progress = false;
        for pass in &frame.passes {
            let PassTarget::Texture(_) = &pass.target else {
                continue;
            };
            for op in &pass.ops {
                let Op::Copy(copy) = op else { continue };
                // 只按 **CPU 镜像**是否已存在判「已捕获」。
                //
                // 旧条件还要求 `targets.snapshot(copy.id).is_none()`(GPU 快照
                // 不存在),但 GPU 快照一旦冻结就长期存在,而 CPU 镜像会随
                // `prune_snapshots` 的存活集合变化被单独剪掉。两者用同一条件
                // 判断会让「GPU 快照在、镜像被剪」的组合永久缺镜像 ⇒
                // `gfx.imageop.source.snapshot_missing` ⇒ 立绘归位那帧画成
                // 纯黑(2026-10-27 实测:`SPLBUF601` 持 `Snapshot(351)`,
                // 351 的 GPU 快照在、镜像不在)。
                if copy.status != OpStatus::Executable
                    || !copy.texts.is_empty()
                    || targets.snapshot_cpu(copy.id).is_some()
                {
                    continue;
                }
                if capture_copy_mirror(frame, textures, targets, &snapshot_dims, copy) {
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
    t_snap = started.elapsed() - mark;

    // 0.5 合成链记忆化。
    //
    // 目标层本帧的「整面清屏 + 若干 MERGE」是一条**纯函数**链:输入(动作
    // 序列、几何、各源像素身份、源的文本修订号)逐项相同时,输出像素必然
    // 相同。官方逐帧重算,结果一致;这里直接复用上一帧的表面,跳过整条
    // 逐像素链。现场(β4 演出1 的对话窗):每帧 40 条 MERGE/MERGE2 全走
    // CPU `apply_merge`,800×600 × 40 ≈ 10.9ms/帧(`composite=40(10.87ms)`
    // 实测,占规划时间 12.7ms 的绝大部分)。
    //
    // 只对「链首是整面 RECTPAINT」的层生效:它把旧表面整块覆盖,结果才与
    // 上一帧内容无关。目标层是快照时不做(那里的 RECTPAINT 走 GPU 命令段)。
    let mut overrides: BTreeMap<LayerId, Override> = BTreeMap::new();
    let mut memo_skip: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut memo_last: BTreeMap<LayerId, (usize, usize)> = BTreeMap::new();
    let mut memo_sig: BTreeMap<LayerId, u64> = BTreeMap::new();
    // 合成链记账(取证):判定过的层链数 / 记忆化命中数。
    let mut memo_chains: u64 = 0;
    let mut memo_hits: u64 = 0;
    composite_cache.retain(|id, _| frame.layers.iter().any(|layer| &layer.id == id));
    {
        let mut chains: BTreeMap<LayerId, Vec<(&ImageOp, (usize, usize))>> = BTreeMap::new();
        for (pass_index, pass) in frame.passes.iter().enumerate() {
            let PassTarget::Texture(_) = &pass.target else {
                continue;
            };
            for (op_index, op) in pass.ops.iter().enumerate() {
                let Op::Image(image) = op else { continue };
                if op.status() != OpStatus::Executable {
                    continue;
                }
                let target = image.target.as_ref().unwrap_or(&image.source).clone();
                chains
                    .entry(target)
                    .or_default()
                    .push((image, (pass_index, op_index)));
            }
        }
        for (layer, ops) in chains {
            // 单条 MERGE 只 0.26ms,链太短不值得记。
            if ops.len() < 4 {
                continue;
            }
            // 链的起点是**层基图**:每帧的第一条 op 在 `apply_image_to_override`
            // 里取 `resolve_base_texture`(不是上一帧的结果),所以整条链是
            // 「基图 + op 列表 + 各源像素」的纯函数 —— 链首是否整面清屏并不
            // 影响这个结论,只要基图本身可寻址(资产/1×1 黑)。
            match frame
                .layers
                .iter()
                .find(|entry| entry.id == layer)
                .map(|entry| &entry.texture)
            {
                Some(TextureRef::Asset(_) | TextureRef::SolidBlack1x1) => {}
                _ => continue,
            }
            if !ops
                .iter()
                .all(|(op, _)| is_full_paint(op) || op.action.is_layer_composite())
            {
                continue;
            }
            if snapshot_of(frame, &layer).is_some() {
                continue;
            }
            let signature = chain_signature(frame, targets, &overrides, &ops);
            memo_chains += 1;
            match composite_cache.get(&layer) {
                Some((cached, texture)) if *cached == signature => {
                    memo_hits += 1;
                    // 命中:形状与像素都等价 ⇒ 直接沿用,链上每条都算已执行
                    // (与 soft 逐条重算的计数口径一致)。
                    overrides.insert(
                        layer.clone(),
                        Override::with_hash(texture.clone(), signature),
                    );
                    acc.ops_total += ops.len();
                    acc.ops_drawn += ops.len();
                    for (_, at) in &ops {
                        memo_skip.insert(*at);
                    }
                }
                _ => {
                    memo_sig.insert(layer.clone(), signature);
                    if let Some((_, at)) = ops.last() {
                        memo_last.insert(layer, *at);
                    }
                }
            }
        }
    }

    // 1. RECTPAINT/REVERSE/HORIZONTAL:CPU 预执行到图层覆盖纹理。
    //    目标是快照的 RECTPAINT 族推迟到命令段(GPU 直接在快照纹理上画)。
    // 文本合成产生的覆盖层(DIV-GFX-058):COPY 取源面时跳过(快照只带
    // 命令时刻冻结的 `copy.texts`)。
    let mut text_override_ids: BTreeSet<LayerId> = BTreeSet::new();
    // COPY 冻结文本的源面(1.6 段合成;plan_copy 直接上传为 GPU 源)。
    let mut copy_text_sources: BTreeMap<CopyId, Rc<Texture>> = BTreeMap::new();
    let mut deferred: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mark = started.elapsed();
    let mut img_stats = ImageOpStats::default();
    // GPU `CGACT[TRANSITION]`:bind group + 按 (pass, op) 排队的合成 pass
    // (pass 在下面的规划循环里按命令序插入,保证在依赖它的 COPY 之后、
    // 屏幕 pass 之前)。
    let mut lerp_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
    let mut gpu_transitions: BTreeMap<(usize, usize), Vec<PlannedPass>> = BTreeMap::new();
    // GPU `CGACT[MERGE]`:ping-pong 合成 pass(bind group 按 draw 索引)。
    let mut merge_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
    for (pass_index, pass) in frame.passes.iter().enumerate() {
        let PassTarget::Texture(target) = &pass.target else {
            continue;
        };
        acc.passes += 1;
        for (op_index, op) in pass.ops.iter().enumerate() {
            if memo_skip.contains(&(pass_index, op_index)) {
                continue;
            }
            match op {
                Op::Image(image) if op.status() == OpStatus::Executable => {
                    let image_target = image.target.as_ref().unwrap_or(&image.source);
                    // 快照目标上的 **RECTPAINT 族** 推迟到命令段:GPU 直接在
                    // 快照纹理上填色,不需要读回像素。
                    //
                    // 其它动作(尤其 `MERGE`:官方 `FUN_004405a4` 把两层的
                    // 表面缓冲逐像素合成)不能走 GPU 填充 —— 必须落在 CPU
                    // 路径上,基面取快照的冻结镜像(`resolve_base_texture` 的
                    // `Snapshot` 分支)。旧实现在这里把所有图像动作一律推迟,
                    // 而 `plan_paint` 只认 RECTPAINT/2/3 ⇒
                    // `es_SPExec.yst:341` 的
                    // `CGACT[ID="sp.tmpB" ID2=<快照层> MERGE=1]` 每帧被跳过
                    // (`gfx.imageop.set.missing`)⇒ 动画帧层只剩清空结果,
                    // 立绘眨眼/换表情时闪空白。
                    let is_paint = matches!(
                        image.action,
                        yu_gfx::op::ImageAction::RectPaint
                            | yu_gfx::op::ImageAction::RectPaint2
                            | yu_gfx::op::ImageAction::RectPaint3
                    );
                    if is_paint && snapshot_of(frame, image_target).is_some() {
                        deferred.insert((pass_index, op_index));
                        continue;
                    }
                    // `CGACT[TRANSITION]`:两源都是稳定 GPU 纹理、且目标层
                    // 本帧没有 CPU 像素消费者 ⇒ 在 GPU 上做整数 lerp
                    // (shader `lerp.wgsl`,与 soft `blend_transition` 逐位一致)。
                    if matches!(&image.action, yu_gfx::op::ImageAction::Other("transition")) {
                        let op_started = std::time::Instant::now();
                        if let Some(planned) = try_gpu_transition(
                            ctx,
                            frame,
                            textures,
                            targets,
                            gpu,
                            pipelines,
                            &mut overrides,
                            &mut lerp_bind_groups,
                            image_target,
                            image,
                            (pass_index, op_index),
                        ) {
                            acc.ops_total += 1;
                            acc.ops_drawn += 1;
                            img_stats.record(&image.action, op_started.elapsed());
                            gpu_transitions.insert((pass_index, op_index), vec![planned]);
                            continue;
                        }
                    }
                    // `CGACT[MERGE]`(非 MERGE2):上下层都是稳定 GPU 纹理
                    // ⇒ ping-pong 离屏 + 整数合成(shader `merge.wgsl`)。
                    if matches!(&image.action, yu_gfx::op::ImageAction::Merge) {
                        let op_started = std::time::Instant::now();
                        if let Some(planned) = try_gpu_merge(
                            ctx,
                            frame,
                            textures,
                            targets,
                            gpu,
                            pipelines,
                            &mut overrides,
                            &mut merge_bind_groups,
                            &snapshot_dims,
                            image_target,
                            image,
                            (pass_index, op_index),
                        ) {
                            acc.ops_total += 1;
                            acc.ops_drawn += 1;
                            img_stats.record(&image.action, op_started.elapsed());
                            gpu_transitions.insert((pass_index, op_index), planned);
                            continue;
                        }
                    }
                    acc.ops_total += 1;
                    let op_started = std::time::Instant::now();
                    let result = apply_image_to_override(
                        frame,
                        textures,
                        targets,
                        &snapshot_dims,
                        target,
                        image,
                        &mut overrides,
                        image_cache,
                    );
                    img_stats.record(&image.action, op_started.elapsed());
                    match result {
                        Ok(()) => acc.ops_drawn += 1,
                        Err(reason) => acc.skip(op, reason),
                    }
                    // 链尾:把结果记进记忆化缓存,下一帧同输入直接复用。
                    if memo_last.get(image_target) == Some(&(pass_index, op_index)) {
                        if let Some(signature) = memo_sig.get(image_target).copied() {
                            if let Some(Override::Cpu { tex, .. }) = overrides.get(image_target) {
                                composite_cache.insert(image_target.clone(), (signature, tex.clone()));
                            }
                        }
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
    t_image = started.elapsed() - mark;

    // 1.5 文本:图层基图(或已加工的覆盖纹理)副本上合成,再按图层
    //     `A/MODE/尺寸` 走常规 quad。与 soft 同一 `TextComposer`(字节/字体
    //     /栅格一致 ⇒ 同帧像素一致)。命中/增量路径不克隆基图、不重算整图
    //     哈希;新字符只增量画尾部(官方命令时刻写层表面语义,DIV-GFX-058)。
    let mark = started.elapsed();
    text.begin_frame_stats();
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
            let draws: Vec<&TextDraw> = frame
                .text_draws
                .iter()
                .filter(|d| d.target == id)
                .collect();
            let layout = frame
                .text_layout
                .as_ref()
                .filter(|l| l.target.as_ref() == Some(&id));
            let revision = frame.text_revisions.get(&id).copied().unwrap_or(0);
            let override_hash = overrides.get(&id).map(Override::hash);
            let base_key = text_base_key(&layer.texture, override_hash);
            // 基图惰性:命中/增量路径不解析不克隆基图(此前每帧整图复制)。
            let dims = match overrides.get(&id) {
                Some(hit) => Ok(hit.dims()),
                None => layer_base_dims(textures, targets, layer),
            };
            let composed = match dims {
                Ok(dims) => text.compose_layer_cached(
                    &id,
                    revision,
                    base_key,
                    dims,
                    &draws,
                    layout,
                    || {
                        match overrides.get(&id) {
                            // 文本合成要 CPU 像素;`Uv` 的像素属于源层、
                            // `Pass` 在显存,都不在这里展开(显式报缺)。
                            Some(hit) => hit.cpu_tex().cloned().ok_or_else(|| {
                                "text.base_is_not_cpu".to_string()
                            }),
                            None => layer_base_cpu(textures, targets, layer).map(Rc::new),
                        }
                    },
                ),
                Err(reason) => Err(reason),
            };
            match composed {
                Ok((composed, stats)) => {
                    merge_text_stats(&mut acc, stats);
                    if let Some(composed) = composed {
                        overrides
                            .insert(id.clone(), Override::with_hash(composed.texture, composed.hash));
                        text_override_ids.insert(id);
                    }
                }
                Err(reason) => {
                    note_text_diag(&mut acc, format!("text.layer_base: {reason}"));
                }
            }
        }
    }
    let text_stats = text.take_frame_stats();
    t_text = started.elapsed() - mark;

    // 1.6 COPY 冻结文本的源面(DIV-GFX-058):命令时刻已画文本进入快照,
    // 命令之后画的文本不得进入(与 soft 的 `compose_copy_texts` 同一语义;
    // 每帧只在快照未捕获时合成一次,冻结帧零成本)。
    let mark = started.elapsed();
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
                        Some(hit.cpu_tex().cloned().ok_or_else(|| {
                            "copy.base_is_not_cpu".to_string()
                        }))
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
    t_copy_text = started.elapsed() - mark;

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
                            trace_geometry,
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
                            trace_geometry,
                            transition_bind_groups: &mut transition_bind_groups,
                            transition_buffers: &mut transition_buffers,
                            dst_width: 0,
                            dst_height: 0,
                        };
                        planner.acc.ops_total += 1;
                        match planner.plan_copy(target, copy, &snapshot_dims, &mut commands) {
                            Ok(()) => planner.acc.ops_drawn += 1,
                            Err(reason) => planner.acc.skip(op, reason),
                        }
                    }
                    if !commands.copies.is_empty() {
                        passes.push(commands);
                    }
                }
                Op::Image(_) if gpu_transitions.contains_key(&(pass_index, op_index)) => {
                    if let Some(planned) = gpu_transitions.remove(&(pass_index, op_index)) {
                        passes.extend(planned);
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
                            trace_geometry,
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
                trace_geometry,
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
                trace_geometry,
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
                    push_quad(&mut vertices, width, height, 0.0, 0.0, width, height, 1.0);
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
    // 阶段耗时**无条件**采集(每帧 6 次 `Instant`,纳秒级):宿主
    // `--frame-stats` 的渲染子阶段列由它填充,不依赖 `YU_RENDER_TIMING`
    // 的日志门控。`YU_RENDER_TIMING` 只负责把同一组数打成一行人类可读
    // 的 `[rt]` 日志。
    let total = started.elapsed();
    let rest = total - t_snap - t_image - t_text - t_copy_text;
    let phases = RenderPhases {
        plan_us: total.as_micros() as u64,
        snap_us: t_snap.as_micros() as u64,
        image_us: t_image.as_micros() as u64,
        text_us: t_text.as_micros() as u64,
        copy_text_us: t_copy_text.as_micros() as u64,
        rest_us: rest.as_micros() as u64,
        text_bake_us: (text_stats.bake_ms * 1e3) as u64,
        text_hash_us: (text_stats.hash_ms * 1e3) as u64,
        text_paint_us: (text_stats.paint_ms * 1e3) as u64,
        text_base_us: (text_stats.base_ms * 1e3) as u64,
        image_paint_us: (img_stats.paint_ms * 1e3) as u64,
        image_pixel_us: (img_stats.pixel_ms * 1e3) as u64,
        image_composite_us: (img_stats.composite_ms * 1e3) as u64,
        decode_us: textures.decode_stats().0,
        decode_bytes: textures.decode_stats().1,
        image_merge_ops: img_stats.merge,
        image_merge2_ops: img_stats.merge2,
        image_transition_ops: img_stats.transition,
        image_bitwise_ops: img_stats.bitwise,
        image_paint_ops: img_stats.paint,
        image_pixel_ops: img_stats.pixel,
        image_sampled_ops: img_stats.sampled,
        memo_chains,
        memo_hits,
        ..RenderPhases::default()
    };
    if timing {
        log::trace!(
            target: "yu-render",
            "[rt] frame={} plan_total={:.2}ms snap={:.2} image={:.2} text={:.2} copy_text={:.2} rest={:.2} layers={} draws={} ops={}/{}",
            frame.frame,
            total.as_secs_f64() * 1e3,
            t_snap.as_secs_f64() * 1e3,
            t_image.as_secs_f64() * 1e3,
            t_text.as_secs_f64() * 1e3,
            t_copy_text.as_secs_f64() * 1e3,
            rest.as_secs_f64() * 1e3,
            frame.layers.len(),
            frame.text_draws.len(),
            acc.ops_drawn,
            acc.ops_total,
        );
        log::trace!(
            target: "yu-render",
            "[rt]   image_paths sampled={} paint={}({:.2}ms) pixel={}({:.2}ms) composite={}({:.2}ms)",
            img_stats.sampled,
            img_stats.paint,
            img_stats.paint_ms,
            img_stats.pixel,
            img_stats.pixel_ms,
            img_stats.composite,
            img_stats.composite_ms,
        );
        log::trace!(
            target: "yu-render",
            "[rt]   text_paths hits={} incr={} bakes={} bake={:.2}ms hash={:.2}ms paint={:.2}ms base={:.2}ms dims={:?}",
            text_stats.hits,
            text_stats.incremental,
            text_stats.full_bakes,
            text_stats.bake_ms,
            text_stats.hash_ms,
            text_stats.paint_ms,
            text_stats.base_ms,
            text_stats.max_dims,
        );
    }
    Ok((
        FramePlan {
            passes,
            vertices,
            transition_bind_groups,
            transition_buffers,
            lerp_bind_groups,
            merge_bind_groups,
        },
        acc.finish(missing_assets),
        phases,
    ))
}

/// `CGACT[TRANSITION]` 的 GPU 路径(整数 lerp,与 soft `blend_transition` 同式)。
///
/// 前置条件全部成立才返回计划,否则 `None` 由调用方落回 CPU:
/// 1. 两个源都能解析成**稳定 GPU 纹理**(资产/1×1/快照/离屏/本帧更早的
///    GPU 合成);CPU 覆盖或 `Uv` 采样修饰一律回退;
/// 2. 目标层本帧没有「需要 CPU 像素」的后续消费者(文本、MERGE、填充、
///    滤镜、作为上层源…),否则 GPU 结果拿不到 CPU 像素;
/// 3. 目标不是快照层(快照目标走命令段的 GPU 填充路径)。
#[allow(clippy::too_many_arguments)]
fn try_gpu_transition(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &mut TargetSet,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    overrides: &mut BTreeMap<LayerId, Override>,
    lerp_bind_groups: &mut Vec<wgpu::BindGroup>,
    target: &LayerId,
    image: &ImageOp,
    at: (usize, usize),
) -> Option<PlannedPass> {
    let debug = std::env::var_os("YU_TRANSITION_DEBUG").is_some();
    let bail = |why: &str| -> Option<PlannedPass> {
        if debug {
            log::info!(target: "yu-render", "[transition] CPU fallback: {why}");
        }
        None
    };
    let Some(to_id) = image.source2.as_ref() else {
        return bail("no ID2");
    };
    if snapshot_of(frame, target).is_some() {
        return bail("target is snapshot");
    }
    if frame.text_revisions.contains_key(target)
        || frame.text_draws.iter().any(|draw| &draw.target == target)
    {
        return bail("text on target");
    }
    if has_cpu_consumer_after(frame, target, at) {
        return bail("later CPU consumer");
    }
    let blend = image.set.as_ref().and_then(|value| value.as_int()).unwrap_or(0);
    let f = blend.clamp(0, 256) as u32;
    let Some((from_id, _fw, _fh)) = gpu_source(
        ctx,
        frame,
        textures,
        targets,
        gpu,
        overrides,
        &image.source,
        image.source_texture.as_ref(),
    ) else {
        return bail("from is CPU-only");
    };
    let Some((to_gpu, width, height)) =
        gpu_source(ctx, frame, textures, targets, gpu, overrides, to_id, None)
    else {
        return bail("to is CPU-only");
    };
    if width == 0 || height == 0 {
        return bail("zero size");
    }
    let target_gpu_id = match targets.ensure_composite(ctx, target, 0, width, height, pipelines) {
        Ok(target) => target.id,
        Err(_) => return bail("no composite target"),
    };
    let (Some(from_view), Some(to_view)) = (
        view_for_id(targets, gpu, from_id),
        view_for_id(targets, gpu, to_gpu),
    ) else {
        return bail("source view missing");
    };
    let from_view = from_view.clone();
    let to_view = to_view.clone();
    if pipelines.ensure_lerp_pipeline(ctx).is_err() {
        return bail("lerp pipeline");
    }
    let Ok(group) = pipelines.create_lerp_bind_group(ctx, &from_view, &to_view, f) else {
        return bail("bind group");
    };
    let bind_index = lerp_bind_groups.len() as u32;
    lerp_bind_groups.push(group);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, from_id);
    fnv_mix(&mut hash, to_gpu);
    fnv_mix(&mut hash, u64::from(f));
    overrides.insert(
        target.clone(),
        Override::Gpu {
            id: target_gpu_id,
            width,
            height,
            slot: 0,
            hash,
        },
    );
    Some(PlannedPass {
        target: Some(PlannedTarget::Composite(target_gpu_id)),
        draws: vec![Draw {
            blend: BlendMode::Normal,
            texture_id: target_gpu_id,
            filter: FilterKind::Point,
            kind: DrawKind::Lerp { bind_index },
        }],
        ..PlannedPass::default()
    })
}

/// 本帧在 `at` 之前是否有图像加工改写了该快照的表面。
///
/// 用于区分「快照表面 = COPY 时刻像素」与「表面已被同帧 `RECTPAINT` 族改写」:
/// 只有前者 GPU 的当前纹理与 CPU 参考路径的冻结像素同源,GPU 合成才可替换。
fn snapshot_modified_before(
    frame: &CompositorFrame,
    copy_id: CopyId,
    at: (usize, usize),
) -> bool {
    for (pass_index, pass) in frame.passes.iter().enumerate() {
        for (op_index, op) in pass.ops.iter().enumerate() {
            if (pass_index, op_index) == at {
                return false;
            }
            let Op::Image(image) = op else { continue };
            if op.status() != OpStatus::Executable {
                continue;
            }
            let target = image.target.as_ref().unwrap_or(&image.source);
            if snapshot_of(frame, target) == Some(copy_id) {
                return true;
            }
        }
    }
    false
}

/// 把图层当前像素解析成**稳定 GPU 纹理 id**(不产生 CPU 像素)。
///
/// `None` = 该源只能走 CPU(被加工成 CPU 覆盖 / 采样修饰 / 不存在)。
fn gpu_source(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    gpu: &mut TextureCache,
    overrides: &BTreeMap<LayerId, Override>,
    id: &LayerId,
    fallback: Option<&TextureRef>,
) -> Option<(u64, u32, u32)> {
    if let Some(ov) = overrides.get(id) {
        return match ov {
            Override::Gpu {
                id, width, height, ..
            } => Some((*id, *width, *height)),
            _ => None,
        };
    }
    let texture_ref = frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .map(|layer| &layer.texture)
        .filter(|texture| !matches!(texture, TextureRef::None))
        .or(fallback)?;
    match texture_ref {
        TextureRef::Asset(key) => {
            let texture = textures.resolve_shared(key).ok()?;
            let gpu_id = gpu
                .upload(ctx, texture.width, texture.height, &texture.rgba, "transition source")
                .ok()?;
            Some((gpu_id, texture.width, texture.height))
        }
        TextureRef::SolidBlack1x1 => {
            let texture = Texture::solid_black_1x1();
            let gpu_id = gpu
                .upload(ctx, texture.width, texture.height, &texture.rgba, "transition solid")
                .ok()?;
            Some((gpu_id, texture.width, texture.height))
        }
        TextureRef::Snapshot(copy_id) => targets
            .snapshot(*copy_id)
            .map(|target| (target.id, target.width, target.height)),
        TextureRef::Offscreen(group) => targets
            .offscreen(group)
            .map(|target| (target.id, target.width, target.height)),
        TextureRef::None => None,
    }
}

/// 纹理 id → view:离屏/快照/合成目标在 `TargetSet`,资产上传在 `TextureCache`。
fn view_for_id<'a>(
    targets: &'a TargetSet,
    gpu: &'a TextureCache,
    id: u64,
) -> Option<&'a wgpu::TextureView> {
    targets
        .view_by_id(id)
        .or_else(|| gpu.get(id).map(|texture| &texture.view))
}

/// `CGACT[MERGE]` 的 GPU 路径(整数合成,与 soft `apply_merge(…, false)` 同式)。
///
/// 与过渡同一套前置条件;额外要求下层(目标层当前像素)可解析成稳定 GPU
/// 纹理。链式 MERGE 用两槽 ping-pong(读上一槽、写下一槽),不会对同一纹理
/// 读写。`MERGE2`(带 `/ao` 除法)保持 CPU。
#[allow(clippy::too_many_arguments)]
fn try_gpu_merge(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &mut TargetSet,
    gpu: &mut TextureCache,
    pipelines: &mut PipelineCache,
    overrides: &mut BTreeMap<LayerId, Override>,
    merge_bind_groups: &mut Vec<wgpu::BindGroup>,
    snapshot_dims: &SnapshotDims,
    target: &LayerId,
    image: &ImageOp,
    at: (usize, usize),
) -> Option<Vec<PlannedPass>> {
    let debug = std::env::var_os("YU_TRANSITION_DEBUG").is_some();
    let bail = |why: &str| -> Option<Vec<PlannedPass>> {
        if debug {
            log::info!(target: "yu-render", "[merge] CPU fallback: {why}");
        }
        None
    };
    // 目标持快照 = 官方「原地写快照表面」形态,单独走下面的 blit + 写回分支。
    let snapshot_target = snapshot_of(frame, target);
    if frame.text_revisions.contains_key(target)
        || frame.text_draws.iter().any(|draw| &draw.target == target)
    {
        return bail("text on target");
    }
    if has_cpu_consumer_after(frame, target, at) {
        return bail("later CPU consumer");
    }
    let Some((upper, _uw, _uh)) = gpu_source(
        ctx,
        frame,
        textures,
        targets,
        gpu,
        overrides,
        &image.source,
        image.source_texture.as_ref(),
    ) else {
        return bail("upper is CPU-only");
    };
    // 快照目标:blit 快照 → 临时槽 → 整数 merge 写回快照纹理(不动 CPU 镜像)。
    if let Some(copy_id) = snapshot_target {
        // 快照 GPU 目标由更早的 COPY 命令在**命令段**创建/冻结(执行期),
        // 规划期只拿步骤 0 定稿的尺寸;blit 源按 `CopyId` 在执行期解析。
        let Some((width, height)) = snapshot_dims.get(&copy_id).copied() else {
            return bail("snapshot dims missing");
        };
        // 快照内容只能来自:①本帧的 COPY(命令段先执行) 或 ②上一帧已冻结的
        // GPU 纹理。两者都不满足 ⇒ 执行期读不到源,回退 CPU(禁止 fatal)。
        let will_exist = targets.snapshot(copy_id).is_some()
            || frame.passes.iter().any(|pass| {
                pass.ops.iter().any(|op| {
                    matches!(op, Op::Copy(copy) if copy.id == copy_id)
                })
            });
        if !will_exist {
            return bail("snapshot not materialized");
        }
        // 本帧这条 MERGE 之前,该快照表面是否已被图像加工改写
        // (`es_SPExec.yst` 的标准形态就是 `COPY → RECTPAINT3 SET=0 → MERGE`)。
        //
        // 这种形态下 GPU 的「下层」= **当前**快照纹理(已被 `RECTPAINT3` 清成
        // alpha=0、RGB=0),而 CPU 参考路径的下层 = `snapshot_pixels()` 给出的
        // **COPY 时刻冻结像素**。两者不是同一份数据:
        //
        // - `MERGE` 按现状只写 RGB、保留目标 alpha(`merge.wgsl`:`out.a = lower.a`);
        // - ⇒ GPU 结果是 alpha 恒 0 的表面。`SPLBUF602` 的 `SIZE` 再以它为源
        //   (`TextUri`/`Override::Uv` 解析源层时,GPU 路径没有 CPU 覆盖可复用),
        //   整层在 `SRCALPHA` 混合里被抹掉 ⇒ 立绘在换差分/眨眼的过渡帧消失,
        //   下一帧切回原路径又出现(`dir1-real` 实测 f=2651..2658、2894..2900)。
        //
        // 官方在同一表面上是**就地执行**(`FUN_0042f809`:`*(iVar4+0x90) = local_68`),
        // 清空后 MERGE 的结果必然可见 ⇒ 官方这次绘制会写 alpha,这正是
        // `docs/divergences/DIV-GFX-063` §2.4 记的未取证点。本轮不猜语义:
        // 只在该分歧**实际发生**时回退 CPU(基面 = 冻结像素,与回归前一致),
        // 未清空/未改写的快照仍走 GPU 合成。
        if snapshot_modified_before(frame, copy_id, at) {
            return bail("snapshot surface modified earlier in frame");
        }
        // 自合成(ID 与 ID2 指向同一快照):GPU 不能同纹理读写 ⇒ 回退 CPU。
        let upper_is_target = upper
            == targets
                .snapshot(copy_id)
                .map(|target| target.id)
                .unwrap_or(u64::MAX)
            || frame
                .layers
                .iter()
                .find(|layer| layer.id == image.source)
                .map(|layer| matches!(&layer.texture, TextureRef::Snapshot(id) if *id == copy_id))
                .unwrap_or(false);
        if upper_is_target {
            return bail("upper is the target snapshot");
        }
        if width == 0 || height == 0 {
            return bail("zero size");
        }
        let temp = match targets.ensure_composite(ctx, target, 2, width, height, pipelines) {
            Ok(target) => target.id,
            Err(_) => return bail("no temp target"),
        };
        let (Some(temp_view), Some(upper_view)) = (
            view_for_id(targets, gpu, temp),
            view_for_id(targets, gpu, upper),
        ) else {
            return bail("source view missing");
        };
        let temp_view = temp_view.clone();
        let upper_view = upper_view.clone();
        if pipelines.ensure_layer_merge_pipeline(ctx).is_err() {
            return bail("merge pipeline");
        }
        let Ok(group) = pipelines.create_layer_merge_bind_group(ctx, &temp_view, &upper_view) else {
            return bail("bind group");
        };
        let bind_index = merge_bind_groups.len() as u32;
        merge_bind_groups.push(group);
        // 快照内容即将改变:CPU 镜像失效(需要 CPU 像素的消费者已在扫描里回退)。
        targets.invalidate_snapshot_cpu(copy_id);
        return Some(vec![
            PlannedPass {
                target: Some(PlannedTarget::Commands),
                blits: vec![BlitCmd {
                    source: copy_id,
                    dest_id: temp,
                    width,
                    height,
                }],
                ..PlannedPass::default()
            },
            PlannedPass {
                target: Some(PlannedTarget::Snapshot(copy_id)),
                draws: vec![Draw {
                    blend: BlendMode::Normal,
                    texture_id: temp,
                    filter: FilterKind::Point,
                    kind: DrawKind::Merge { bind_index },
                }],
                ..PlannedPass::default()
            },
        ]);
    }
    // 下层 = 目标层当前像素(上一槽 / CPU 覆盖上传 / 层基图)。
    let (lower, prev_slot) = match overrides.get(target) {
        Some(Override::Gpu {
            id, width, height, slot, ..
        }) => ((*id, *width, *height), Some(*slot)),
        Some(Override::Cpu { tex, .. }) => {
            let gpu_id = gpu
                .upload(ctx, tex.width, tex.height, &tex.rgba, "merge lower")
                .ok()?;
            ((gpu_id, tex.width, tex.height), None)
        }
        Some(Override::Uv { .. }) => return bail("lower is Uv"),
        None => {
            let Some((id, width, height)) = gpu_base(ctx, frame, textures, targets, gpu, target)
            else {
                return bail("lower base is CPU-only");
            };
            ((id, width, height), None)
        }
    };
    let (lower_id, width, height) = lower;
    if width == 0 || height == 0 {
        return bail("zero size");
    }
    let next_slot = match prev_slot {
        Some(0) => 1,
        _ => 0,
    };
    let target_gpu_id = match targets.ensure_composite(
        ctx, target, next_slot, width, height, pipelines,
    ) {
        Ok(target) => target.id,
        Err(_) => return bail("no composite target"),
    };
    let (Some(lower_view), Some(upper_view)) = (
        view_for_id(targets, gpu, lower_id),
        view_for_id(targets, gpu, upper),
    ) else {
        return bail("source view missing");
    };
    let lower_view = lower_view.clone();
    let upper_view = upper_view.clone();
    if pipelines.ensure_layer_merge_pipeline(ctx).is_err() {
        return bail("merge pipeline");
    }
    let Ok(group) = pipelines.create_layer_merge_bind_group(ctx, &lower_view, &upper_view) else {
        return bail("bind group");
    };
    let bind_index = merge_bind_groups.len() as u32;
    merge_bind_groups.push(group);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(&mut hash, lower_id);
    fnv_mix(&mut hash, upper);
    fnv_mix(&mut hash, u64::from(next_slot));
    overrides.insert(
        target.clone(),
        Override::Gpu {
            id: target_gpu_id,
            width,
            height,
            slot: next_slot,
            hash,
        },
    );
    Some(vec![PlannedPass {
        target: Some(PlannedTarget::Composite(target_gpu_id)),
        draws: vec![Draw {
            blend: BlendMode::Normal,
            texture_id: target_gpu_id,
            filter: FilterKind::Point,
            kind: DrawKind::Merge { bind_index },
        }],
        ..PlannedPass::default()
    }])
}

/// 目标层的**基图**解析成 GPU 纹理(不看 `overrides`;供 MERGE 下层用)。
fn gpu_base(
    ctx: &GpuContext,
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    gpu: &mut TextureCache,
    target: &LayerId,
) -> Option<(u64, u32, u32)> {
    let layer = frame.layers.iter().find(|layer| &layer.id == target)?;
    match &layer.texture {
        TextureRef::Asset(key) => {
            let texture = textures.resolve_shared(key).ok()?;
            let gpu_id = gpu
                .upload(ctx, texture.width, texture.height, &texture.rgba, "merge base")
                .ok()?;
            Some((gpu_id, texture.width, texture.height))
        }
        TextureRef::SolidBlack1x1 => {
            let texture = Texture::solid_black_1x1();
            let gpu_id = gpu
                .upload(ctx, texture.width, texture.height, &texture.rgba, "merge solid")
                .ok()?;
            Some((gpu_id, texture.width, texture.height))
        }
        TextureRef::Snapshot(copy_id) => targets
            .snapshot(*copy_id)
            .map(|target| (target.id, target.width, target.height)),
        TextureRef::Offscreen(group) => targets
            .offscreen(group)
            .map(|target| (target.id, target.width, target.height)),
        TextureRef::None => None,
    }
}

/// 本帧在 `at` 之后是否还有需要**该层 CPU 像素**的加工/文本消费。
fn has_cpu_consumer_after(
    frame: &CompositorFrame,
    target: &LayerId,
    at: (usize, usize),
) -> bool {
    let mut later = false;
    for (pass_index, pass) in frame.passes.iter().enumerate() {
        for (op_index, op) in pass.ops.iter().enumerate() {
            if !later {
                if (pass_index, op_index) == at {
                    later = true;
                }
                continue;
            }
            let Op::Image(image) = op else { continue };
            if op.status() != OpStatus::Executable {
                continue;
            }
            let image_target = image.target.as_ref().unwrap_or(&image.source);
            // 这些动作不需要读回 CPU 像素:过渡/MERGE(可继续 GPU 链)、
            // SIZE/镜像(纯采样修饰)。
            let gpu_friendly = matches!(
                image.action,
                yu_gfx::op::ImageAction::Other("transition")
                    | yu_gfx::op::ImageAction::Merge
                    | yu_gfx::op::ImageAction::SizeNearest
                    | yu_gfx::op::ImageAction::SizeBilinear
                    | yu_gfx::op::ImageAction::Horizontal
                    | yu_gfx::op::ImageAction::Vertical
            );
            if gpu_friendly {
                continue;
            }
            if image_target == target
                || &image.source == target
                || image.source2.as_ref() == Some(target)
            {
                return true;
            }
        }
    }
    false
}

fn live_snapshots(frame: &CompositorFrame) -> BTreeSet<CopyId> {
    // 存活快照的**传递闭包**。
    //
    // 起点:①层当前引用的快照(`SPLBUF601` 持 `Snapshot(351)`);②本帧 COPY
    // 产出的快照;③COPY 命令时刻源纹理里的快照。
    //
    // 必须做闭包:快照可以由**另一快照**复制而来
    // (`es_SPExec.yst` 的 `SPLBUFa601` → `sp.tmpA` → `SPLBUF601`),而上游
    // 快照的产生 COPY 只在创建它的那一帧出现过。按「本帧是否被引用」判活会
    // 在下一帧把上游快照剪掉 ⇒ 下游镜像无法重建
    // (`gfx.imageop.source.snapshot_missing`)⇒ 立绘归位那帧画成纯黑。
    let mut live = BTreeSet::new();
    for layer in &frame.layers {
        if let TextureRef::Snapshot(id) = &layer.texture {
            live.insert(*id);
        }
    }
    let mut pending: Vec<CopyId> = Vec::new();
    for pass in &frame.passes {
        for op in &pass.ops {
            let Op::Copy(copy) = op else { continue };
            live.insert(copy.id);
            pending.push(copy.id);
            // 中间层被删除后,后续 copy 仍以它的快照为源
            // (`source_texture`);快照必须保留到消费完。
            if let Some(TextureRef::Snapshot(id)) = &copy.source_texture {
                if live.insert(*id) {
                    pending.push(*id);
                }
            }
        }
    }
    // 沿「快照 ← 产生它的 COPY 的源」回溯,直到不动点。
    while let Some(id) = pending.pop() {
        for pass in &frame.passes {
            for op in &pass.ops {
                let Op::Copy(copy) = op else { continue };
                if copy.id != id {
                    continue;
                }
                if let Some(TextureRef::Snapshot(src)) = &copy.source_texture {
                    if live.insert(*src) {
                        pending.push(*src);
                    }
                }
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
    snapshot_dims: &SnapshotDims,
    copy: &CopyOp,
) -> bool {
    // 源一律按 `Rc` 共享:旧实现每次 COPY 把整张源图 `clone()` 一遍
    // (1100×825/2048² 上每帧几十 MB),是 `snap_ms` 33ms 的主体。
    // 只覆盖 CPU 可得源(资产/1×1/快照/后缓冲镜像的链式展开)。
    //
    // 主组(`:/W0/`)复制**不在这里**造镜像:它的像素是上一帧呈现面,而
    // `SAVE[ID]` 需要的是「命令时刻该层表面」——软后端回放在同一帧里会
    // 继续应用后续 `CGACT` 加工,镜像拿不到这个时序。保留旧语义(抓帧器
    // 回放),不引入未验证的存档缩略图差异。
    let CopySource::Layer(source_layer) = &copy.source else {
        return false;
    };
    let source: Rc<Texture> = {
        let texture_ref = frame
            .layers
            .iter()
            .find(|layer| layer.id == *source_layer)
            .map(|layer| &layer.texture)
            .filter(|t| !matches!(t, TextureRef::None))
            .or(copy.source_texture.as_ref());
        let Some(texture_ref) = texture_ref else {
            return false;
        };
        match texture_ref {
            TextureRef::Asset(key) => match textures.resolve_shared(key) {
                Ok(texture) => texture,
                Err(_) => return false,
            },
            TextureRef::SolidBlack1x1 => Rc::new(Texture::solid_black_1x1()),
            TextureRef::Snapshot(pid) => match targets.snapshot_cpu_shared(*pid) {
                Some(texture) => texture,
                None => return false,
            },
            TextureRef::None | TextureRef::Offscreen(_) => return false,
        }
    };
    let (sw, sh) = (source.width, source.height);
    // 尺寸与命令段建出来的 GPU 快照**必须一致**,否则镜像与显存内容错位;
    // 两边都取步骤 0 按命令序定稿的同一份值。
    let (width, height) = snapshot_dims
        .get(&copy.id)
        .copied()
        .unwrap_or((sw, sh));
    let mut surface = Surface::new(width, height, [0, 0, 0, 0]);
    let (x0, y0, x1, y1) = copy.region.resolve(sw, sh);
    // 直接按 (宽, 高, 像素切片) 拷区域:旧实现先整帧 clone 一份源
    // (1100×825/2048² 每帧几十 MB)再逐像素 blit,是 `snap_ms` 33ms 的主体。
    yu_render_core::blit_rgba(
        source.width,
        source.height,
        &source.rgba,
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
    true
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

/// 只窥视图层基图尺寸(**免克隆**)。与 [`layer_base_cpu`] 同一分支序;
/// 合成缓存命中/增量路径用它做尺寸校验,不再为校验整图复制一遍。
fn layer_base_dims(
    textures: &mut TextureStore,
    targets: &TargetSet,
    layer: &Layer,
) -> Result<(u32, u32), String> {
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|t| (t.width, t.height))
            .map_err(|d| d.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok((1, 1)),
        TextureRef::None => layer
            .image_size()
            .map(|size| (size.width, size.height))
            .ok_or_else(|| "text.layer_size_unknown".to_string()),
        TextureRef::Snapshot(copy_id) => targets
            .snapshot_cpu(*copy_id)
            .map(|t| (t.width, t.height))
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

/// 帧内图像加工结果。
///
/// 按「有没有真的动像素」分两类:
///
/// - [`Override::Cpu`]:像素在 CPU(文本烘焙、逐像素滤镜)。指纹只用于跨帧
///   复用 GPU 上传;若每次绘制都重算,一条 800×600 的覆盖纹理就是 1.9 MB
///   的 FNV 遍历(title 期脚本每帧对 51 个层重发 `CGACT[SIZE]`,逐帧重算
///   ≈ 100 MB 哈希,EVID-J3-08 性能勘误)。
/// - [`Override::Uv`]:**没有动像素**,只是「从源的这块子矩形采样,铺到那个
///   尺寸」。`CGACT[SIZE]` 带源区域就是这种:立绘/精灵动画每帧换区域与目标
///   尺寸(实测 `(250,48)-(709,391)` 逐帧漂移),CPU `scale_region` 必然全
///   miss、单条 16–35 ms;绘制期把这个矩形当 UV 交给采样器,一次全屏 quad
///   即可,与区域是否变化无关。
#[derive(Clone)]
pub(crate) enum Override {
    Cpu {
        tex: Rc<Texture>,
        hash: u64,
    },
    /// 采样修饰:源层 + 归一化 UV 子矩形 + 输出尺寸。
    ///
    /// `width/height` 是**浮点**:引擎把目标尺寸按 16.16 定点保存
    /// (`ROUND(v*65536)`,见 `docs/render-parity.md` §7),立绘缩放每帧
    /// 只变零点几像素。若在这里取整,quad 尺寸就会「几帧不动、下一帧跳
    /// 1px」—— 与位置量化完全同类的阶梯抖动。
    Uv {
        source: LayerId,
        uv: [f32; 4],
        width: f64,
        height: f64,
        bilinear: bool,
        hash: u64,
    },
    /// 显存里的图层合成结果(目前用于 `CGACT[TRANSITION]`/`MERGE` 的 GPU 合成)。
    ///
    /// 像素只在 GPU 上,`cpu_tex()` = `None`:后续需要 CPU 像素的加工
    /// (`MERGE2`/`REVERSE`/文本基面…)必须先落回 CPU 路径 —— 规划阶段会
    /// 预扫描目标层,存在 CPU 消费者时**不走 GPU 分支**(显式回退,不猜)。
    /// `slot` = 该结果所在的 ping-pong 槽(`MERGE` 链读上一槽、写下一槽)。
    Gpu {
        id: u64,
        width: u32,
        height: u32,
        slot: u8,
        hash: u64,
    },
}

impl Override {
    fn new(tex: Rc<Texture>) -> Self {
        let hash = hash_rgba(tex.width, tex.height, &tex.rgba);
        Self::Cpu { tex, hash }
    }

    /// 用调用方已持有的内容指纹构造(文本合成缓存命中路径零重算:
    /// 指纹在烘焙/增量时算一次,存于 `yu_backend_soft::ComposedLayer::hash`)。
    fn with_hash(tex: Rc<Texture>, hash: u64) -> Self {
        Self::Cpu { tex, hash }
    }

    fn hash(&self) -> u64 {
        match self {
            Self::Cpu { hash, .. } | Self::Uv { hash, .. } | Self::Gpu { hash, .. } => *hash,
        }
    }

    /// 输出表面尺寸(取整;只用于「按目标尺寸反查源」这类整数口径)。
    fn dims(&self) -> (u32, u32) {
        match self {
            Self::Cpu { tex, .. } => (tex.width, tex.height),
            Self::Uv { width, height, .. } => {
                ((*width).round() as u32, (*height).round() as u32)
            }
            Self::Gpu { width, height, .. } => (*width, *height),
        }
    }

    /// 绘制尺寸(保留亚像素;`Uv` 的 quad 直接用它)。
    fn draw_dims(&self) -> (f64, f64) {
        match self {
            Self::Cpu { tex, .. } => (f64::from(tex.width), f64::from(tex.height)),
            Self::Uv { width, height, .. } => (*width, *height),
            Self::Gpu { width, height, .. } => (f64::from(*width), f64::from(*height)),
        }
    }

    /// CPU 像素视图。`Uv` 的像素属于源层、`Gpu` 的像素只在显存:返回
    /// `None`,需要 CPU 像素的调用方必须显式报缺(不静默补零)。
    fn cpu_tex(&self) -> Option<&Rc<Texture>> {
        match self {
            Self::Cpu { tex, .. } => Some(tex),
            Self::Uv { .. } | Self::Gpu { .. } => None,
        }
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
    backbuffer: Option<&'a yu_render_core::Surface>,
    /// `YU_RENDER_TIMING` 下逐层记录几何(位置/尺寸/UV)。
    trace_geometry: bool,
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
        // 取证开关(默认关):`YU_TRANS_DEBUG` 打一行实际取样参数,
        // `YU_TRANS_DUMP` 额外把 from/mask 两张 GPU 纹理落成 `.rgba`(800x600 等)。
        // 2026-10-27 用它在 FOUT 幕上确认:mask 走的是子区域+UV 覆盖,不是整面。
        if std::env::var("YU_TRANS_DEBUG").is_ok() {
            let describe = |id: &LayerId| match self.overrides.get(id) {
                Some(Override::Uv { uv, width: ow, height: oh, .. }) => {
                    format!("uv={uv:?} ow={ow} oh={oh}")
                }
                Some(_) => "override=other".to_string(),
                None => "override=none".to_string(),
            };
            let own = describe(&layer.id);
            let paired = transition
                .to_layer
                .as_ref()
                .map(|id| describe(id))
                .unwrap_or_default();
            eprintln!(
                "[trans] {} from={}x{} mask={}x{} layer={}x{} t={} ta={} {own} mask_{paired}",
                layer.id.display(), from.1, from.2, mask.1, mask.2, width, height,
                transition.t, transition.ta
            );
            if std::env::var("YU_TRANS_DUMP").is_ok() {
                let safe: String = layer
                    .id
                    .display()
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                for (tag, (id, w, h)) in [("from", from), ("mask", mask)] {
                    let Some(gpu) = self.gpu.get(id) else { continue };
                    let Ok(bytes) = crate::readback::dump_texture(self.ctx, &gpu.texture, w, h)
                    else {
                        continue;
                    };
                    let path =
                        format!("target/tmp/trans_f{}_{safe}_{tag}_{w}x{h}.rgba", self.frame.frame);
                    let _ = std::fs::write(&path, &bytes);
                    eprintln!("[trans] dumped {tag} -> {path}");
                }
            }
        }
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
        // 几何:官方 `FUN_00414ba4` 的网格循环覆盖**整个层矩形**(部件表
        // `+0x130` × 分块表 `+0xb0` 的累计坐标铺满层),所以过渡 quad 至少是
        // 层框大小;`SIZE`/`SIZE2` 修饰只决定「内容自己的尺寸」。
        //
        // 取逐分量 max:
        // - 立绘退出:内容 295x580 > 层框 295x501 ⇒ 用内容尺寸(否则被压矮);
        // - 演出 1 淡出:`size` 只把内容画进层表面的右下角(399x299 < 801x601)
        //   ⇒ 仍画满层框,否则那块矩形的边界会露出一条几何硬边
        //   (2026-10-27 实机:幕的左边是直的,与掩膜无关)。
        let (draw_w, draw_h, draw_uv) = match self.overrides.get(&layer.id) {
            Some(Override::Uv {
                uv,
                width: ow,
                height: oh,
                ..
            }) if variant == Variant::Fixed => (*ow, *oh, *uv),
            _ => (
                f64::from(width),
                f64::from(height),
                [0.0f32, 0.0, 1.0, 1.0],
            ),
        };
        let mask_uv = match transition.to_layer.as_ref().and_then(|id| self.overrides.get(id)) {
            Some(Override::Uv { uv, .. }) if variant == Variant::Fixed => *uv,
            _ => [0.0f32, 0.0, 1.0, 1.0],
        };
        let (draw_uv, mask_uv) = (clamp_uv_max(draw_uv), clamp_uv_max(mask_uv));
        let uniform_values: Vec<f32> = match variant {
            // 固定管线:UV 矩形随 uniform 下发,mask 在 shader 里反归一化回 [0,1]。
            // 第 0 槽是 `YS_FadeVal`(psC/psD 同源)。
            Variant::Fixed => vec![
                transition.fade as f32,
                if transition.complete() { 1.0 } else { 0.0 },
                // 官方「一格」柔带 `0.16/TA²`;与亚像素带 `1/K` 相加成单一公式
                // (见 `yu_gfx::transition_coverage`)。
                if transition.ta >= 1 {
                    let ta = transition.ta as f32;
                    0.16 / (ta * ta)
                } else {
                    0.0
                },
                0.0,
                draw_uv[0],
                draw_uv[1],
                draw_uv[2],
                draw_uv[3],
                mask_uv[0],
                mask_uv[1],
                mask_uv[2],
                mask_uv[3],
            ],
            // psC: YS_FadeVal, YS_FadeVal2(=TA), YS_AlphaVal(=A/256)
            Variant::Psc => vec![transition.fade as f32, f32::from(transition.ta), sa, 0.0],
            // psD: YS_FadeVal, YS_AlphaVal
            Variant::Psd => vec![transition.fade as f32, sa, 0.0, 0.0],
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
        if draw_w > 0.0 && draw_h > 0.0 {
            push_quad_uv(
                self.vertices,
                self.dst_width,
                self.dst_height,
                x as f64,
                y as f64,
                draw_w,
                draw_h,
                alpha,
                draw_uv,
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
        // 图层几何留痕(`YU_RENDER_TIMING` 门控):位置/尺寸是否逐帧连续变化,
        // 是判定「动画是否被整数化/是否错位」的第一手事实。
        if self.trace_geometry {
            log::trace!(
                target: "yu-render",
                "[geo] f={} layer={} xy=({:.4},{:.4}) explicit_size={:?} override={} alpha={}",
                self.frame.frame,
                layer.id.display(),
                x,
                y,
                explicit_size.map(|s| (s.width, s.height)),
                self.overrides
                    .get(&layer.id)
                    .map(|o| match o {
                        Override::Cpu { tex, .. } => format!("cpu {}x{}", tex.width, tex.height),
                        Override::Gpu {
                            width, height, ..
                        } => format!("gpu {width}x{height}"),
                        Override::Uv {
                            width,
                            height,
                            uv,
                            ..
                        } => format!(
                            "uv {}x{} [{:.3},{:.3},{:.3},{:.3}]",
                            width, height, uv[0], uv[1], uv[2], uv[3]
                        ),
                    })
                    .unwrap_or_else(|| "-".to_string()),
                alpha,
            );
        }

        if let Some(ov) = self.overrides.get(&layer.id) {
            // `Uv` 修饰:直接对源纹理做子矩形采样。缩放就是一次带 UV 的
            // quad,不重采样、不生成中间纹理。
            if let Override::Uv {
                source,
                uv,
                width: ow,
                height: oh,
                bilinear,
                ..
            } = ov
            {
                let (source_id, source_view, _sw, _sh) = match self.image_source_texture(source) {
                    Ok(value) => value,
                    Err(reason) => {
                        self.skip(op, reason);
                        return Ok(());
                    }
                };
                // 目标尺寸保留浮点(`dv.dims()`);层记录的 `image_size` 是
                // 取整值,只在没有 `Uv` 修饰时使用。
                let (width, height) = (*ow, *oh);
                let _ = explicit_size;
                // 显式指定了 point/线性(`SIZE` vs `SIZE2`)时以脚本为准;
                // 否则按源/目标尺寸关系自动选(与既有 draw 同口径)。
                // `SIZE2`/`SIZE3..5` 是双线性族;`SIZE` 是最近邻。两者都由
                // 采样器承担,和官方 D3D9 的 point/linear 采样状态一一对应。
                let filter = if *bilinear {
                    FilterKind::Linear
                } else {
                    FilterKind::Point
                };
                return self.draw_uv(
                    out,
                    blend,
                    source_id,
                    &source_view,
                    filter,
                    (x, y),
                    (width, height),
                    alpha,
                    DrawKind::Quad,
                    *uv,
                );
            }
            // GPU 合成结果(`CGACT[TRANSITION]` 的 lerp 输出):直接画纹理。
            if let Override::Gpu {
                id, width, height, ..
            } = ov
            {
                let Some(view) = view_for_id(self.targets, self.gpu, *id).cloned() else {
                    self.skip(op, "imageop.composite_missing");
                    return Ok(());
                };
                let (draw_width, draw_height) = match explicit_size {
                    Some(size) => (size.width, size.height),
                    None => (*width, *height),
                };
                let filter = pick_filter(self.frame, *width, *height, draw_width, draw_height);
                return self.draw(
                    out,
                    blend,
                    *id,
                    &view,
                    filter,
                    x as f64,
                    y as f64,
                    draw_width,
                    draw_height,
                    alpha,
                    DrawKind::Quad,
                );
            }
            let Some(tex) = ov.cpu_tex() else {
                self.skip(op, "imageop.override_missing");
                return Ok(());
            };
            let hash = ov.hash();
            // 1) 尺寸与内容都没变:直接复用;2) 同尺寸内容变化:原地重写
            // (每帧新建一张整尺寸纹理 ⇒ 显存泄漏/OOM);3) 其余:新上传。
            let cached = self.uploaded_overrides.get(&layer.id).copied();
            let same_dims = cached.is_some_and(|(_, w, h, _)| w == tex.width && h == tex.height);
            let gpu_tex = match cached {
                Some((id, w, h, cached_hash))
                    if same_dims && cached_hash == hash =>
                {
                    (id, w, h)
                }
                Some((id, _, _, _)) if same_dims => {
                    match self
                        .gpu
                        .rewrite(self.ctx, id, tex.width, tex.height, &tex.rgba)
                    {
                        Ok(true) => {
                            self.uploaded_overrides.insert(
                                layer.id.clone(),
                                (id, tex.width, tex.height, hash),
                            );
                            (id, tex.width, tex.height)
                        }
                        Ok(false) => match self.gpu.upload(
                            self.ctx,
                            tex.width,
                            tex.height,
                            &tex.rgba,
                            "imageop layer",
                        ) {
                            Ok(new_id) => {
                                self.uploaded_overrides.insert(
                                    layer.id.clone(),
                                    (new_id, tex.width, tex.height, hash),
                                );
                                (new_id, tex.width, tex.height)
                            }
                            Err(diag) => {
                                self.skip(
                                    op,
                                    format!("gpu.upload {}: {}", diag.code, diag.detail),
                                );
                                return Ok(());
                            }
                        },
                        Err(diag) => {
                            self.skip(
                                op,
                                format!("gpu.rewrite {}: {}", diag.code, diag.detail),
                            );
                            return Ok(());
                        }
                    }
                }
                _ => match self.gpu.upload(
                    self.ctx,
                    tex.width,
                    tex.height,
                    &tex.rgba,
                    "imageop layer",
                ) {
                    Ok(id) => {
                        self.uploaded_overrides.insert(
                            layer.id.clone(),
                            (id, tex.width, tex.height, hash),
                        );
                        (id, tex.width, tex.height)
                    }
                    Err(diag) => {
                        self.skip(op, format!("gpu.upload {}: {}", diag.code, diag.detail));
                        return Ok(());
                    }
                },
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
                x as f64,
                y as f64,
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
                            x as f64,
                            y as f64,
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
        snapshot_dims: &SnapshotDims,
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
                None => self.layer_image(id, copy.source_texture.as_ref())?,
            },
        };
        // 已捕获的快照**冻结**(EVID-T-12,与 soft 的 `snapshots.contains_key`
        // 早退同义):命令段每帧都会重放这条 CopyOp,若再推一次 GPU 复制,
        // 快照内容会跟着后缓冲滚动,CGS 盖屏/淡出的起点就错了
        // (EVID-J3-06:title 淡出等于没发生)。
        if self.targets.snapshot(copy.id).is_some() {
            return Ok(());
        }
        // 快照尺寸 = 步骤 0 按命令序定稿的**本次复制输出尺寸**(官方
        // `FUN_0042f809` 把区域尺寸写回层表面,同一帧后续加工立即读到)。
        // 定稿值与镜像捕获、后续加工共用一份,避免三处各算一遍。
        let (width, height) = snapshot_dims
            .get(&copy.id)
            .copied()
            .unwrap_or((source.1, source.2));
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
        // `MERGE/MERGE2`:把上层表面用 `Normal` 混合画到目标表面。
        //
        // 官方 `FUN_004405a4` 调通配绘制原语
        // `DAT_00801458(surface, …, X, Y, ratio, 0)` —— 一次 D3D9 带纹理
        // quad,不是逐像素循环;`X/Y` 是**绘制偏移**,不是源区域。
        // 之前在这里走 CPU `apply_merge`,每帧对整张快照做一次像素遍历
        // (立绘动画每帧一条,实测是 render 3ms 的主要成分)。
        if image.action.is_layer_composite() {
            let (source_id, source_view, source_w, source_h) = {
                let (id, view, w, h) = match self.image_source_texture(&image.source) {
                    Ok(value) => value,
                    Err(reason) => return Err(reason),
                };
                (id, view, w, h)
            };
            let _ = (source_w, source_h);
            self.pipelines
                .ensure_merge_pipeline(self.ctx, image.action == ImageAction::Merge2)
                .map_err(|diag| diag.code.to_string())?;
            self.pipelines
                .ensure_bind_group(self.ctx, source_id, &source_view, FilterKind::Linear)
                .map_err(|diag| diag.code.to_string())?;
            let (dx, dy) = (image.x.unwrap_or(0.0), image.y.unwrap_or(0.0));
            let first_vertex = self.vertices.len() as u32;
            push_quad(
                self.vertices,
                sw,
                sh,
                dx,
                dy,
                sw,
                sh,
                1.0,
            );
            out.paints.push(PaintCmd {
                target: copy_id,
                kind: PaintKind::Texture {
                    source_id,
                    write_alpha: image.action == ImageAction::Merge2,
                },
                first_vertex,
            });
            return Ok(());
        }
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
        // RECTPAINT 是整数像素矩形填充(在快照纹理上画),区域取整。
        // 与 `SIZE` 的亚像素采样区分:后者保留浮点走 UV。
        let (x0, y0, x1, y1) = match (image.x, image.y, image.x2, image.y2) {
            (Some(x), Some(y), Some(x2), Some(y2)) => (
                x.round() as i64,
                y.round() as i64,
                x2.round() as i64,
                y2.round() as i64,
            ),
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
            x0 as f64,
            y0 as f64,
            (x1 - x0 + 1) as u32,
            (y1 - y0 + 1) as u32,
            1.0,
        );
        out.paints.push(PaintCmd {
            target: copy_id,
            kind: PaintKind::Solid {
                alpha_only,
                color: color.map(f32::to_bits),
            },
            first_vertex,
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

    /// 某层的 GPU 纹理身份 + 视图 + 像素尺寸(UV 采样用)。
    ///
    /// 与 `layer_image` 的区别:这里返回**视图**而非只有 id —— 绘制期要
    /// 直接拿它建 bind group。源可以是资产/1×1/快照/已加工结果。
    fn image_source_texture(
        &mut self,
        source: &LayerId,
    ) -> Result<(u64, wgpu::TextureView, u32, u32), String> {
        if let Some(ov) = self.overrides.get(source) {
            match ov {
                Override::Uv { source: inner, .. } => {
                    let inner = inner.clone();
                    return self.image_source_texture(&inner);
                }
                // GPU 合成结果:纹理就在显存,直接返回(零拷贝)。
                Override::Gpu {
                    id, width, height, ..
                } => {
                    let view = self
                        .targets
                        .view_by_id(*id)
                        .cloned()
                        .or_else(|| self.gpu.get(*id).map(|t| t.view.clone()))
                        .ok_or_else(|| "imageop.source_gpu_missing".to_string())?;
                    return Ok((*id, view, *width, *height));
                }
                Override::Cpu { tex, .. } => {
                    let (w, h, hash) = (tex.width, tex.height, ov.hash());
                    let cached = self.uploaded_overrides.get(source).copied();
                    // 同尺寸:命中直接复用;内容变了就**原地重写**(新建会每帧
                    // 泄漏一张整尺寸纹理 ⇒ 显存 OOM)。
                    if let Some((gpu_id, uw, uh, cached_hash)) = cached {
                        if uw == w && uh == h {
                            if cached_hash != hash
                                && !self
                                    .gpu
                                    .rewrite(self.ctx, gpu_id, w, h, &tex.rgba)
                                    .map_err(|diag| diag.code.to_string())?
                            {
                                // 尺寸/内容不符(极少):落到新上传
                            } else {
                                self.uploaded_overrides
                                    .insert(source.clone(), (gpu_id, w, h, hash));
                                let view = self
                                    .gpu
                                    .get(gpu_id)
                                    .map(|t| t.view.clone())
                                    .ok_or_else(|| "imageop.source_gpu_missing".to_string())?;
                                return Ok((gpu_id, view, w, h));
                            }
                        }
                    }
                    let rgba = tex.rgba.clone();
                    let gpu_id = self
                        .gpu
                        .upload(self.ctx, w, h, &rgba, "imageop source")
                        .map_err(|diag| diag.code.to_string())?;
                    self.uploaded_overrides
                        .insert(source.clone(), (gpu_id, w, h, hash));
                    let view = self
                        .gpu
                        .get(gpu_id)
                        .map(|t| t.view.clone())
                        .ok_or_else(|| "imageop.source_gpu_missing".to_string())?;
                    return Ok((gpu_id, view, w, h));
                }
            }
        }
        // 源层可能已被 `CGEND` 删除;`Uv` 修饰只记了 LayerId,此时按
        // 该层在本帧的 `CopyOp.source_texture` / 加工产出找回表面。
        let fallback = self.source_fallback(source);
        let (id, w, h) = self.layer_image(source, fallback.as_ref())?;
        let view = view_by_id(self.gpu, self.targets, id)
            .cloned()
            .ok_or_else(|| "imageop.source_view_missing".to_string())?;
        Ok((id, view, w, h))
    }

    /// 层在本帧命令时刻的基图(源层被删后仍可解析像素)。
    fn source_fallback(&self, source: &LayerId) -> Option<TextureRef> {
        for pass in &self.frame.passes {
            for op in &pass.ops {
                match op {
                    Op::Copy(copy) => {
                        if copy.dest == *source {
                            if let Some(texture) = &copy.source_texture {
                                return Some(texture.clone());
                            }
                        }
                    }
                    Op::Image(image) => {
                        let target = image.target.as_ref().unwrap_or(&image.source);
                        if target == source {
                            if let Some(texture) = &image.source_texture {
                                return Some(texture.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// 图层源的当前图像。
    ///
    /// 源层可能已被 `CGEND` 删除(`DIV-GFX-053`):此时回退到 `CopyOp` 在命令
    /// 时刻记录的 `source_texture` 重放。
    fn layer_image(
        &mut self,
        id: &LayerId,
        fallback: Option<&TextureRef>,
    ) -> Result<(u64, u32, u32), String> {
        // GPU 合成结果(`CGACT[TRANSITION]`):像素已在显存,直接给纹理 id。
        if let Some(Override::Gpu {
            id: gpu_id,
            width,
            height,
            ..
        }) = self.overrides.get(id)
        {
            return Ok((*gpu_id, *width, *height));
        }
        // 文本覆盖纹理不作为 COPY 源(DIV-GFX-058):快照面 = 基图(+图像
        // 加工)+ 命令时刻冻结的 `copy.texts`,不带帧末实时文本。转场取样
        // (`plan_transition`)不受此限制 —— 引擎取样当前像素,含已画文本。
        if let Some(ov) = self
            .overrides
            .get(id)
            .filter(|_| !self.text_override_ids.contains(id))
        {
            // `Uv`/`Pass` 没有可直接拷贝的整图层纹理:COPY 需要的是「该层
            // 当前像素」这块矩形。`Uv` 只是采样修饰,源纹理就是它引用的源层;
            // `Pass` 是显存里的渲染结果。两者都按「源纹理 + 目标尺寸」交给
            // 调用方,由它把区域重映射到目标(见 `plan_copy`)。
            match ov {
                Override::Uv { source, .. } => {
                    let (id, _view, w, h) = self.image_source_texture(source)?;
                    return Ok((id, w, h));
                }
                // GPU 合成结果:显存纹理,直接给 COPY 用。
                Override::Gpu {
                    id, width, height, ..
                } => {
                    return Ok((*id, *width, *height));
                }
                Override::Cpu { .. } => {}
            }
            let Some(texture) = ov.cpu_tex() else {
                return Err("copy.source.override_missing".to_string());
            };
            let hash = ov.hash();
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
        let texture = match layer {
            Some(layer) => &layer.texture,
            None => fallback.ok_or_else(|| "gfx.copy.source.gone".to_string())?,
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
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        alpha: f32,
        kind: DrawKind,
    ) -> Result<(), Diag> {
        self.draw_uv(
            out,
            blend,
            texture_id,
            view,
            filter,
            (x, y),
            (f64::from(width), f64::from(height)),
            alpha,
            kind,
            [0.0, 0.0, 1.0, 1.0],
        )
    }

    /// 同一 draw,但 UV 取子矩形。`CGACT[SIZE]` 带源区域时,整个"缩放"就是
    /// 一次带 UV 的 quad —— 不需要 CPU 重采样,也不需要中间纹理。
    #[allow(clippy::too_many_arguments)]
    fn draw_uv(
        &mut self,
        out: &mut PlannedPass,
        blend: BlendMode,
        texture_id: u64,
        view: &wgpu::TextureView,
        filter: FilterKind,
        (x, y): (f64, f64),
        (width, height): (f64, f64),
        alpha: f32,
        kind: DrawKind,
        uv: [f32; 4],
    ) -> Result<(), Diag> {
        if !self.pipelines.ensure_pipeline(self.ctx, blend)? {
            // 调用方已用 `supported_blend` 过滤;走到这里说明计划器与管线
            // 能力表不一致,按环境错误上报而不是静默吞掉。
            return Err(Diag::fatal(code::TARGET, yu_log::Boundary::Platform)
                .with_detail(format!("没有为 blend {} 建立管线", blend.name())));
        }
        self.pipelines
            .ensure_bind_group(self.ctx, texture_id, view, filter)?;
        if width > 0.0 && height > 0.0 {
            push_quad_uv(
                self.vertices,
                self.dst_width,
                self.dst_height,
                x,
                y,
                width,
                height,
                alpha,
                uv,
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

/// 生成目标矩形的两个三角形,UV 取子矩形 `[u0, v0, u1, v1]`。
///
/// `CGACT[SIZE]` 的源区域是**闭区间像素范围** `(x0,y0)-(x1,y1)`,与 CPU
/// `scale_region` 的 `cw = x1-x0+1` 同口径,所以右下 UV 落在
/// `(x1+1)/w`;采样器中心对齐后等价于 `sx = x0 + dx*cw/dw`。
#[allow(clippy::too_many_arguments)]
fn push_quad_uv(
    vertices: &mut Vec<Vertex>,
    dst_width: u32,
    dst_height: u32,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    alpha: f32,
    uv: [f32; 4],
) {
    let target_w = f64::from(dst_width);
    let target_h = f64::from(dst_height);
    let left = 2.0 * x / target_w - 1.0;
    let right = 2.0 * (x + width) / target_w - 1.0;
    let top = 1.0 - 2.0 * y / target_h;
    let bottom = 1.0 - 2.0 * (y + height) / target_h;
    let (left, right) = (left as f32, right as f32);
    let (top, bottom) = (top as f32, bottom as f32);
    let [u0, v0, u1, v1] = uv;

    let tl = Vertex { position: [left, top], uv: [u0, v0], alpha };
    let tr = Vertex { position: [right, top], uv: [u1, v0], alpha };
    let bl = Vertex { position: [left, bottom], uv: [u0, v1], alpha };
    let br = Vertex { position: [right, bottom], uv: [u1, v1], alpha };
    vertices.extend_from_slice(&[tl, tr, bl, tr, br, bl]);
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
    x: f64,
    y: f64,
    width: u32,
    height: u32,
    alpha: f32,
) {
    let target_w = f64::from(dst_width);
    let target_h = f64::from(dst_height);
    let left = 2.0 * x / target_w - 1.0;
    let right = 2.0 * (x + f64::from(width)) / target_w - 1.0;
    let top = 1.0 - 2.0 * y / target_h;
    let bottom = 1.0 - 2.0 * (y + f64::from(height)) / target_h;
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


/// 源层的像素尺寸(用于把 `CGACT` 的像素区域归一化成 UV)。
///
/// 优先本帧加工结果(`Override` 已记录输出尺寸),否则按层纹理取。链式
/// `Uv` 只展开一层:语料里没有「区域缩放的结果再被区域缩放」。
fn image_source_dims(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    overrides: &BTreeMap<LayerId, Override>,
    source: &LayerId,
    fallback: Option<&TextureRef>,
    planned: Option<&SnapshotDims>,
) -> Result<(u32, u32), String> {
    if let Some(ov) = overrides.get(source) {
        return Ok(ov.dims());
    }
    // 快照尺寸优先取**命令序定稿**值:同一帧「先 COPY 后 SIZE」时 GPU
    // 目标还没建(要等命令段),只有这份值可用。
    let planned_dims = |id: &CopyId| {
        planned
            .and_then(|map| map.get(id).copied())
            .or_else(|| targets.snapshot(*id).map(|t| (t.width, t.height)))
    };
    let layer = frame.layers.iter().find(|layer| &layer.id == source);
    // 源层已被 `CGEND` 删除(`ImageOp::source_texture`):按命令时刻的基图
    // 取尺寸。语料 `es_Sprite.yst` 的动画帧每帧都是
    // `CG[SPLBUF…]` → `COPY` → `SIZE2` → `CGEND[SPLBUF…]`,SIZE 在该帧
    // 之后解析,源节点已经不在了。
    let (Some(layer), _) = (layer, ()) else {
        let texture = fallback.ok_or_else(|| "gfx.imageop.source.missing".to_string())?;
        return match texture {
            TextureRef::Asset(key) => textures
                .resolve(key)
                .map(|tex| (tex.width, tex.height))
                .map_err(|diag| diag.code.to_string()),
            TextureRef::SolidBlack1x1 => Ok((1, 1)),
            TextureRef::Snapshot(id) => planned_dims(id)
                .ok_or_else(|| "gfx.imageop.source.snapshot_missing".to_string()),
            _ => Err("gfx.imageop.source_size.unknown".to_string()),
        };
    };
    // 尺寸必须与**实际采样到的纹理**一致,否则 UV 归一化会错:
    // - `Asset`:真的解码一次(`TextureStore` 有缓存),拿真实像素尺寸。
    //   若只信 `layer.image_size()`,脚本对 `SPLBUF*` 不设 `SX/SY` 时这里
    //   会得到 0 ⇒ UV 全被 clamp 成一点 ⇒ 整层采成纯色(2026-10-26 实测)。
    // - `SolidBlack1x1`:1×1。
    // - `Snapshot`:用快照目标尺寸。
    // - 其余(Offscreen/None):退回层声明尺寸,拿不到就显式报缺。
    match &layer.texture {
        TextureRef::Asset(key) => textures
            .resolve(key)
            .map(|tex| (tex.width, tex.height))
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok((1, 1)),
        TextureRef::Snapshot(id) => {
            planned_dims(id).ok_or_else(|| "gfx.imageop.source.snapshot_missing".to_string())
        }
        _ => layer
            .image_size()
            .map(|s| (s.width, s.height))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .ok_or_else(|| "gfx.imageop.source_size.unknown".to_string()),
    }
}

/// UV 矩形上界夹到 1(闭区间区域换算出 801/800 这类越界值;越界取样在
/// ClampToEdge 下等价于边缘像素,夹取只是把「整面」还原成恒等 [0,1])。
fn clamp_uv_max(uv: [f32; 4]) -> [f32; 4] {
    [
        uv[0].min(1.0),
        uv[1].min(1.0),
        uv[2].min(1.0).max(uv[0].min(1.0)),
        uv[3].min(1.0).max(uv[1].min(1.0)),
    ]
}

/// 整面 `RECTPAINT` 族(无区域):把层表面整块覆盖。
fn is_full_paint(image: &ImageOp) -> bool {
    matches!(
        image.action,
        ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3
    ) && image.x.is_none()
        && image.y.is_none()
        && image.x2.is_none()
        && image.y2.is_none()
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

/// 合成链的**输入指纹**:动作序列 + 几何 + `SET` + 各源像素身份。
///
/// 这些逐项相同 ⇒ `apply_rect_paint`/`apply_merge` 的输出必然逐像素相同
/// (两者都是纯函数),因此可以跨帧复用结果。
fn chain_signature(
    frame: &CompositorFrame,
    targets: &TargetSet,
    overrides: &BTreeMap<LayerId, Override>,
    ops: &[(&ImageOp, (usize, usize))],
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    // 目标层的基图:链的起点(`resolve_base_texture`)。
    if let Some((first, _)) = ops.first() {
        let target = first.target.as_ref().unwrap_or(&first.source);
        if let Some(layer) = frame.layers.iter().find(|entry| &entry.id == target) {
            fnv_mix(&mut hash, texture_ref_token(targets, &layer.texture));
        }
    }
    for (image, _) in ops {
        fnv_bytes(&mut hash, image.action.name().as_bytes());
        for value in [image.x, image.y, image.x2, image.y2] {
            fnv_mix(&mut hash, value.map(f64::to_bits).unwrap_or(u64::MAX));
        }
        fnv_mix(
            &mut hash,
            image
                .set
                .as_ref()
                .and_then(|value| value.as_int())
                .map(|value| value as u64)
                .unwrap_or(u64::MAX),
        );
        fnv_mix(&mut hash, layer_token(frame, targets, overrides, &image.source, 0));
        if let Some(source2) = &image.source2 {
            fnv_mix(&mut hash, layer_token(frame, targets, overrides, source2, 0));
        }
    }
    hash
}

/// 源层**当前像素**的身份:文本修订号 + 覆盖纹理哈希 / 基图引用。
fn layer_token(
    frame: &CompositorFrame,
    targets: &TargetSet,
    overrides: &BTreeMap<LayerId, Override>,
    id: &LayerId,
    depth: usize,
) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fnv_mix(
        &mut hash,
        frame.text_revisions.get(id).copied().unwrap_or(0),
    );
    if depth >= 2 {
        return hash;
    }
    match overrides.get(id) {
        Some(Override::Cpu { tex: _, .. }) => {
            fnv_mix(&mut hash, 1);
            if let Some(override_) = overrides.get(id) {
                fnv_mix(&mut hash, override_.hash());
            }
        }
        Some(Override::Uv {
            source,
            uv,
            width,
            height,
            bilinear,
            hash: uv_hash,
        }) => {
            fnv_mix(&mut hash, 2);
            fnv_mix(&mut hash, *uv_hash);
            for value in uv {
                fnv_mix(&mut hash, u64::from(value.to_bits()));
            }
            fnv_mix(&mut hash, width.to_bits());
            fnv_mix(&mut hash, height.to_bits());
            fnv_mix(&mut hash, u64::from(*bilinear));
            fnv_mix(&mut hash, layer_token(frame, targets, overrides, source, depth + 1));
        }
        Some(Override::Gpu { id, hash: gpu_hash, .. }) => {
            // GPU 合成结果:身份 = 目标纹理 id + 输入指纹(跨帧稳定)。
            fnv_mix(&mut hash, 5);
            fnv_mix(&mut hash, *id);
            fnv_mix(&mut hash, *gpu_hash);
        }
        None => match frame.layers.iter().find(|layer| &layer.id == id) {
            Some(layer) => {
                fnv_mix(&mut hash, 3);
                fnv_mix(&mut hash, texture_ref_token(targets, &layer.texture));
            }
            None => fnv_mix(&mut hash, 4),
        },
    }
    hash
}

fn texture_ref_token(targets: &TargetSet, texture: &TextureRef) -> u64 {
    match texture {
        TextureRef::Asset(key) => {
            let mut hash = 0xcbf2_9ce4_8422_2325u64;
            fnv_bytes(&mut hash, key);
            hash
        }
        // 快照内容由 `CopyId` 冻结;纹理重建(尺寸变化)时 `RenderTarget.id`
        // 会变,所以把当前目标 id 也混进去。
        TextureRef::Snapshot(id) => {
            let target = targets.snapshot(*id).map(|target| target.id).unwrap_or(0);
            id.0.wrapping_mul(31).wrapping_add(target)
        }
        TextureRef::SolidBlack1x1 => 0x5bf0_0001,
        TextureRef::None => 0x5bf0_0002,
        TextureRef::Offscreen(_) => 0x5bf0_0003,
    }
}

/// 一次 COPY 的目标表面尺寸。
///
/// 官方 `FUN_0042f809`:`local_2c = (X2 - X) + 1`、`local_28 = (Y2 - Y) + 1`,
/// 末尾写回层的 `+0x78/+0x7c`;目标层若只有 `CG[ID=… FILE=""]` 的 1×1 占位,
/// 沿用它的 `SX/SY` 会把快照建成 1×1 ⇒ 之后每帧按 1×1 采样 ⇒ 整层画成单色
/// (`es_SPExec.yst` 的立绘缓冲层 `SPLBUF601` 就是这样)。
fn copy_output_dims(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    planned: &SnapshotDims,
    copy: &CopyOp,
) -> Result<(u32, u32), String> {
    let (sw, sh) = match &copy.source {
        CopySource::Layer(id) => image_source_dims(
            frame,
            textures,
            targets,
            &BTreeMap::new(),
            id,
            copy.source_texture.as_ref(),
            Some(planned),
        )?,
        CopySource::Group(id) => group_surface_size(frame, id),
    };
    let region_full = copy.region.resolve(sw, sh);
    let (rw, rh) = (
        (region_full.2 - region_full.0 + 1).max(0) as u32,
        (region_full.3 - region_full.1 + 1).max(0) as u32,
    );
    // 表面尺寸 = 源表面(有区域时 = 区域尺寸),**不看**目标层的显示尺寸:
    // 官方 COPY 替换目标表面,`CG`/`CGACT` 的尺寸只是绘制拉伸
    // (与 soft `apply_copy_op` 同口径;缩略图链 COPY 800×600 → SIZE2 100×75)。
    Ok(if rw > 0 && rh > 0 { (rw, rh) } else { (sw, sh) })
}

/// `CGACT` 源区域 → 归一化 UV 子矩形。
///
/// `region` 是**闭区间像素范围** `(x0,y0)-(x1,y1)`(与 CPU `scale_region`
/// 的 `cw = x1-x0+1` 同口径),所以右下 UV 落在 `(x1+1)/w`;采样器中心对齐
/// 后等价于 CPU 的 `sx = x0 + dx*cw/dw`。
fn region_uv(region: Option<(f64, f64, f64, f64)>, source_w: u32, source_h: u32) -> [f32; 4] {
    let (sw, sh) = (source_w.max(1) as f64, source_h.max(1) as f64);
    match region {
        // 区域是**闭区间像素范围**,右下 UV 落在 `(x1+1)/w`。
        //
        // 证据锚点:
        // - 闭区间口径 = `yu_backend_soft::imageop::scale_region` 的
        //   `cw = x1-x0+1`(CPU 参考实现,同一语义);
        // - 坐标为浮点 = `ERIS/Sprite/es_Sprite.yst` 的
        //   `CGACT[… SIZE2=1 FX=@X1*100+@FLX …]`(脚本注释原文:「@LEFT_X
        //   の小数点値を掃い、それを CGACT に伝える」),`FX/100` 即亚像素
        //   位置;取整会把慢速移动量化成阶梯(2026-10-26 实测);
        // - **不做 `[0,1]` clamp**:夹紧会把满幅区域 `(0,0,800,600)` 的
        //   右边界从 801/800 折成 1.0,图像被系统性压窄 1/800(10433 次
        //   采样里 7226 次触发),放大后是几个像素的「周围往里挤」。
        Some((x0, y0, x1, y1)) => {
            // **不要 clamp 到 [0,1]**:区域是闭区间,跨度是 `x1-x0+1` 个源像素
            // (与 CPU `scale_region` 的 `cw` 同口径)。满幅区域 `(0,0,800,600)`
            // 在 800px 源上右边界是 `801/800 = 1.00125`;夹到 1.0 就只采
            // 800px ⇒ 图像被系统性压窄 1/800,放大后是几个像素的"周围往里
            // 挤"(实测 10433 次采样里 7226 次触发)。保留真实比例后几何
            // 精确,越界部分由采样器边缘钳制承担。
            [
                (x0 / sw) as f32,
                (y0 / sh) as f32,
                ((x1 + 1.0) / sw) as f32,
                ((y1 + 1.0) / sh) as f32,
            ]
        }
        None => [0.0, 0.0, 1.0, 1.0],
    }
}

/// UV 缩放的内容身份(没有像素可哈希,用「源 + 区域 + 输出尺寸 + 滤波」)。
///
/// 只用于同一帧内脚本重复发同一条 `CGACT` 时判定「同一结果」,不参与
/// 跨帧像素缓存 —— UV 方案本身没有需要缓存的像素。
fn uv_scale_hash(source: &LayerId, image: &ImageOp, w: f64, h: f64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        hash ^= value;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    };
    for byte in source.as_bytes() {
        mix(u64::from(*byte));
    }
    for value in [image.x, image.y, image.x2, image.y2] {
        mix(value.map_or(u64::MAX, |v| v as u64));
    }
    mix(w.to_bits());
    mix(h.to_bits());
    mix(u64::from(image.action.is_bilinear_scale()));
    hash
}

fn apply_image_to_override(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    planned: &SnapshotDims,
    pass_target: &LayerId,
    image: &ImageOp,
    overrides: &mut BTreeMap<LayerId, Override>,
    image_cache: &mut BTreeMap<yu_render_core::ImageCacheKey, Override>,
) -> Result<(), String> {
    let target = image.target.as_ref().unwrap_or(&image.source);
    if target != pass_target {
        return Err("gfx.imageop.target.mismatch".to_string());
    }
    // 浮点区域(亚像素);只有整数像素动作(RECTPAINT/RESIZE)在各自分支取整。
    //
    // **并非所有动作都用「区域」**:官方 `FUN_004405a4`(MERGE/MERGE2/
    // XOR/AND)把 `X/Y` 当**目标偏移**用(`*DAT_00801458(… , X, Y, ratio, 0)`
    // 绘制偏移),不是源矩形;`SIZE` 用四个坐标做源子矩形;`REVERSE`/
    // `HORIZONTAL`/`VERTICAL`/`TRIM`/`MEIDO` 不接受区域。所以只有区域类
    // 动作才要求「四坐标齐全」,否则单给 X/Y 会误判成 partial 并整条跳过
    // (语料 `es_SPExec.yst:341` 的 `CGACT[… MERGE=1 X=@MGX Y=@MGY]`)。
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
    // 整数像素动作需要整数区域;`SIZE` 分支用浮点 `region` 走 UV。
    let int_region = region.map(|(x, y, x2, y2)| {
        (
            x.round() as i64,
            y.round() as i64,
            x2.round() as i64,
            y2.round() as i64,
        )
    });
    if image.action.is_scale() {
        let (Some(dw), Some(dh)) = (image.sx, image.sy) else {
            return Err("gfx.imageop.size.missing".to_string());
        };
        // 缩放尺寸是浮点(实测 `SY=825.5`);`<= 0` 才是「无尺寸」。
        if dw <= 0.0 || dh <= 0.0 {
            return Ok(());
        }
        // **不取整**:引擎的目标尺寸是 16.16 定点(`ROUND(v*65536)`),
        // 立绘缩放每帧只变零点几像素;取整会让 quad 尺寸「几帧不动、
        // 下一帧跳 1px」(与位置量化同类的阶梯)。零尺寸才跳过。
        if dw < 1.0 || dh < 1.0 {
            return Ok(());
        }
        // **不碰像素**:带区域缩放是「从源的这块矩形采样,铺到那个尺寸」,
        // 绘制期用 UV 子矩形交给采样器即可。立绘/精灵动画每帧换区域
        // (实测 `(250,48)-(709,391)` 逐帧漂移),CPU `scale_region` 必然
        // 全 miss 且单条 16–35 ms;UV 方案与区域是否变化无关。
        //
        // 源尺寸只用于把像素矩形归一化;源若本身也是 `Uv` 修饰,按链式展开
        // (复合区域在语料里不存在,展开深度由 `image_source_dims` 限一)。
        let (sw, sh) = image_source_dims(frame, textures, targets, overrides, &image.source, image.source_texture.as_ref(), Some(planned))?;
        let uv = region_uv(region, sw, sh);
        let hash = uv_scale_hash(&image.source, image, dw, dh);
        if std::env::var("YU_RENDER_TIMING").is_ok() {
            log::trace!(
                target: "yu-render",
                "[rt]   uv_scale src={} source_dims={}x{} region={:?} dst={}x{} uv=[{:.4},{:.4},{:.4},{:.4}]",
                image.source.display(),
                sw,
                sh,
                region,
                dw,
                dh,
                uv[0],
                uv[1],
                uv[2],
                uv[3]
            );
        }
        overrides.insert(
            target.clone(),
            Override::Uv {
                source: image.source.clone(),
                uv,
                width: dw,
                height: dh,
                bilinear: image.action.is_bilinear_scale(),
                hash,
            },
        );
        return Ok(());
    }
    // `HORIZONTAL`/`VERTICAL`:镜像 = UV 负步长,**纯采样**,不碰像素。
    //
    // 与 `SIZE` 一样表达成 `Override::Uv`,只是 UV 矩形左右/上下对调:
    // 水平镜像把 `[u0,u1]` 换成 `[u1,u0]`,垂直把 `[v0,v1]` 换成 `[v1,v0]`。
    // 采样器按点/线性插值,与原像素完全一致(镜像没有插值误差)。
    if matches!(
        image.action,
        ImageAction::Horizontal | ImageAction::Vertical
    ) {
        let (sw, sh) = image_source_dims(frame, textures, targets, overrides, &image.source, image.source_texture.as_ref(), Some(planned))?;
        let (w, h) = match overrides.get(&image.source) {
            Some(ov) => ov.draw_dims(),
            None => (f64::from(sw), f64::from(sh)),
        };
        let mut uv = [0.0f32, 0.0, 1.0, 1.0];
        // 源若是 UV 修饰(区域缩放结果再镜像),沿用其子矩形再翻转。
        if let Some(Override::Uv { uv: inner, .. }) = overrides.get(&image.source) {
            uv = *inner;
        }
        match image.action {
            ImageAction::Horizontal => uv.swap(0, 2),
            ImageAction::Vertical => uv.swap(1, 3),
            _ => {}
        }
        overrides.insert(
            target.clone(),
            Override::Uv {
                source: image.source.clone(),
                uv,
                width: w,
                height: h,
                bilinear: false,
                hash: uv_scale_hash(&image.source, image, w, h),
            },
        );
        return Ok(());
    }
    // `REVERSE`:RGB 逐通道取反,是真正的像素运算(需 shader)。在 GPU 版
    // 反转着色器落地前保持 CPU 路径,并要求源是 CPU 面(显式报缺,不读回)。
    if image.action == ImageAction::Reverse {
        if let Some(src) = overrides.get(&image.source) {
            let Some(src_tex) = src.cpu_tex() else {
                return Err("gfx.imageop.flip.needs_cpu_source".to_string());
            };
            let mut texture = (**src_tex).clone();
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
                ImageAction::Reverse => yu_render_core::ImageOpSig::Reverse,
                ImageAction::Vertical => yu_render_core::ImageOpSig::Vertical,
                _ => yu_render_core::ImageOpSig::Horizontal,
            };
            let source_id = match &layer.texture {
                TextureRef::Asset(key) => Some(yu_render_core::ImageSource::Asset(key.clone())),
                TextureRef::SolidBlack1x1 => Some(yu_render_core::ImageSource::Solid),
                TextureRef::Snapshot(id) => Some(yu_render_core::ImageSource::Snapshot(id.0)),
                _ => None,
            };
            let key = source_id.map(|source| yu_render_core::ImageCacheKey {
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
            let mut texture = (*resolve_base_texture(frame, textures, targets, target)?).clone();
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
        let from = resolve_cpu_source_with(
            frame,
            textures,
            targets,
            overrides,
            &image.source,
            image.source_texture.as_ref(),
            0,
        )?;
        let to = resolve_cpu_source(frame, textures, targets, overrides, to_id, 0)?;
        let blend = image.set.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
        let blended = yu_backend_soft::blend_transition(&from, &to, blend);
        overrides.insert(target.clone(), Override::new(Rc::new(blended)));
        return Ok(());
    }
    // `MERGE/MERGE2/XOR/AND`:`ID`(上层)合成到 `ID2`(下层),结果写 `ID2`。
    if image.action.is_layer_composite() {
        let mut lower = match overrides.remove(target) {
            Some(Override::Cpu { tex, .. }) => {
                Rc::try_unwrap(tex).unwrap_or_else(|rc| (*rc).clone())
            }
            Some(_) => return Err("gfx.imageop.needs_cpu_target".to_string()),
            None => (*resolve_base_texture(frame, textures, targets, target)?).clone(),
        };
        let upper = resolve_cpu_source_with(
            frame,
            textures,
            targets,
            overrides,
            &image.source,
            image.source_texture.as_ref(),
            0,
        )?;
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
        Some(Override::Cpu { tex, .. }) => {
            Rc::try_unwrap(tex).unwrap_or_else(|rc| (*rc).clone())
        }
        Some(_) => return Err("gfx.imageop.needs_cpu_target".to_string()),
        None => (*resolve_base_texture(frame, textures, targets, target)?).clone(),
    };
    match image.action {
        ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3 => {
            let Some(set) = image.set.as_ref().and_then(|v| v.as_int()) else {
                return Err("gfx.imageop.set.missing".to_string());
            };
            apply_rect_paint(&mut texture, image.action.clone(), set, int_region)?;
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
            // RESIZE 的四边扩缩量是整数像素。
            let (x, y, x2, y2) = (
                image.x.unwrap_or(0.0).round() as i64,
                image.y.unwrap_or(0.0).round() as i64,
                image.x2.unwrap_or(0.0).round() as i64,
                image.y2.unwrap_or(0.0).round() as i64,
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
    resolve_cpu_source_with(frame, textures, targets, overrides, id, None, depth)
}

/// 同 [`resolve_cpu_source`],但允许在层已被 `CGEND` 删除时用调用方记录的
/// 命令时刻基图(`ImageOp::source_texture`)取像素。
fn resolve_cpu_source_with(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    overrides: &BTreeMap<LayerId, Override>,
    id: &LayerId,
    fallback: Option<&TextureRef>,
    depth: usize,
) -> Result<Rc<Texture>, String> {
    if depth > 4 {
        return Err("gfx.imageop.source.depth".to_string());
    }
    if let Some(ov) = overrides.get(id) {
        return ov
            .cpu_tex()
            .cloned()
            .ok_or_else(|| "gfx.imageop.source_not_cpu".to_string());
    }
    let layer = frame.layers.iter().find(|layer| &layer.id == id);
    let texture = match layer {
        Some(layer) => &layer.texture,
        None => match fallback {
            Some(texture) => texture,
            None => return Err("gfx.imageop.source.missing".to_string()),
        },
    };
    match texture {
        // 共享缓存里的 `Rc`(零拷贝):旧实现每解析一次资产就整图
        // `rgba.clone()`,一条 `MERGE` 链的上层源每帧被复制几十次。
        TextureRef::Asset(key) => textures
            .resolve_shared(key)
            .map_err(|diag| diag.code.to_string()),
        TextureRef::SolidBlack1x1 => Ok(Rc::new(Texture::solid_black_1x1())),
        TextureRef::Snapshot(copy_id) => {
            // 冻结镜像优先:它由 1.4/1.6 段在**命令时刻**捕获,源层之后
            // 被 `CGEND` 删除也不影响(与 soft `snapshots` 表同一语义)。
            if let Some(mirror) = targets.snapshot_cpu_shared(*copy_id) {
                return Ok(mirror);
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

/// 快照当前像素的 CPU 视图。
///
/// 优先用 1.4/1.6 段在**命令时刻**冻结的镜像;镜像不在(本帧刚产生、且没走
/// 预捕获路径)时,按产生它的 `CopyOp` 在 CPU 上重放一次 —— 与官方
/// 「复制即写目标表面」(`FUN_0042f809`)同义,也与 soft 的 `apply_copy_op`
/// 同一构造。仍不可得返回 `None`(调用方显式报缺)。
fn snapshot_pixels(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
    copy_id: CopyId,
) -> Option<Texture> {
    if let Some(mirror) = targets.snapshot_cpu(copy_id) {
        return Some(mirror.clone());
    }
    let no_overrides = BTreeMap::new();
    for pass in &frame.passes {
        for op in &pass.ops {
            let Op::Copy(copy) = op else { continue };
            if copy.id != copy_id {
                continue;
            }
            let source = match &copy.source {
                // 源层可能已被 `CGEND` 删除 ⇒ 用命令时刻的 `source_texture`。
                CopySource::Layer(src) => resolve_cpu_source_with(
                    frame,
                    textures,
                    targets,
                    &no_overrides,
                    src,
                    copy.source_texture.as_ref(),
                    0,
                )
                .ok()?,
                CopySource::Group(_) => return None,
            };
            let (x0, y0, x1, y1) = copy.region.resolve(source.width, source.height);
            return Some(yu_backend_soft::scale_region(
                &source,
                (x1 - x0 + 1).max(1) as u32,
                (y1 - y0 + 1).max(1) as u32,
                Some((x0, y0, x1, y1)),
                false,
            ));
        }
    }
    None
}

fn resolve_base_texture(
    frame: &CompositorFrame,
    textures: &mut TextureStore,
    targets: &TargetSet,
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
        // 快照目标:取冻结的 CPU 镜像作基面(与 soft 的 `snapshots` 表同义)。
        //
        // `es_SPExec.yst:341` 的 `CGACT[ID="sp.tmpB" ID2=<快照层> MERGE=1]`
        // 就是这种目标;旧实现直接报 unsupported ⇒ 整条 MERGE 被跳过,
        // 动画帧层只剩前一步的清空结果,立绘眨眼/换表情时闪空白。
        // 快照目标:官方是「就地写目标表面」(`FUN_0042f809`:
        // `*(iVar4+0x90) = local_68`),所以基面 = 该快照当前的像素。
        TextureRef::Snapshot(id) => snapshot_pixels(frame, textures, targets, *id)
            .map(Rc::new)
            .ok_or_else(|| "gfx.imageop.target.snapshot_missing".to_string()),
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
    backbuffer: Option<&yu_render_core::Surface>,
    backbuffer_gpu: &mut Option<(u64, u64)>,
    trace_geometry: bool,
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
            trace_geometry,
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
    yu_backend_soft::content_hash_rgba(width, height, rgba)
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
