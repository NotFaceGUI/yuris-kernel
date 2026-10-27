//! 渲染报告契约(两个后端字段语义一致)。

use crate::texture::AssetIssue;

/// 一种被跳过的操作(按 `(kind, reason, target)` 聚合计数)。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SkippedOp {
    /// `layer` / `group` / `transition` / `mask` / `copy` / `image`。
    pub kind: &'static str,
    /// 跳过原因(`OpStatus` 的稳定 code,或后端自身的原因)。
    pub reason: String,
    /// 操作目标图层/组(稳定显示形式)。
    pub target: String,
    /// 同一 key 在本帧的出现次数。
    pub count: u32,
}

/// 一帧的渲染摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReport {
    pub frame: u64,
    pub viewport_width: u32,
    pub viewport_height: u32,
    /// `CompositorFrame.layers` 数量(含不可见层)。
    pub layers: usize,
    pub visible_layers: usize,
    /// pass 总数(含空 pass)。
    pub passes: usize,
    /// 实际执行并写入 offscreen 表面的组 pass 数。
    pub offscreen_passes: usize,
    pub ops_total: usize,
    /// 真正执行(含完全在视口外、未写像素的操作)。
    pub ops_drawn: usize,
    pub skipped_ops_total: u32,
    pub skipped: Vec<SkippedOp>,
    /// 本帧解析失败的资产(`asset.*`)。
    pub missing_assets: Vec<AssetIssue>,
    /// 本帧 `TextRun` 数量(全部未栅格化)。
    pub skipped_text: usize,
    /// `CGACT[TEXT]` 绘制项应用数。
    pub text_draws: usize,
    /// 文本栅格化成功写入的字符数。
    pub text_glyphs: usize,
    /// 文本缺字/不可解码字符数。
    pub text_missing: usize,
    /// 文本失败原因(去重、稳定排序)。
    pub text_diags: Vec<String>,
    pub filter_mag: &'static str,
    pub filter_min: &'static str,
    pub filter_mip: &'static str,
    /// `engine_settings` / `placeholder`。
    pub filter_source: &'static str,
    pub color_space: &'static str,
}