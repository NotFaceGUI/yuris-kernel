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

/// 一帧渲染的阶段耗时(微秒)。
///
/// **与 [`RenderReport`] 分开**是硬约束:`RenderReport` 参与 soft/wgpu 的
/// 逐计数对账(要求 `Eq` 且两后端逐帧可比较),把运行时耗时混进去会让同
/// 一帧的两个后端永远不相等。这里只做性能取证:后端能测的项填值,测不到
/// 的项保持 0(soft 只填 `total_us`/`plan_us` 之类的大项)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderPhases {
    /// 规划总时长(含下列 `snap`/`image`/`text`/`copy_text`/`rest`)。
    pub plan_us: u64,
    /// 快照尺寸定稿 + CPU 镜像预捕获。
    pub snap_us: u64,
    /// 图像加工(`SIZE`/`MERGE`/`RECTPAINT` …)。
    pub image_us: u64,
    /// 文本合成(烘焙/增量)。
    pub text_us: u64,
    /// COPY 上的冻结文本。
    pub copy_text_us: u64,
    /// 规划其余部分(遍历 passes/层、顶点生成)。
    pub rest_us: u64,
    /// 顶点上传 + 命令编码。
    pub encode_us: u64,
    /// 屏幕读回(流式模式为提交,不阻塞)。
    pub readback_us: u64,
    /// 读回之后的收尾(缩放 pass / 状态更新)。
    pub tail_us: u64,
    /// 文本子阶段:`TextComposer` 的整面重烘焙。
    pub text_bake_us: u64,
    /// 文本子阶段:内容哈希。
    pub text_hash_us: u64,
    /// 文本子阶段:字形绘制。
    pub text_paint_us: u64,
    /// 文本子阶段:基面解析。
    pub text_base_us: u64,
    /// 文本子阶段:CPU 基面取得(`content_cpu`:资产整张克隆或 GPU 读回)。
    pub text_cpu_base_us: u64,
    /// 其中:同步 GPU 读回耗时/次数(按层内容 `read_texture`)。
    pub text_cpu_readback_us: u64,
    pub text_cpu_readbacks: u64,
    /// 其中:资产整张克隆耗时/字节。
    pub text_cpu_asset_us: u64,
    pub text_cpu_asset_bytes: u64,
    /// 其中:文本基面 CPU 缓存命中次数(免读回/免拷贝)与写入次数。
    pub text_cpu_cached: u64,
    pub text_cpu_stores: u64,
    /// GPU 文本 pass:本帧处理的层数/字形数/draw 数/缓存命中/CPU 回退。
    pub text_gpu_layers: u64,
    pub text_gpu_glyphs: u64,
    pub text_gpu_draws: u64,
    pub text_gpu_cached: u64,
    pub text_gpu_fallback: u64,
    /// 图像子阶段:`RECTPAINT` 族 CPU 填充。
    pub image_paint_us: u64,
    /// 图像子阶段:`REVERSE` 等逐像素变换。
    pub image_pixel_us: u64,
    /// 图像子阶段:`MERGE`/`TRANSITION` 整面 CPU 合成。
    pub image_composite_us: u64,
    /// 本帧新上传纹理数。
    pub uploads: u64,
    /// 本帧上传字节(解码后的 RGBA8;含资产首次解码)。
    pub upload_bytes: u64,
    /// 其中资产**首次解码**的字节(一次性加载成本,不是每帧渲染预算)。
    pub upload_asset_bytes: u64,
    /// 本帧 draw call 数。
    pub draw_calls: u64,
    /// 本帧读回字节数。
    pub readback_bytes: u64,
    /// 本帧 GPU 纹理拷贝的**读写总字节**(copy_texture_to_texture,
    /// 含 COPY 快照与 merge blit;按 `extent` 计算,读+写各算一次)。
    pub copy_bytes: u64,
    /// 本帧 GPU 纹理拷贝次数。
    pub copy_count: u64,
    /// 本帧新建的快照 GPU 纹理数(按 CopyId 首次出现计)。
    pub snapshot_alloc: u64,
    /// 本帧资产解码耗时(微秒)与像素字节(VFS 读 + 图像解码)。
    pub decode_us: u64,
    pub decode_bytes: u64,
    /// 图像加工 op 计数(按动作族;长帧取证:`MERGE`/`MERGE2` 是 CPU 合成)。
    pub image_merge_ops: u64,
    pub image_merge2_ops: u64,
    pub image_transition_ops: u64,
    pub image_bitwise_ops: u64,
    pub image_paint_ops: u64,
    pub image_pixel_ops: u64,
    pub image_sampled_ops: u64,
    /// 合成链记账:参与判定的层链数 / 记忆化命中数。
    ///
    /// `chains=5 hits=0` + `image_composite` 高 = 每帧都在重算同一批像素,
    /// 是「该命中却未命中」的直接证据。
    pub memo_chains: u64,
    pub memo_hits: u64,
    /// 本帧真实执行的表面像素 pass 数(填充/合成/过渡/滤镜/重采样)。
    pub surface_passes: u64,
    /// 本帧纹理区域复制次数(COPY 快照/RESIZE/TRIM)。
    pub blits: u64,
    /// 快照别名命中数(零拷贝;W3 第一态)。
    pub alias_hits: u64,
    /// 操作前缀相同而直接复用的层数(官方的“就地执行一次”)。
    pub reused_layers: u64,
    /// UV 采样修饰被物化成真实像素的次数。
    pub materialized: u64,
    /// 无 GPU 实现的 CPU 回退次数(显式计数,不静默)。
    pub cpu_fallbacks: u64,
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