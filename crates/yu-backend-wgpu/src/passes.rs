//! `CompositorFrame.passes` → GPU 绘制计划。
//!
//! 规划阶段只做“读契约 + 解析纹理 + 建管线/bind group + 生成顶点”,
//! 不提交 GPU 命令;执行阶段严格按本模块输出的 pass 顺序录制。
//!
//! 计数与跳过原因逐项复刻 `yu-backend-soft::render`(字段直接用它的
//! [`RenderReport`]),唯一有意的差别是零尺寸组 pass 的 `ops_total` 计数:
//! 软后端在该分支只加 `skipped` 不加 `ops_total`,会破坏
//! `ops_total = drawn + skipped`;本后端仍计总数,并在 README 差异账本留档。
//! ERIS 首帧不含零尺寸组,因此两边计数一致。

use std::collections::BTreeMap;

use yu_backend_soft::{AssetIssue, RenderReport, SkippedOp, TextureStore};
use yu_gfx::{
    fixed_pipeline_alpha, BlendMode, Clear, CompositorFrame, Layer, LayerId, Op, OpStatus,
    PassTarget, TextureRef,
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
}

/// 一次 draw = 6 个顶点 + 管线/bind group 选择。
#[derive(Clone, Debug)]
pub(crate) struct Draw {
    pub blend: BlendMode,
    pub texture_id: u64,
    pub filter: FilterKind,
}

/// 一个已规划 pass。
#[derive(Clone, Debug)]
pub(crate) struct PlannedPass {
    pub target: PlannedTarget,
    pub clear: wgpu::Color,
    pub first_vertex: u32,
    pub draws: Vec<Draw>,
}

/// 一帧的 GPU 执行计划。
#[derive(Clone, Debug, Default)]
pub(crate) struct FramePlan {
    pub passes: Vec<PlannedPass>,
    pub vertices: Vec<Vertex>,
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
) -> Result<(FramePlan, RenderReport), Diag> {
    let mut acc = Accum::new(frame);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut passes: Vec<PlannedPass> = Vec::new();
    let mut screen_planned = false;

    for pass in &frame.passes {
        acc.passes += 1;
        match &pass.target {
            PassTarget::Texture(_) => {
                for op in &pass.ops {
                    acc.ops_total += 1;
                    let reason = match op.status() {
                        OpStatus::Unimplemented(why) => why.to_string(),
                        OpStatus::Executable => "texture_pass.not_executed".to_string(),
                    };
                    acc.skip(op, reason);
                }
            }
            PassTarget::Offscreen(id) => {
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
                    target: PlannedTarget::Offscreen(id.clone()),
                    clear: clear_color(pass.clear.unwrap_or(Clear::explicit([0, 0, 0, 0]))),
                    first_vertex: vertices.len() as u32,
                    draws: Vec::new(),
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
            PassTarget::Screen => {
                let (width, height) = (frame.viewport.width, frame.viewport.height);
                targets.ensure_screen(ctx, width, height, pipelines)?;
                let mut planned = PlannedPass {
                    target: PlannedTarget::Screen,
                    clear: clear_color(frame.clear),
                    first_vertex: vertices.len() as u32,
                    draws: Vec::new(),
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
        }
    }

    if !screen_planned {
        // 与 soft 一致:没有 Screen pass 时仍输出一张按 frame.clear 清屏的表面。
        let (width, height) = (frame.viewport.width, frame.viewport.height);
        targets.ensure_screen(ctx, width, height, pipelines)?;
        passes.push(PlannedPass {
            target: PlannedTarget::Screen,
            clear: clear_color(frame.clear),
            first_vertex: vertices.len() as u32,
            draws: Vec::new(),
        });
    }

    let missing_assets = textures.issues();
    Ok((FramePlan { passes, vertices }, acc.finish(missing_assets)))
}

struct Planner<'a> {
    ctx: &'a GpuContext,
    frame: &'a CompositorFrame,
    textures: &'a mut TextureStore,
    gpu: &'a mut TextureCache,
    pipelines: &'a mut PipelineCache,
    targets: &'a TargetSet,
    vertices: &'a mut Vec<Vertex>,
    acc: &'a mut Accum,
    dst_width: u32,
    dst_height: u32,
}

impl Planner<'_> {
    fn plan_op(&mut self, op: &Op, out: &mut PlannedPass) -> Result<(), Diag> {
        self.acc.ops_total += 1;
        match op {
            Op::Layer(layer) => self.plan_layer(op, layer, out),
            Op::Group(group) => {
                if !group.layer.visible {
                    self.acc.skip(op, "invisible");
                    return Ok(());
                }
                if let Some(why) = status_reason(op) {
                    self.acc.skip(op, why);
                    return Ok(());
                }
                let Some(blend) = supported_blend(group.layer.blend) else {
                    self.acc
                        .skip(op, format!("blend.{}", group.layer.blend.name()));
                    return Ok(());
                };
                let Some(target) = self.targets.offscreen(&group.offscreen) else {
                    self.acc.skip(op, "offscreen.missing");
                    return Ok(());
                };
                let (width, height) = (target.width, target.height);
                if width == 0 || height == 0 {
                    self.acc.skip(op, "group.size.zero");
                    return Ok(());
                }
                let view = target.view.clone();
                let (x, y) = (group.layer.transform.x, group.layer.transform.y);
                let alpha = fixed_pipeline_alpha(group.layer.alpha) as f32;
                // 组离屏结果按软后端约定用 MAG 过滤(不区分放大/缩小)。
                let filter = FilterKind::from_mode(self.frame.filter.mag);
                self.draw(
                    out, blend, target.id, &view, filter, x, y, width, height, alpha,
                )
            }
            Op::Transition(_) => {
                let reason = status_reason(op).unwrap_or("transition.unsupported");
                self.acc.skip(op, reason);
                Ok(())
            }
            Op::Mask(_) => {
                let reason = status_reason(op).unwrap_or("mask.unsupported");
                self.acc.skip(op, reason);
                Ok(())
            }
            Op::Copy(_) => {
                let reason = status_reason(op).unwrap_or("copy.unsupported");
                self.acc.skip(op, reason);
                Ok(())
            }
            Op::Image(_) => {
                let reason = status_reason(op).unwrap_or("imageop.unsupported");
                self.acc.skip(op, reason);
                Ok(())
            }
        }
    }

    fn plan_layer(&mut self, op: &Op, layer: &Layer, out: &mut PlannedPass) -> Result<(), Diag> {
        if !layer.visible {
            self.acc.skip(op, "invisible");
            return Ok(());
        }
        if let Some(why) = status_reason(op) {
            self.acc.skip(op, why);
            return Ok(());
        }
        if !layer.parts.is_empty() {
            self.acc.skip(op, "gfx.parts.unsupported");
            return Ok(());
        }
        let Some(blend) = supported_blend(layer.blend) else {
            self.acc.skip(op, format!("blend.{}", layer.blend.name()));
            return Ok(());
        };
        let explicit_size = layer.image_size();
        let (x, y) = (layer.transform.x, layer.transform.y);
        let alpha = fixed_pipeline_alpha(layer.alpha) as f32;

        match &layer.texture {
            TextureRef::Asset(key) => {
                if explicit_size.is_some() {
                    // 资源图 + SX/SY 的缩放/裁剪未取证;gfx 已标 Unimplemented,
                    // 这里再显式挡一次(与 soft 同一理由码)。
                    self.acc.skip(op, "asset.size.unsupported");
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
                            out, blend, texture_id, &view, filter, x, y, width, height, alpha,
                        )
                    }
                    Err(diag) if diag.code.starts_with("asset.") => {
                        self.acc.skip(op, diag.code.to_string());
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
                    out, blend, texture_id, &view, filter, x, y, width, height, alpha,
                )
            }
            TextureRef::Offscreen(id) => {
                let Some(target) = self.targets.offscreen(id) else {
                    self.acc.skip(op, "offscreen.missing");
                    return Ok(());
                };
                let (width, height) = (target.width, target.height);
                let view = target.view.clone();
                let filter = FilterKind::from_mode(self.frame.filter.mag);
                self.draw(
                    out, blend, target.id, &view, filter, x, y, width, height, alpha,
                )
            }
            TextureRef::Snapshot(_) => {
                self.acc.skip(op, "snapshot.unsupported");
                Ok(())
            }
            TextureRef::None => {
                self.acc.skip(op, "texture.none");
                Ok(())
            }
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
            });
        }
        self.acc.ops_drawn += 1;
        Ok(())
    }
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
        BlendMode::Normal | BlendMode::Add | BlendMode::Subtract => Some(mode),
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
        Op::Image(image) => image.source.display(),
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
            skipped_text: frame.text.len(),
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
            filter_mag: self.filter_mag,
            filter_min: self.filter_min,
            filter_mip: self.filter_mip,
            filter_source: self.filter_source,
            color_space: self.color_space,
        }
    }
}
