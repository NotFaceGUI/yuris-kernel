//! 图层常驻表面(`LayerSurfaces`)。
//!
//! # 证据
//!
//! 官方引擎的 `CGACT` 在命令处理器内**就地**改目标图层的像素表面
//! (`FUN_0042f809` 尾部 `*(iVar4+0x90) = local_68`,`crates/yu-gfx/src/state.rs`
//! 的 `supersede_image_op` 注释引同一地址);存档的图层像素通道直接读它
//! (`0x4477c0`,首 u32 = `0x640` 位图头,`crates/yu-gfx/src/host.rs` 的
//! `LayerCapture`)。`COPY` 也直接读/写纹理,不读“上一帧”缓冲
//! (`DIV-GFX-051` §官方行为)。
//!
//! 因此后端契约是:**每个图层一张常驻像素表面,`CGACT` 按命令序就地改它,
//! 帧只负责把可见层 blit 到后缓冲**。本模块只承载这份状态:
//!
//! - [`LayerSurfaces`]:按 `LayerId` 的表面表 + 纹理池 + 预算/剪枝;
//! - [`Content`]:一层“当前像素”的描述(`纹理 + UV 采样窗 + 逻辑像素网格`);
//! - 已应用操作前缀(`applied`):同一 `(seq, 签名)` 的 op 不再重复执行 ——
//!   官方命令只执行一次,本内核每帧重发的是**命令日志**。
//!
//! # 不变量
//!
//! 1. **写入永不原地**:每个像素写入申请一张新纹理(池内复用),旧内容对
//!    本帧已登记的读取者仍然有效(命令时刻语义,EVID-T-15)。
//! 2. **跨帧常驻**:层存活且基面/操作前缀未变 ⇒ 内容纹理原样保留,
//!    下一帧零 pass、零上传。
//! 3. **UV 是采样修饰**:`SIZE/SIZE2/HORIZONTAL/VERTICAL` 只改采样窗口与
//!    逻辑网格,不重采样(与 soft `apply_image_op` 的“无区域复用源纹理”
//!    及既有 wgpu `Override::Uv` 同口径);只有像素动作(填充/合成/过渡/
//!    滤镜/文本/复制落点)才物化。
//! 4. 池只在**帧末**回收未被引用的内容纹理(快照/文本/别名持有 `Rc` ⇒
//!    自动钉住,计数 > 1 时不入池)。

use std::collections::BTreeMap;
use std::rc::Rc;

use yu_gfx::LayerId;

use crate::device::GpuContext;
use crate::pipelines::TARGET_FORMAT;
use crate::textures::RenderTarget;

/// 一张可采样/可渲染的纹理句柄。`wgpu::Texture`/`TextureView` 内部是
/// 引用计数句柄,克隆只 bump 一次计数。
#[derive(Clone)]
pub struct TexRef {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl TexRef {
    pub fn from_target(target: &RenderTarget) -> Self {
        Self {
            id: target.id,
            width: target.width,
            height: target.height,
            texture: target.texture.clone(),
            view: target.view.clone(),
        }
    }

    pub fn bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

/// 一层的“当前像素”。
///
/// `tex` + `uv` 是**采样**结果:逻辑像素网格是 `width × height`,第
/// `(dx, dy)` 个像素对应 `tex` 上 UV 窗口内的采样。`uv == [0,0,1,1]` 且
/// `width/height == tex 尺寸` 时就是恒等(不需要重采样)。
#[derive(Clone)]
pub struct Content {
    pub tex: Rc<TexRef>,
    /// 采样窗口(归一化,已按闭区间换算:宽 `x1-x0+1`)。
    pub uv: [f32; 4],
    /// 逻辑像素网格(缩放的输出尺寸;恒等时 = tex 尺寸)。
    pub width: u32,
    pub height: u32,
    /// `SIZE` 族最近一次指定的滤波(采样修饰的绘制提示)。
    pub bilinear: bool,
    /// `SIZE` 族给出的**显示尺寸**(浮点;`None` = 用层声明尺寸)。
    ///
    /// 官方把目标尺寸按 16.16 定点保存,立绘缩放每帧只变零点几像素;
    /// 绘制期用这份浮点值,避免「几帧不动、下一帧跳 1px」的阶梯。
    pub display: Option<(f64, f64)>,
    /// 内容身份:每次写入/修饰变化都不同;源身份判定与跨帧缓存键都用它。
    pub hash: u64,
}

impl Content {
    pub fn identity(tex: Rc<TexRef>, hash: u64) -> Self {
        let (width, height) = (tex.width, tex.height);
        Self {
            tex,
            uv: [0.0, 0.0, 1.0, 1.0],
            width,
            height,
            bilinear: false,
            display: None,
            hash,
        }
    }

    /// 采样窗口是否已经是恒等(像素网格 = 纹理尺寸,UV 全幅)。
    pub fn is_identity(&self) -> bool {
        self.uv == [0.0, 0.0, 1.0, 1.0]
            && self.width == self.tex.width
            && self.height == self.tex.height
    }
}

/// 一层的常驻状态。
#[derive(Default)]
struct LayerState {
    /// 基面身份(`layer.texture` + 快照代际);变了 ⇒ 旧内容与操作前缀作废。
    base: u64,
    /// 已应用的操作前缀(`(seq, 签名)`,命令序)。
    applied: Vec<(u64, u64)>,
    /// 当前像素;`None` = 直接采样基图(未物化)。
    content: Option<Content>,
    /// 已烘进内容的文本修订号(0 = 无文本;与 `CompositorFrame::text_revisions`
    /// 同键)。
    text_rev: u64,
    /// 文本绘制项签名(修订号缺席时的补充身份)。
    text_sig: u64,
    /// 文本合成前的像素身份(文本变化时据此重建)。
    raw_hash: u64,
    /// 文本合成前的像素(冻结文本 COPY 的基面;`DIV-GFX-058`)。
    raw: Option<Content>,
    /// 上一帧是否被引用(剪枝用)。
    last_frame: u64,
}

impl LayerState {
    fn clear(&mut self) {
        self.applied.clear();
        self.content = None;
        self.text_rev = 0;
        self.text_sig = 0;
        self.raw_hash = 0;
        self.raw = None;
    }
}

/// 一层表面应用的记账(诊断/性能门槛)。
#[derive(Clone, Copy, Debug, Default)]
pub struct SurfaceStats {
    /// 本帧新建的层表面纹理数。
    pub created: u64,
    /// 本帧从池里复用的纹理数。
    pub reused: u64,
    /// 本帧释放回池的纹理数。
    pub released: u64,
    /// 本帧真正执行的像素 pass 数(表面写入)。
    pub surface_passes: u64,
    /// 本帧纹理区域复制次数(COPY 快照/RESIZE/TRIM)。
    pub blits: u64,
    /// 本帧因操作前缀相同而复用的层数。
    pub reused_layers: u64,
    /// 本帧为 UV 采样修饰物化(重采样)的次数。
    pub materialized: u64,
    /// 本帧按需 CPU 回退(无 GPU shader 的动作)次数。
    pub cpu_fallbacks: u64,
    /// 当前常驻表面纹理数 / 字节。
    pub live_surfaces: usize,
    pub live_bytes: usize,
    /// 池中空闲纹理数 / 字节。
    pub pooled: usize,
    pub pooled_bytes: usize,
    /// 快照别名命中数(零拷贝)。
    pub alias_hits: u64,
}

/// 单个尺寸的池容量上限(按纹理张数;同尺寸纹理可互相复用)。
const POOL_PER_SIZE: usize = 4;
/// 只缓存不超过这个尺寸的纹理:大表面(精灵缓冲 2048²=16MB)复用收益抵不上
/// 常驻显存,直接释放更稳(重新分配只花一次 `create_texture`)。
const POOL_MAX_TEXTURE_BYTES: usize = 4 * 1024 * 1024;
/// 池内总字节上限:超出时丢弃最旧的尺寸组(不静默:计数在诊断里)。
const POOL_MAX_BYTES: usize = 64 * 1024 * 1024;
/// 单张表面上限(4096² RGBA = 64MB;超过 128MB 一定是畸形尺寸)。
const MAX_SURFACE_BYTES: usize = 128 * 1024 * 1024;
/// 常驻层表面的总预算(GPU 还要放资产/交换链/快照;2GB 沿用 `TargetSet`)。
const MAX_SURFACE_TOTAL_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// 图层表面表 + 纹理池。
pub struct LayerSurfaces {
    layers: BTreeMap<LayerId, LayerState>,
    pool: BTreeMap<(u32, u32), Vec<Rc<TexRef>>>,
    frame: u64,
    next_revision: u64,
    pooled_bytes: usize,
    dropped_pooled: u64,
    stats: SurfaceStats,
}

impl Default for LayerSurfaces {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerSurfaces {
    pub fn new() -> Self {
        Self {
            layers: BTreeMap::new(),
            pool: BTreeMap::new(),
            frame: 0,
            next_revision: 1,
            pooled_bytes: 0,
            dropped_pooled: 0,
            stats: SurfaceStats::default(),
        }
    }

    /// 帧开始:清本帧计数,记录帧号。
    pub fn begin_frame(&mut self, frame: u64) {
        self.frame = frame;
        self.stats = SurfaceStats::default();
        self.stats.live_surfaces = self.live_textures();
    }

    pub fn stats(&self) -> SurfaceStats {
        self.stats
    }

    pub fn stats_mut(&mut self) -> &mut SurfaceStats {
        &mut self.stats
    }

    pub fn note_reused_layer(&mut self) {
        self.stats.reused_layers += 1;
    }

    pub fn note_alias_hit(&mut self) {
        self.stats.alias_hits += 1;
    }

    pub fn note_cpu_fallback(&mut self) {
        self.stats.cpu_fallbacks += 1;
    }

    fn next_hash(&mut self) -> u64 {
        let value = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        value
    }

    /// 当前层的像素(未物化 = `None`,调用方按基图采样)。
    pub fn content(&self, id: &LayerId) -> Option<&Content> {
        self.layers.get(id).and_then(|state| state.content.as_ref())
    }

    /// 该层是否已有“已应用前缀 + 物化内容”的状态。
    pub fn has_state(&self, id: &LayerId) -> bool {
        self.layers
            .get(id)
            .is_some_and(|state| !state.applied.is_empty() || state.content.is_some())
    }

    /// 同步基面身份并返回是否发生了重置(基图变了 ⇒ 旧内容作废)。
    pub fn sync_base(&mut self, id: &LayerId, base: u64) -> bool {
        let state = self.layers.entry(id.clone()).or_default();
        if state.base == base {
            return false;
        }
        state.base = base;
        state.clear();
        true
    }

    /// 操作前缀里已经应用到的第 `index` 项的签名。
    pub fn applied_at(&self, id: &LayerId, index: usize) -> Option<(u64, u64)> {
        self.layers.get(id)?.applied.get(index).copied()
    }

    /// 清掉一层的物化内容(操作前缀被折叠/改写时回到基面)。
    pub fn clear_content(&mut self, id: &LayerId) {
        if let Some(state) = self.layers.get_mut(id) {
            state.content = None;
            state.raw = None;
            state.text_rev = 0;
            state.text_sig = 0;
            state.raw_hash = 0;
        }
    }

    pub fn applied_len(&self, id: &LayerId) -> usize {
        self.layers.get(id).map_or(0, |state| state.applied.len())
    }

    /// 截断操作前缀(第 `index` 项及其之后需要重新执行)。
    pub fn truncate_applied(&mut self, id: &LayerId, index: usize) {
        if let Some(state) = self.layers.get_mut(id) {
            state.applied.truncate(index);
        }
    }

    /// 登记一条已应用操作。
    pub fn push_applied(&mut self, id: &LayerId, entry: (u64, u64)) {
        let state = self.layers.entry(id.clone()).or_default();
        state.applied.push(entry);
    }

    /// 设置层的当前像素(像素动作的输出;写入永不原地)。
    pub fn set_content(&mut self, id: &LayerId, content: Content) {
        let state = self.layers.entry(id.clone()).or_default();
        state.content = Some(content);
    }

    /// 文本阶段:内容 = 基面 + 字形;同时记住基面(冻结文本 COPY 用)。
    pub fn set_text_content(&mut self, id: &LayerId, content: Content, raw: Content) {
        let state = self.layers.entry(id.clone()).or_default();
        state.content = Some(content);
        state.raw = Some(raw);
    }

    /// 只改采样修饰(`SIZE`/镜像):保留纹理与已应用前缀,替换窗口/网格。
    pub fn set_sampled(&mut self, id: &LayerId, content: Content) {
        let state = self.layers.entry(id.clone()).or_default();
        state.content = Some(content);
        state.raw = None;
    }

    /// 文本阶段记账(重建条件由调用方判定)。
    pub fn text_state(&self, id: &LayerId) -> (u64, u64, u64) {
        self.layers
            .get(id)
            .map(|state| (state.text_rev, state.text_sig, state.raw_hash))
            .unwrap_or((0, 0, 0))
    }

    pub fn note_text_baked(&mut self, id: &LayerId, rev: u64, sig: u64, raw_hash: u64) {
        let state = self.layers.entry(id.clone()).or_default();
        state.text_rev = rev;
        state.text_sig = sig;
        state.raw_hash = raw_hash;
    }

    /// 记录文本合成前的像素(文本变化时重建的基面)。
    pub fn set_raw(&mut self, id: &LayerId, content: Content) {
        let state = self.layers.entry(id.clone()).or_default();
        state.raw = Some(content);
    }

    /// 基面像素的稳定身份:基面 token + 已应用操作前缀。
    ///
    /// `Content::hash` 混入了纹理池 id,同一像素在换池后身份会变;文本
    /// 基面缓存必须用这份**像素来源**身份,否则每帧重画同一基面都会缓存
    /// 失配并重复 GPU 读回。
    pub fn raw_identity(&self, id: &LayerId) -> Option<u64> {
        let state = self.layers.get(id)?;
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        fnv_mix(&mut hash, state.base);
        for (seq, sig) in &state.applied {
            fnv_mix(&mut hash, *seq);
            fnv_mix(&mut hash, *sig);
        }
        Some(hash)
    }

    /// 文本合成前的像素(冻结文本 COPY 的基面)。
    pub fn raw_content(&self, id: &LayerId) -> Option<Content> {
        self.layers.get(id).and_then(|state| state.raw.clone())
    }

    /// 为新内容申请一张纹理(池内同尺寸复用优先)。
    ///
    /// `what` 只用于 OOM 诊断定位;尺寸非法/超预算返回 `Err`(调用方按
    /// 跳过计数,不猜)。
    pub fn acquire(
        &mut self,
        ctx: &GpuContext,
        width: u32,
        height: u32,
        what: &'static str,
    ) -> Result<Rc<TexRef>, String> {
        if width == 0 || height == 0 {
            return Err("gpu.surface.zero_size".to_string());
        }
        let bytes = (width as usize) * (height as usize) * 4;
        if bytes > MAX_SURFACE_BYTES {
            return Err(format!("gpu.surface.too_large {width}x{height}"));
        }
        if let Some(list) = self.pool.get_mut(&(width, height)) {
            if let Some(tex) = list.pop() {
                self.pooled_bytes = self.pooled_bytes.saturating_sub(tex.bytes());
                self.stats.reused += 1;
                return Ok(tex);
            }
        }
        if self.live_bytes() + bytes > MAX_SURFACE_TOTAL_BYTES {
            // 先清池;仍不够就显式拒绝(不静默降级)。
            self.drop_pool();
            if self.live_bytes() + bytes > MAX_SURFACE_TOTAL_BYTES {
                return Err(format!(
                    "gpu.surface.budget {width}x{height} (常驻 {}MB)",
                    self.live_bytes() / (1024 * 1024)
                ));
            }
        }
        let ctx_checked = ctx
            .check_dimensions(width, height, what)
            .map_err(|diag| diag.code.to_string())?;
        let _ = ctx_checked;
        let descriptor = wgpu::TextureDescriptor {
            label: Some(what),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST
                // GPU 文本 pass 需要 storage read_write。
                | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        };
        let texture = ctx
            .checked_oom(&format!("{what} {width}x{height}"), || {
                ctx.device.create_texture(&descriptor)
            })
            .map_err(|diag| diag.code.to_string())?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.stats.created += 1;
        Ok(Rc::new(TexRef {
            id: crate::textures::allocate_texture_id(),
            width,
            height,
            texture,
            view,
        }))
    }

    /// 帧末回收:只保留各层当前内容纹理(以及被快照/别名钉住的)。
    ///
    /// `live` = 本帧仍存活的层;其余层从表里删除(显存不随场景累积)。
    pub fn end_frame(&mut self, live: &std::collections::BTreeSet<LayerId>) {
        self.layers.retain(|id, state| {
            state.last_frame = self.frame;
            live.contains(id)
        });
        self.trim_pool();
        self.stats.live_surfaces = self.live_textures();
        self.stats.pooled = self.pool.values().map(Vec::len).sum();
        self.stats.pooled_bytes = self.pooled_bytes;
    }

    /// 把一张不再被层内容引用的纹理放回池(强引用 > 1 说明还有快照/别名
    /// 持有 ⇒ 不入池,等最后一个持有者释放时自然回收)。
    pub fn release(&mut self, tex: Rc<TexRef>) {
        if Rc::strong_count(&tex) > 1 {
            return;
        }
        let bytes = tex.bytes();
        if bytes > POOL_MAX_BYTES || bytes > POOL_MAX_TEXTURE_BYTES {
            return;
        }
        let entry = self.pool.entry((tex.width, tex.height)).or_default();
        if entry.len() >= POOL_PER_SIZE {
            return;
        }
        entry.push(tex);
        self.pooled_bytes += bytes;
        self.stats.released += 1;
    }

    fn trim_pool(&mut self) {
        while self.pooled_bytes > POOL_MAX_BYTES {
            let key = match self.pool.keys().next().copied() {
                Some(key) => key,
                None => break,
            };
            let mut empty = false;
            if let Some(list) = self.pool.get_mut(&key) {
                match list.pop() {
                    Some(tex) => {
                        self.pooled_bytes = self.pooled_bytes.saturating_sub(tex.bytes());
                        self.dropped_pooled += 1;
                    }
                    None => empty = true,
                }
            }
            if empty {
                self.pool.remove(&key);
            }
        }
        self.pool.retain(|_, list| !list.is_empty());
    }

    fn drop_pool(&mut self) {
        self.dropped_pooled += self.pool.values().map(|list| list.len() as u64).sum::<u64>();
        self.pool.clear();
        self.pooled_bytes = 0;
    }

    fn live_textures(&self) -> usize {
        self.layers
            .values()
            .filter(|state| state.content.is_some())
            .count()
    }

    /// 常驻层表面字节(诊断/预算;不含池)。
    pub fn live_bytes(&self) -> usize {
        self.layers
            .values()
            .filter_map(|state| state.content.as_ref())
            .map(|content| content.tex.bytes())
            .sum()
    }

    pub fn total_bytes(&self) -> usize {
        self.live_bytes() + self.pooled_bytes
    }

    /// 池丢弃计数(预算/容量上限生效的痕迹;退出摘要可见)。
    pub fn dropped_pooled(&self) -> u64 {
        self.dropped_pooled
    }

    /// 引擎重启:新 `CopyId`/新层,全部表面失效(DIV-HOST-001 同口径)。
    pub fn reset(&mut self) -> usize {
        let removed = self.layers.len();
        self.layers.clear();
        self.drop_pool();
        removed
    }

    /// UV 采样窗的**复合**:把 `inner`(当前内容的窗口)按 `outer`(新区域
    /// 归一化到当前逻辑网格的 UV)再映射一层。
    ///
    /// 线性映射可结合,因此级联 `SIZE` 只保留一个 UV,不需要中间纹理:
    /// `u' = inner.u0 + outer.u * (inner.u1 - inner.u0)`。
    pub fn compose_uv(inner: [f32; 4], outer: [f32; 4]) -> [f32; 4] {
        [
            inner[0] + outer[0] * (inner[2] - inner[0]),
            inner[1] + outer[1] * (inner[3] - inner[1]),
            inner[0] + outer[2] * (inner[2] - inner[0]),
            inner[1] + outer[3] * (inner[3] - inner[1]),
        ]
    }

    /// 新的内容身份(写入/修饰变化时用)。
    pub fn bump_hash(&mut self) -> u64 {
        self.next_hash()
    }

}

/// 一帧的图层内容视图(`LayerId → Content`;命令序推进时同步更新)。
pub(crate) type Contents = BTreeMap<LayerId, Content>;

fn fnv_mix(hash: &mut u64, value: u64) {
    *hash ^= value;
    *hash = hash.wrapping_mul(0x100_0000_01b3);
}
