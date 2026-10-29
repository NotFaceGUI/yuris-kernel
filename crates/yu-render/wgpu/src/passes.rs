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
    AssetIssue, RenderReport, SkippedOp, TextPaintStats, TextureStore,
};
use yu_gfx::{
    fixed_pipeline_alpha, shader_alpha, BlendMode, Clear, CompositorFrame, CopyId, Layer, LayerId,
    Op, OpStatus, TextureRef,
};
use yu_log::Diag;

use crate::device::{code, GpuContext};
use crate::pipelines::{FilterKind, PipelineCache};
use crate::surfaces::{Contents, TexRef};
use crate::textures::{TargetSet, TextureCache, Vertex};

/// 计划中的一个 pass 目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlannedTarget {
    Screen,
    Offscreen(LayerId),
    /// 常驻层表面的写入目标(按纹理 id;`FramePlan::views` 解析 view)。
    Surface(u64),
    /// 复制/填充命令段(无 draw,不改变渲染目标)。
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
    /// 表面写入用全屏三角形(合成/滤镜/重采样):`aux_bind_groups[bind_index]`。
    Aux {
        bind_index: u32,
        pipeline: AuxPipeline,
    },
}

/// 表面写入的附加管线变体(编码期映射到 `PipelineCache` 的管线)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuxPipeline {
    /// `CGACT[MERGE]`(逐位一致专用 shader)。
    LayerMerge,
    /// `CGACT[MERGE2/XOR/AND]`。
    Composite,
    /// `CGACT[TRANSITION]` 双源整数 lerp。
    Lerp,
    /// 逐层原地滤镜(REVERSE/镜像/MEIDO/SAIDO/AKARUSA/MONO/SEPIA/MOSAIC/ROTATION)。
    Transform,
    /// 区域重采样(物化 `SIZE` 的采样窗)。
    Resample,
}

#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub blend: BlendMode,
    pub texture_id: u64,
    pub filter: FilterKind,
    pub kind: DrawKind,
}

/// 常驻表面上的一次矩形填充(RECTPAINT 族)。
#[derive(Clone, Debug)]
pub(crate) struct PaintCmd {
    pub target_id: u64,
    /// 只写 alpha(`RECTPAINT2`)。
    pub alpha_only: bool,
    /// `f32::to_bits`(与 `PipelineCache::paint_groups` 的键一致)。
    pub color: [u32; 4],
    /// 该绘制使用的 6 个顶点在 `FramePlan.vertices` 里的起点。
    pub first_vertex: u32,
}

/// 一个已规划 pass。
#[derive(Clone, Debug, Default)]
pub(crate) struct PlannedPass {
    pub target: Option<PlannedTarget>,
    pub clear: wgpu::Color,
    pub first_vertex: u32,
    pub draws: Vec<Draw>,
    /// 纹理 → 纹理区域复制(COPY 快照/RESIZE/TRIM/保留下层的填充)。
    pub blits: Vec<BlitCmd>,
    /// 表面上的矩形填充(RECTPAINT 族)。
    pub paints: Vec<PaintCmd>,
    /// 需要先清成透明的表面 id(`TargetSet`/`FramePlan::views` 解析)。
    pub clear_targets: Vec<u64>,
    /// `true` = 保留目标现有内容(表面写入);`false` = 按 `clear` 清屏。
    pub load: bool,
}

/// 一次纹理区域复制(源/目标都是已存在的纹理 id)。
#[derive(Clone, Debug)]
pub(crate) struct BlitCmd {
    /// 产生这条复制的路径(`copy`/`resize`/`trim`/`paint`),诊断用。
    pub label: &'static str,
    pub source_id: u64,
    pub dest_id: u64,
    pub src_origin: (u32, u32),
    pub dst_origin: (u32, u32),
    pub width: u32,
    pub height: u32,
}

/// 一帧的 GPU 执行计划。
#[derive(Default)]
pub(crate) struct FramePlan {
    pub passes: Vec<PlannedPass>,
    pub vertices: Vec<Vertex>,
    /// 屏幕过渡 bind group(按 draw 索引)。
    pub transition_bind_groups: Vec<wgpu::BindGroup>,
    /// 过渡 uniform buffer(保活,执行期由 bind group 引用)。
    #[allow(dead_code)]
    pub transition_buffers: Vec<wgpu::Buffer>,
    /// 表面写入的附加 bind group(合成/滤镜/重采样/lerp)。
    pub aux_bind_groups: Vec<wgpu::BindGroup>,
    /// 表面纹理 id → view(编码期解析 `PlannedTarget::Surface` 与清屏目标)。
    pub views: std::collections::BTreeMap<u64, wgpu::TextureView>,
    /// 表面纹理 id → 纹理(编码期做区域复制)。
    pub textures: std::collections::BTreeMap<u64, wgpu::Texture>,
    /// GPU 文本任务(基面拷贝 + 字形 compute;编码期在所有 render pass 前执行)。
    pub text_jobs: Vec<crate::text_gpu::TextGpuJob>,
}

impl FramePlan {
    pub fn draw_calls(&self) -> usize {
        self.passes.iter().map(|p| p.draws.len()).sum()
    }
}

/// 规划一帧,同时产出与软后端字段一致的 [`RenderReport`]。
#[allow(clippy::too_many_arguments)]
pub(crate) struct Planner<'a> {
    pub(crate) ctx: &'a GpuContext,
    pub(crate) frame: &'a CompositorFrame,
    pub(crate) textures: &'a mut TextureStore,
    pub(crate) gpu: &'a mut TextureCache,
    pub(crate) pipelines: &'a mut PipelineCache,
    pub(crate) targets: &'a mut TargetSet,
    pub(crate) vertices: &'a mut Vec<Vertex>,
    pub(crate) acc: &'a mut Accum,
    /// 当前图层像素(`LayerId → Content`;命令序推进时同步更新)。
    pub(crate) contents: &'a Contents,
    /// 基面解析缓存(资产上传/快照/离屏)。
    pub(crate) bases: &'a mut BTreeMap<(LayerId, u64), Rc<TexRef>>,
    /// 表面纹理 id → view(过渡/合成取样时解析)。
    pub(crate) views: &'a BTreeMap<u64, wgpu::TextureView>,
    pub(crate) transition_bind_groups: &'a mut Vec<wgpu::BindGroup>,
    pub(crate) transition_buffers: &'a mut Vec<wgpu::Buffer>,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    /// COPY 源组的按需重绘不进入报告计数(soft 的同名路径同样回滚)。
    pub(crate) counted: bool,
    /// `YU_RENDER_TIMING` 下逐层记录几何(位置/尺寸/UV)。
    pub(crate) trace_geometry: bool,
}

impl Planner<'_> {
    pub(crate) fn skip(&mut self, op: &Op, reason: impl Into<String>) {
        if self.counted {
            self.acc.skip(op, reason);
        }
    }

    fn drawn(&mut self) {
        if self.counted {
            self.acc.ops_drawn += 1;
        }
    }

    pub(crate) fn plan_op(&mut self, op: &Op, out: &mut PlannedPass) -> Result<(), Diag> {
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
        // `from`/`mask` 的当前像素:常驻内容优先(含 UV 采样窗),否则基图。
        let from = match self.transition_source(transition.from_layer.as_ref(), &transition.from) {
            Ok(value) => value,
            Err(reason) => {
                self.skip(op, reason);
                return Ok(());
            }
        };
        let mask = match &transition.to {
            Some(to) => {
                let resolved = match transition.to_layer.as_ref() {
                    Some(id) => self.transition_source(Some(id), to),
                    None => self.transition_source(None, to),
                };
                match resolved {
                    Ok(value) => value,
                    Err(reason) => {
                        self.skip(op, reason);
                        return Ok(());
                    }
                }
            }
            None => {
                // 配对缺失(不应到达;gfx 已在 op_for_layer 回退普通层)。
                return self.plan_layer(op, layer, out);
            }
        };
        let (width, height) = match layer.image_size() {
            Some(size) => (size.width, size.height),
            None => (from.1, from.2),
        };
        let filter = pick_filter(self.frame, from.1, from.2, width, height);
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
        // 内容自身的显示尺寸(`SIZE` 的浮点目标)与采样窗。
        let (draw_w, draw_h) = match self.contents.get(&layer.id) {
            Some(content) => match content.display {
                Some((w, h)) => (w, h),
                None => (f64::from(width), f64::from(height)),
            },
            None => (f64::from(width), f64::from(height)),
        };
        let (draw_uv, mask_uv) = match variant {
            Variant::Fixed => (from.3, mask.3),
            Variant::Psc | Variant::Psd => ([0.0f32, 0.0, 1.0, 1.0], [0.0f32, 0.0, 1.0, 1.0]),
        };
        let (draw_uv, mask_uv) = (clamp_uv_max(draw_uv), clamp_uv_max(mask_uv));
        let uniform_values: Vec<f32> = match variant {
            Variant::Fixed => vec![
                transition.fade as f32,
                if transition.complete() { 1.0 } else { 0.0 },
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
            Variant::Psc => vec![transition.fade as f32, f32::from(transition.ta), sa, 0.0],
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
        let from_texture = from.4.clone();
        let mask_texture = mask.4.clone();
        let group = self.pipelines.create_transition_bind_group(
            self.ctx,
            &from_texture,
            &mask_texture,
            filter,
            &uniform,
        )?;
        let bind_index = self.transition_bind_groups.len() as u32;
        self.transition_bind_groups.push(group);
        self.transition_buffers.push(uniform);
        let (x, y) = (layer.transform.x, layer.transform.y);
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
        if self.trace_geometry {
            log::trace!(
                target: "yu-render",
                "[geo] f={} layer={} xy=({:.4},{:.4}) explicit_size={:?} content={} alpha={}",
                self.frame.frame,
                layer.id.display(),
                x,
                y,
                explicit_size.map(|s| (s.width, s.height)),
                self.contents
                    .get(&layer.id)
                    .map(|c| format!(
                        "{}x{} uv=[{:.3},{:.3},{:.3},{:.3}] display={:?}",
                        c.width, c.height, c.uv[0], c.uv[1], c.uv[2], c.uv[3], c.display
                    ))
                    .unwrap_or_else(|| "base".to_string()),
                alpha,
            );
        }

        // 常驻内容:直接画(采样窗用 UV;显示尺寸用 `SIZE` 的浮点目标)。
        if let Some(content) = self.contents.get(&layer.id).cloned() {
            if content.width == 0 || content.height == 0 {
                self.skip(op, "layer.size.zero");
                return Ok(());
            }
            let (draw_w, draw_h) = match content.display {
                Some((w, h)) => (w, h),
                None => match explicit_size {
                    Some(size) => (f64::from(size.width), f64::from(size.height)),
                    None => (f64::from(content.width), f64::from(content.height)),
                },
            };
            let filter = if content.display.is_some() {
                if content.bilinear {
                    FilterKind::Linear
                } else {
                    FilterKind::Point
                }
            } else if !content.is_identity() {
                FilterKind::Point
            } else {
                pick_filter(
                    self.frame,
                    content.width,
                    content.height,
                    draw_w.round() as u32,
                    draw_h.round() as u32,
                )
            };
            let tex_id = content.tex.id;
            let view = content.tex.view.clone();
            return self.draw_uv(
                out,
                blend,
                tex_id,
                &view,
                filter,
                (x as f64, y as f64),
                (draw_w, draw_h),
                alpha,
                DrawKind::Quad,
                content.uv,
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

    /// 纹理 id → view(表面先在 `FramePlan::views` 里找)。
    fn view_of(&self, id: u64) -> Option<&wgpu::TextureView> {
        self.views
            .get(&id)
            .or_else(|| view_by_id(self.gpu, self.targets, id))
    }

    /// 过渡源:常驻内容优先(返回纹理 + 采样窗 + 视图),否则按纹理引用解析。
    ///
    /// 必须直接带出 `TextureView`:常驻表面纹理可能**本帧未被写入**
    /// (稳定的立绘/背景),不在逐帧 `views` 表里;若再按 id 反查会误判
    /// `transition.mask_missing` ⇒ 整条过渡被跳过(演出淡入淡出直接消失)。
    fn transition_source(
        &mut self,
        id: Option<&LayerId>,
        fallback: &TextureRef,
    ) -> Result<(u64, u32, u32, [f32; 4], wgpu::TextureView), String> {
        if let Some(id) = id {
            if let Some(content) = self.contents.get(id).cloned() {
                return Ok((
                    content.tex.id,
                    content.tex.width,
                    content.tex.height,
                    content.uv,
                    content.tex.view.clone(),
                ));
            }
            return match self.layer_image(id, None) {
                Ok((tex_id, width, height)) => {
                    let view = self
                        .view_of(tex_id)
                        .cloned()
                        .ok_or_else(|| "transition.source_missing".to_string())?;
                    Ok((tex_id, width, height, [0.0, 0.0, 1.0, 1.0], view))
                }
                Err(reason) => Err(reason),
            };
        }
        let (tex_id, width, height) = self.image_ref(fallback)?;
        let view = self
            .view_of(tex_id)
            .cloned()
            .ok_or_else(|| "transition.source_missing".to_string())?;
        Ok((tex_id, width, height, [0.0, 0.0, 1.0, 1.0], view))
    }

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
    /// 某层的 GPU 纹理身份 + 视图 + 像素尺寸(UV 采样用)。
    ///
    /// 与 `layer_image` 的区别:这里返回**视图**而非只有 id —— 绘制期要
    /// 直接拿它建 bind group。源可以是资产/1×1/快照/已加工结果。
    /// 层在本帧命令时刻的基图(源层被删后仍可解析像素)。
    /// 图层源的当前图像。
    ///
    /// 源层可能已被 `CGEND` 删除(`DIV-GFX-053`):此时回退到 `CopyOp` 在命令
    /// 时刻记录的 `source_texture` 重放。
    fn layer_image(
        &mut self,
        id: &LayerId,
        fallback: Option<&TextureRef>,
    ) -> Result<(u64, u32, u32), String> {
        if let Some(content) = self.contents.get(id) {
            return Ok((content.tex.id, content.width, content.height));
        }
        let layer = self.frame.layers.iter().find(|layer| &layer.id == id);
        if layer.is_some_and(|layer| layer.is_group()) {
            return self.group_image(id);
        }
        let texture = match layer {
            Some(layer) => &layer.texture,
            None => fallback.ok_or_else(|| "gfx.copy.source.gone".to_string())?,
        };
        if matches!(texture, TextureRef::None) {
            return Err("gfx.copy.source.texture_none".to_string());
        }
        let tex = crate::plan::resolve_ref(
            self.ctx,
            self.textures,
            self.gpu,
            self.targets,
            id,
            texture,
            self.bases,
        )?;
        Ok((tex.id, tex.width, tex.height))
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
pub(crate) fn note_text_diag(acc: &mut Accum, why: String) {
    if !acc.text_diags.contains(&why) {
        acc.text_diags.push(why);
        acc.text_diags.sort();
    }
}

pub(crate) fn merge_text_stats(acc: &mut Accum, stats: TextPaintStats) {
    acc.text_draws += stats.draws;
    acc.text_glyphs += stats.glyphs;
    acc.text_missing += stats.missing;
    for why in stats.diags {
        note_text_diag(acc, why);
    }
}

pub(crate) fn group_surface_size(frame: &CompositorFrame, id: &LayerId) -> (u32, u32) {
    frame
        .layers
        .iter()
        .find(|layer| &layer.id == id)
        .and_then(|layer| layer.group_size())
        .map_or((frame.viewport.width, frame.viewport.height), |size| {
            (size.width, size.height)
        })
}

pub(crate) fn status_reason(op: &Op) -> Option<&'static str> {
    match op.status() {
        OpStatus::Executable => None,
        OpStatus::Unimplemented(why) => Some(why),
    }
}

pub(crate) fn supported_blend(mode: BlendMode) -> Option<BlendMode> {
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

pub(crate) fn pick_filter(
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

pub(crate) struct Accum {
    // 计数由 plan.rs 直接推进(与 soft 的 RenderReport 逐字段对齐)。
    pub(crate) frame: u64,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) layers: usize,
    pub(crate) visible_layers: usize,
    pub(crate) passes: usize,
    pub(crate) offscreen_passes: usize,
    pub(crate) ops_total: usize,
    pub(crate) ops_drawn: usize,
    pub(crate) skipped_ops_total: u32,
    pub(crate) skipped: BTreeMap<(&'static str, String, String), u32>,
    pub(crate) skipped_text: usize,
    pub(crate) text_draws: usize,
    pub(crate) text_glyphs: usize,
    pub(crate) text_missing: usize,
    pub(crate) text_diags: Vec<String>,
    pub(crate) filter_mag: &'static str,
    pub(crate) filter_min: &'static str,
    pub(crate) filter_mip: &'static str,
    pub(crate) filter_source: &'static str,
    pub(crate) color_space: &'static str,
}

impl Accum {
    pub(crate) fn new(frame: &CompositorFrame) -> Self {
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

    pub(crate) fn skip(&mut self, op: &Op, reason: impl Into<String>) {
        self.skipped_ops_total += 1;
        let key = (op_kind(op), reason.into(), op_target(op));
        *self.skipped.entry(key).or_insert(0) += 1;
    }

    pub(crate) fn finish(self, missing_assets: Vec<AssetIssue>) -> RenderReport {
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

pub(crate) fn clamp_uv_max(uv: [f32; 4]) -> [f32; 4] {
    [
        uv[0].min(1.0),
        uv[1].min(1.0),
        uv[2].min(1.0).max(uv[0].min(1.0)),
        uv[3].min(1.0).max(uv[1].min(1.0)),
    ]
}

/// 像素内容指纹(FNV-1a over 尺寸 + RGBA)。
pub(crate) fn hash_rgba(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in [u64::from(width), u64::from(height)] {
        hash ^= value;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    for byte in rgba {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// 表面写入的产物收集器(计划期把 pass/顶点/bind group 就地登记)。
pub(crate) struct SurfaceWrites<'a> {
    pub(crate) passes: &'a mut Vec<PlannedPass>,
    pub(crate) vertices: &'a mut Vec<Vertex>,
    pub(crate) aux: &'a mut Vec<wgpu::BindGroup>,
    pub(crate) views: &'a mut std::collections::BTreeMap<u64, wgpu::TextureView>,
    pub(crate) textures: &'a mut std::collections::BTreeMap<u64, wgpu::Texture>,
    pub(crate) keepalive: &'a mut std::collections::BTreeMap<u64, std::rc::Rc<TexRef>>,
}

impl SurfaceWrites<'_> {
    pub(crate) fn remember(&mut self, tex: &std::rc::Rc<TexRef>) {
        self.views.insert(tex.id, tex.view.clone());
        self.textures.insert(tex.id, tex.texture.clone());
        self.keepalive.insert(tex.id, tex.clone());
    }

    /// 全屏三角形成像(合成/滤镜/重采样):`bind` 已按管线布局建好。
    pub(crate) fn fullscreen(
        &mut self,
        target: &std::rc::Rc<TexRef>,
        bind: wgpu::BindGroup,
        pipeline: AuxPipeline,
    ) {
        self.remember(target);
        let index = self.aux.len() as u32;
        self.aux.push(bind);
        let mut pass = PlannedPass {
            target: Some(PlannedTarget::Surface(target.id)),
            load: true,
            ..PlannedPass::default()
        };
        pass.draws.push(Draw {
            blend: BlendMode::Normal,
            texture_id: target.id,
            filter: FilterKind::Point,
            kind: DrawKind::Aux { bind_index: index, pipeline },
        });
        self.passes.push(pass);
    }
}

/// 生成目标矩形的两个三角形,UV 取子矩形 `[u0, v0, u1, v1]`。
pub(crate) fn push_quad_uv(
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

pub(crate) fn view_by_id<'a>(
    gpu: &'a TextureCache,
    targets: &'a TargetSet,
    id: u64,
) -> Option<&'a wgpu::TextureView> {
    if let Some(texture) = gpu.get(id) {
        return Some(&texture.view);
    }
    targets.view_by_id(id)
}

pub(crate) fn push_quad(
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

    let tl = Vertex { position: [left, top], uv: [0.0, 0.0], alpha };
    let tr = Vertex { position: [right, top], uv: [1.0, 0.0], alpha };
    let bl = Vertex { position: [left, bottom], uv: [0.0, 1.0], alpha };
    let br = Vertex { position: [right, bottom], uv: [1.0, 1.0], alpha };
    vertices.extend_from_slice(&[tl, tr, bl, tr, br, bl]);
}

pub(crate) fn clear_color(clear: Clear) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(clear.rgba[0]) / 255.0,
        g: f64::from(clear.rgba[1]) / 255.0,
        b: f64::from(clear.rgba[2]) / 255.0,
        a: f64::from(clear.rgba[3]) / 255.0,
    }
}

/// 存活快照的**传递闭包**(层引用 + 本帧 COPY 产出 + COPY 命令时刻的源快照)。
pub(crate) fn live_snapshots(frame: &CompositorFrame) -> BTreeSet<CopyId> {
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
            if let Some(TextureRef::Snapshot(id)) = &copy.source_texture {
                if live.insert(*id) {
                    pending.push(*id);
                }
            }
        }
    }
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
