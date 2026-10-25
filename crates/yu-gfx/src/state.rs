//! `GfxState`:消费 `yu_rt::VmEvent`,维护图层树,产出 [`CompositorFrame`]。
//!
//! # 已实现的命令(ERIS 首屏实测覆盖)
//!
//! | 命令 | 首屏次数(frame 0) | 处理 |
//! |---|---:|---|
//! | `CG` | 30,668 | ID/IDNO/E/A/X/Y/Z/SX/SY/FILE/MODE + 原始参数留档 |
//! | `CGACT` | 280 | `COPY2`/`SWAP` 状态、`RECTPAINT*` 记为图像操作 |
//! | `CGEND` | 1,183 | 删层/删组(递归) |
//! | `FONT` | 3 | 字体属性快照 |
//! | `CGINFO`/`FONTINFO` | 606/1 | 只读查询,归 VM;gfx 只挂号 |
//!
//! 其余 `CG` 参数(`LINT/TSX/FX/...`)原样进入 [`Layer::extra`]/
//! [`crate::layer::FilterTransform`],并挂 `Unimplemented` 诊断;不静默丢。
//!
//! # 错误容错
//!
//! 引擎绘制函数失败会中止本帧(旧线 01 §5),但**状态更新失败**在脚本侧是
//! 可恢复的(手册 `cg/cg.html` 例 4 只声明“图层不存在 = 错误”)。本状态机
//! 对单条命令失败采用 `Recoverable` 诊断 + 跳过该命令,不终止整帧;依赖
//! 未取证语义的图层用 `LayerStatus::Unimplemented` 标记,由后端跳过。

use std::collections::{BTreeMap, BTreeSet, HashSet};

use yu_log::Diag;
use yu_rt::event::{ArgValue, Domain, VmEvent};
use yu_rt::tables::TableSet;
use yu_rt::value::Value;

use crate::error;
use crate::frame::{
    probe_order_from_codes, Clear, ColorSpace, CompositorFrame, TextureFilter, Viewport,
    WindowState,
};
use crate::layer::{
    BlendConfidence, BlendMode, CopyId, EngineValue, FilterParam, ImageFormat, Layer, LayerId,
    LayerKind, LayerStatus, SizeU32, TextureRef, Transition,
};
use crate::op::{
    CopyKind, CopyOp, CopyRegion, CopySource, CopyTiming, ImageAction, ImageOp, MaskDraw, Op,
    OpStatus,
};
use crate::pass::{Pass, PassTarget};
use crate::path;
use crate::table::ParamTable;
use crate::text::{
    layout_text, TextDraw, TextDrawKey, TextFlagsSource, TextLayoutFrame, TextRun, TextStyle,
    MAX_TEXT_DRAWS_PER_LAYER,
};
use crate::transition::TransitionDraw;

/// 已解析参数(名字/值都取自 `self.params`,先复制出来以释放不可变借用)。
struct OwnedArg {
    name: Option<String>,
    slot: u8,
    value: Value,
}

/// 图形域配置(视口/清屏/过滤/扩展名顺序/主窗口图层组)。
#[derive(Clone, Debug)]
pub struct GfxConfig {
    pub viewport: Viewport,
    pub clear: Clear,
    pub filter: TextureFilter,
    /// 资产扩展名探测序。
    ///
    /// **A5**:不设内置默认值。`GfxState::from_tables` 从工程 `yscfg` 装载;
    /// `yscfg` 为空且这里也为空时**拒绝构造**(`gfx.config.probe_order_missing`)。
    /// 宿主若确有独立证据,可显式填本字段(显式注入不是兜底)。
    pub image_probe_order: Vec<ImageFormat>,
    /// 引擎 `@_DXMODE` 的宿主注入值(0 = 固定管线)。见
    /// [`crate::CompositorFrame::dx_mode`] 的范围说明。
    pub dx_mode: u32,
    /// 主窗口(`WINDOW[NO=0]`)显示的图层组。手册 `window/window.html`:
    /// 「NO=0 がメインウィンドウ、１～３がサブウィンドウ」「CGID=表示対象の
    /// レイヤセット」。默认 `:/W0`。
    pub main_layer_set: LayerId,
}

impl GfxConfig {
    pub fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            clear: Clear::default_black(),
            filter: TextureFilter::unknown(),
            // A5:未装载前为空;`from_tables` 负责从 `yscfg` 填充并校验。
            image_probe_order: Vec::new(),
            dx_mode: 0,
            main_layer_set: LayerId::new(b":/W0".to_vec()),
        }
    }

    /// 宿主注入 `_DXMODE`(链式构造)。
    pub fn with_dx_mode(mut self, dx_mode: u32) -> Self {
        self.dx_mode = dx_mode;
        self
    }
}

/// 状态生产日志的硬上限。`image_log` 可能因同一层反复加工而增长;
/// 超限时丢弃最旧项并计数,避免内存无界(行为在诊断里显式可见)。
const MAX_OP_LOG: usize = 16_384;

struct Node {
    layer: Layer,
    parent: Option<LayerId>,
    children: BTreeSet<LayerId>,
}

/// 一个图层的持久 `CGACT[TEXT]` 绘制项(DIV-GFX-058)。
///
/// `seen` 是命令时刻的精确去重集:官方引擎重复命令会重复合成
/// (`FONT[DRAWMODE=1]` 下逐次加深),而脚本(对话逐字路径)每帧重发
/// 同一集合,重放时必须只保留一次。超限丢最旧并显式计数。
#[derive(Clone, Debug, Default)]
struct LayerTexts {
    draws: Vec<TextDraw>,
    seen: HashSet<TextDrawKey>,
    dropped: u64,
    /// 文本内容修订号:每次**实际**改动 `draws`(新绘制项/上限丢弃/交换)
    /// 递增。后端据此缓存「基图 + 文本」合成结果,内容不变不重合成
    /// (P8 性能契约:持久文本不能变成每帧全量重烤)。
    revision: u64,
}

impl LayerTexts {
    /// 追加一条绘制项;重复项按 [`TextDraw::dedup_key`] 丢弃。
    /// 超限时丢最旧一条并把 `dropped` 加一(诊断由调用方汇总)。
    fn push(&mut self, draw: TextDraw) -> bool {
        let key = draw.dedup_key();
        if !self.seen.insert(key) {
            return false;
        }
        if self.draws.len() >= MAX_TEXT_DRAWS_PER_LAYER {
            let oldest = self.draws.remove(0);
            self.seen.remove(&oldest.dedup_key());
            self.dropped += 1;
        }
        self.draws.push(draw);
        true
    }
}

/// 图形域状态机。
pub struct GfxState {
    params: ParamTable,
    config: GfxConfig,
    nodes: BTreeMap<LayerId, Node>,
    seq: u64,
    copy_seq: u64,
    /// COPY + 图像加工的共同命令序(EVID-T-15;`frame()` 交错输出)。
    texture_seq: u64,
    frame: u64,
    style: TextStyle,
    /// 跨帧文本缓冲(按 `TEXT[ID]`;引擎只有一个文本状态,实际最多一条)。
    ///
    /// DIV-GFX-034:不再每帧清空;清空只由 `TEXT[CLEAR]`(经
    /// [`GfxState::sync_text_state`])、[`GfxState::clear_text`]、目标层被
    /// `CGEND`/`CG` 重建触发。
    text_layers: BTreeMap<LayerId, TextBuffer>,
    /// 最近一次 `TEXT[ID]`(文本事件落点)。
    current_text: LayerId,
    /// 文本修订号发号器(全局单调;后端合成缓存键必须全局唯一,
    /// 不能因层重建/内容清除而复用旧号)。
    text_revision_seq: u64,
    /// `CGACT[TEXT=1]` 绘制项(按目标图层,**跨帧持久**)。
    ///
    /// DIV-GFX-058(P8):官方把字符即时写进图层表面,画一次即常驻
    /// (按钮标签/存读档行文本/ES.SCREENSHOT 均只画一次);对话逐字路径
    /// 每帧重发同集合,经 [`LayerTexts`] 精确去重后可见结果不变
    /// (DIV-GFX-055 预登记的演进)。清除只发生在层像素被重写时:
    /// `CGEND`/`CG(FILE)`/`CGACT[COPY]` 目标/Executable 图像加工/`SWAP`。
    text_draws: BTreeMap<LayerId, LayerTexts>,
    copy_log: Vec<CopyOp>,
    image_log: Vec<ImageOp>,
    /// 被上限丢弃的最旧操作数(审计用)。
    copy_log_dropped: u64,
    image_log_dropped: u64,
    /// 本帧诊断(按 `(code, command, detail)` 有序去重;`FrameStart` 清空)。
    diags: BTreeMap<(String, String, String), error::FrameDiag>,
    /// 窗口状态(按 `NO`;0 = 主窗口)。
    windows: BTreeMap<u8, WindowState>,
    /// 资产像素尺寸缓存(键 = `CG[FILE]` 原始字节)。
    ///
    /// 由 VM 在 `CGINFO[SX/SY]` 回退路径解析图像头后注入(DIV-GFX-024):
    /// gfx 自己不做文件 I/O,但 `CGINFO` 必须与引擎一样在**脚本执行期**
    /// 回答尺寸,否则精灵脚本的 `SX/SY` 布局全为 0(EVID-J3-03)。
    asset_sizes: BTreeMap<Vec<u8>, SizeU32>,
    /// 已安装字体族清单(CP932 字节,序号 = `FONTINFO[NO]`,1 基)。
    ///
    /// 宿主注入(桌面/serve 用 fontdb 的 family 列表;与渲染器实际可解析的
    /// 同一来源)。手册 `font/fontinfo.html` 只描述 NUM/LANG/NAME 的存在;
    /// 引擎语义 = `EnumFontFamiliesExA` 枚举(手册无页,2026-10-24 政策按
    /// REA/引擎证据实现):`NUM` = 条数;`NAME NO=n` = 第 n 项名字(越界 =
    /// 空串,不报错);`LANG NO=n` = 字符集——本宿主清单统一按 0 应答
    /// (`es_Font.yst:166` 以 `IF[@mFontLang] LOOPCONTINUE` 过滤非 0 项,
    /// 0 = 保留该字体)。未注入时 NUM=0/NAME=空串,脚本走「无字体可选」
    /// 路径(不再挂号;DIV-GFX-036 的宿主清单部分由此收口)。
    font_families: Vec<Vec<u8>>,
    /// `COPY/COPY2` 生成的快照纹理尺寸(创建时从源层继承)。
    snapshot_sizes: BTreeMap<CopyId, SizeU32>,
    /// 快照对应的源资产键(`CGINFO[MFILE]` 透传)。
    snapshot_files: BTreeMap<CopyId, Vec<u8>>,
    /// 当前光标逻辑坐标(`CGINFO[ONMOUSE*]` 的缺省查询点;VM 每帧推入)。
    pointer: (i64, i64),
}

/// [`GfxState::text_state`] 的返回视图(字节 / `_TEXT_FLAG` / 样式 / 原点)。
pub type TextView<'a> = (&'a [u8], Option<&'a [u8]>, &'a TextStyle, (i64, i64));

/// 一个文本层的跨帧缓冲(`$_TEXT` / `$_TEXT_FLAG` 的 gfx 镜像)。
#[derive(Clone, Debug, Default)]
struct TextBuffer {
    bytes: Vec<u8>,
    /// `_TEXT_FLAG` 控制字节流;`None` = 宿主尚未同步(排版不可用)。
    flags: Option<Vec<u8>>,
    /// 字体快照(最近一次 `FONT`)。
    style: TextStyle,
    /// 文本层局部原点(引擎光标 `+0x14/+0x18`)。
    origin: (i64, i64),
    /// 最近写入帧号。
    frame: u64,
    /// 当前应显示字符数(`None` = 全部;H-P2-02 前由宿主注入)。
    reveal: Option<usize>,
}

/// `CGINFO[ONMOUSE/ONMOUSE2/ONMOUSE3]` 需要的像素采样面。
///
/// 引擎直接读 D3D 表面(`FUN_0043d664`);内核的 gfx 层没有文件/纹理 I/O,
/// 由持有 `TextureStore` 的宿主注入(soft/wgpu 共用同一解码器)。
pub trait TextureSampler {
    /// 纹理自然尺寸(像素)。
    fn size(&mut self, texture: &TextureRef) -> Option<SizeU32>;
    /// 取纹理像素(越界/不可采样 → `None`)。
    fn sample(&mut self, texture: &TextureRef, x: i64, y: i64) -> Option<[u8; 4]>;
}

impl GfxState {
    /// 从工程表构造(命令表 = 参数名唯一来源)。
    ///
    /// 视口优先取工程 `yscfg` 的宽高(手册/引擎设定工具写入);`0` 表示
    /// 引擎未设置,此时保留调用方 `GfxConfig` 的值。
    pub fn from_tables(tables: &TableSet, mut config: GfxConfig) -> Result<Self, Diag> {
        let params = ParamTable::from_tables(tables);
        params.validate()?;
        let order = probe_order_from_codes(&tables.boot.config.image_ext_order);
        // A5:缺表不得静默兜底。
        // - `yscfg` 给出非空序 → 用它(引擎设置优先);
        // - `yscfg` 为空、宿主显式注入了序 → 用宿主的(显式注入,不是内置表);
        // - 两者都为空 → 拒绝构造,让宿主显式给值。
        if !order.is_empty() {
            config.image_probe_order = order;
        }
        if config.image_probe_order.is_empty() {
            return Err(error::fatal(
                error::code::CONFIG_PROBE_ORDER_MISSING,
                "工程 yscfg 的 image_ext_order 为空,且宿主未显式注入探测序;按 A5 拒绝启动(禁止内置兜底表)",
            )
            .at_table("yscfg"));
        }
        let cfg = &tables.boot.config;
        if cfg.width != 0 && cfg.height != 0 {
            config.viewport = Viewport::new(cfg.width, cfg.height);
        }
        Ok(Self::new(params, config))
    }

    pub fn new(params: ParamTable, config: GfxConfig) -> Self {
        let mut windows = BTreeMap::new();
        windows.insert(0, WindowState::new(0, Some(config.main_layer_set.clone())));
        let mut s = Self {
            params,
            config,
            nodes: BTreeMap::new(),
            seq: 0,
            copy_seq: 0,
            texture_seq: 0,
            frame: 0,
            style: TextStyle::default(),
            text_layers: BTreeMap::new(),
            current_text: LayerId::new(Vec::new()),
            text_revision_seq: 0,
            text_draws: BTreeMap::new(),
            copy_log: Vec::new(),
            image_log: Vec::new(),
            copy_log_dropped: 0,
            image_log_dropped: 0,
            diags: BTreeMap::new(),
            windows,
            asset_sizes: BTreeMap::new(),
            font_families: Vec::new(),
            snapshot_sizes: BTreeMap::new(),
            snapshot_files: BTreeMap::new(),
            pointer: (0, 0),
        };
        s.ensure_w0();
        s
    }

    pub fn config(&self) -> &GfxConfig {
        &self.config
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.config.viewport = viewport;
        if let Some(w0) = self.nodes.get_mut(&Self::w0()) {
            w0.layer.kind = LayerKind::Group {
                size: Some(viewport_to_size(viewport)),
            };
        }
    }

    pub fn set_filter(&mut self, filter: TextureFilter) {
        self.config.filter = filter;
    }

    /// 宿主注入 `@_DXMODE`(0 = 固定管线;非 0 = 引擎 HLSL 转场路径)。
    pub fn set_dx_mode(&mut self, dx_mode: u32) {
        self.config.dx_mode = dx_mode;
    }

    /// 注入资产像素尺寸(VM 解析图像头后调用;键为 `CG[FILE]` 原字节)。
    pub fn set_asset_size(&mut self, key: &[u8], size: SizeU32) {
        self.asset_sizes.insert(key.to_vec(), size);
    }

    /// 注入已安装字体族清单(见 [`Self::font_families`] 字段注释)。
    pub fn set_font_families(&mut self, families: Vec<Vec<u8>>) {
        self.font_families = families;
    }

    /// 当前光标逻辑坐标(VM 每帧推入;`CGINFO[ONMOUSE*]` 缺省查询点)。
    pub fn pointer(&self) -> (i64, i64) {
        self.pointer
    }

    pub fn set_pointer(&mut self, x: i64, y: i64) {
        self.pointer = (x, y);
    }

    /// 查询已注入的资产尺寸。
    pub fn asset_size(&self, key: &[u8]) -> Option<SizeU32> {
        self.asset_sizes.get(key).copied()
    }

    pub fn layer(&self, id: &LayerId) -> Option<&Layer> {
        self.nodes.get(id).map(|n| &n.layer)
    }

    /// 本帧诊断(按 `(code, command, detail)` 升序;重复项合并为 `count`)。
    pub fn diagnostics(&self) -> Vec<error::FrameDiag> {
        self.diags.values().cloned().collect()
    }

    /// 被硬上限丢弃的最旧复制/图像操作数(审计;正常游戏为 0)。
    pub fn dropped_ops(&self) -> (u64, u64) {
        (self.copy_log_dropped, self.image_log_dropped)
    }

    /// 纹理复制日志(状态生产;`frame()` 只输出仍被现存图层引用的项)。
    pub fn copies(&self) -> &[CopyOp] {
        &self.copy_log
    }

    /// 图像加工日志。
    pub fn images(&self) -> &[ImageOp] {
        &self.image_log
    }

    /// 用 VM 的文本状态快照**替换**当前文本层(权威字节 + `_TEXT_FLAG`)。
    ///
    /// 这是 `_TEXT_FLAG` 逐字 `'1'/'2'` 流进入 gfx 的唯一入口(契约见
    /// `yu_rt::Vm::text_state` 的“宿主/gfx 桥”注释)。`TEXT[CLEAR]` 在 VM 里
    /// 没有事件,宿主每帧(或至少在 `TEXT` 命令后)调用本方法即可同步;
    /// `bytes` 为空即等于 CLEAR(DIV-GFX-034 的传播路径)。
    ///
    /// 标志流与字节不对齐时**不猜**:挂 `gfx.text.flag_mismatch` 诊断并把
    /// 标志置 `None`(后端不排版)。
    pub fn sync_text_state(&mut self, snapshot: &yu_rt::text::TextSnapshot) {
        let id = self.current_text.clone();
        self.write_text_state(&id, &snapshot.bytes, &snapshot.flags);
        let buf = self.text_layers.entry(id).or_default();
        buf.origin = (snapshot.cursor_x, snapshot.cursor_y);
    }

    /// 同上,但显式指定文本层(宿主若自行维护层 ID)。
    pub fn sync_text_state_for(&mut self, id: &LayerId, snapshot: &yu_rt::text::TextSnapshot) {
        self.write_text_state(id, &snapshot.bytes, &snapshot.flags);
        let buf = self.text_layers.entry(id.clone()).or_default();
        buf.origin = (snapshot.cursor_x, snapshot.cursor_y);
    }

    fn write_text_state(&mut self, id: &LayerId, bytes: &[u8], flags: &[u8]) {
        let mismatch = layout_text(bytes, flags, (0, 0), &self.style).is_none();
        let buf = self.text_layers.entry(id.clone()).or_default();
        buf.bytes = bytes.to_vec();
        buf.flags = if mismatch { None } else { Some(flags.to_vec()) };
        buf.style = self.style.clone();
        buf.frame = self.frame;
        if mismatch {
            self.push_diag(
                "TEXT",
                error::unimplemented(
                    error::code::TEXT_UNSUPPORTED,
                    "`_TEXT_FLAG` 与文本字节不对齐;按未同步处理(不猜排版)",
                ),
            );
        }
    }

    /// 清空当前文本层(`TEXT[CLEAR]` 的显式入口;等同 VM 清空后的同步)。
    pub fn clear_text(&mut self) {
        let id = self.current_text.clone();
        if let Some(buf) = self.text_layers.get_mut(&id) {
            buf.bytes.clear();
            buf.flags = Some(Vec::new());
        }
    }

    /// 设置当前文本层应显示的字符数(`None` = 全部;M-D 的渲染截断入口)。
    pub fn set_text_reveal(&mut self, chars: Option<usize>) {
        let id = self.current_text.clone();
        if let Some(buf) = self.text_layers.get_mut(&id) {
            buf.reveal = chars;
        }
    }

    /// 当前文本层的原始状态(`bytes`, `_TEXT_FLAG`, style, origin)。
    pub fn text_state(&self) -> TextView<'_> {
        let buf = self.text_layers.get(&self.current_text);
        match buf {
            Some(b) => (&b.bytes, b.flags.as_deref(), &b.style, b.origin),
            None => (&[], None, &self.style, (0, 0)),
        }
    }

    /// 消费一批事件。调用方应每帧 `drain_events` 后喂入,避免有界队列丢弃。
    pub fn consume_events(&mut self, events: &[VmEvent]) {
        for ev in events {
            self.on_event(ev);
        }
    }

    pub fn on_event(&mut self, ev: &VmEvent) {
        match ev {
            VmEvent::FrameStart { frame } => {
                self.frame = *frame;
                // DIV-GFX-034:文本缓冲**跨帧持久**。清空只发生在
                // `TEXT[CLEAR]`(宿主经 `sync_text_state` 同步)、
                // `clear_text()` 或目标层被 CGEND/CG 重建时。
                // DIV-GFX-058:`CGACT[TEXT]` 绘制项同样跨帧持久(官方写层
                // 表面,画一次即常驻);`FrameStart` 不再清空。
                // 诊断是“本帧”诊断:每帧清空,防止跨帧无界增长。
                self.diags.clear();
                // 复制/图像操作是**状态生产**(快照/纹理修改),不是逐帧易失;
                // 保留日志,frame() 只输出仍被现存图层引用的项。
            }
            VmEvent::FrameEnd { frame } => {
                self.frame = *frame;
            }
            VmEvent::Text { frame, id, bytes } => {
                self.frame = *frame;
                let id = LayerId::new(id.as_bytes().to_vec());
                self.current_text = id.clone();
                let buf = self.text_layers.entry(id).or_default();
                buf.bytes.extend_from_slice(bytes);
                // 事件只带字节;`_TEXT_FLAG` 必须由宿主用
                // `Vm::text_state()` 同步(`sync_text_state`)。追加后旧标志流
                // 不再覆盖新字节 ⇒ 置 None,禁止后端猜宽度。
                buf.flags = None;
                buf.style = self.style.clone();
                buf.frame = *frame;
            }
            VmEvent::DomainCall {
                frame,
                domain,
                name,
                args,
                ..
            } => {
                self.frame = *frame;
                match domain {
                    Domain::Graphics => self.dispatch(name, args),
                    Domain::Window => self.dispatch_window(name, args),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// 构建当前帧的合成契约(只读;可重复调用,字节稳定)。
    pub fn frame(&self) -> CompositorFrame {
        let mut f = CompositorFrame::new(self.frame, self.config.viewport);
        f.dx_mode = self.config.dx_mode;
        f.clear = self.config.clear;
        f.filter = self.config.filter;
        f.image_probe_order = self.config.image_probe_order.clone();
        f.color_space = ColorSpace::GammaEncodedSrgb8;

        let mut passes = Vec::new();

        // 1. 纹理复制/图像加工:按**命令执行序**交错输出(EVID-T-15;
        //    官方处理器在命令到达时立即执行,COPY 看到的是此前加工后的纹理)。
        //
        // 可达性:
        // - 复制目标必须仍引用该快照(否则快照无人可见);
        // - 源层仍存在 → 按层自合成;源层已被 `CGEND` 删除 → 用命令时刻
        //   记录的 `source_texture` 重放(`DIV-GFX-053`:ERIS 按钮快照的
        //   源 `BT.OFF.TMP` 在同一批命令末尾被删,官方复制已经完成)。
        //   两者都不可得才丢弃,完整历史留在 `GfxState::copies()` 供审计。
        let mut texture_ops: Vec<(u64, Op)> = Vec::new();
        for c in &self.copy_log {
            let dest_live = self
                .nodes
                .get(&c.dest)
                .is_some_and(|n| matches!(n.layer.texture, TextureRef::Snapshot(id) if id == c.id));
            if !dest_live {
                continue;
            }
            let source_live = match &c.source {
                CopySource::Layer(id) | CopySource::Group(id) => self.nodes.contains_key(id),
            };
            if c.status == OpStatus::Executable && !source_live && c.source_texture.is_none() {
                continue;
            }
            texture_ops.push((c.seq, Op::Copy(c.clone())));
        }
        for i in &self.image_log {
            let source_live = self.nodes.contains_key(&i.source);
            let target_live = i.target.as_ref().is_none_or(|t| self.nodes.contains_key(t));
            if source_live && target_live {
                texture_ops.push((i.seq, Op::Image(i.clone())));
            }
        }
        texture_ops.sort_by_key(|(seq, _)| *seq);
        for (_, op) in texture_ops {
            let target = match &op {
                Op::Copy(c) => c.dest.clone(),
                Op::Image(i) => i.target.clone().unwrap_or_else(|| i.source.clone()),
                _ => unreachable!("texture ops are copy/image"),
            };
            passes.push(Pass {
                target: PassTarget::Texture(target),
                ops: vec![op],
                clear: None,
            });
        }

        // 2. 组离屏 pass(后序;子组先于父组;显式栈,深树不爆栈)。
        let mut emitted_groups = BTreeSet::new();
        self.build_group_passes(&mut passes, &mut emitted_groups);

        // 3. 屏幕 pass(最后)。
        //
        // 手册 window/window.html:NO=0 是主窗口,CGID 指定其显示的图层组;
        // 1..3 是子窗口。gfx 只把**主窗口图层组**合成到屏幕;其余根组仍产出
        // 离屏 pass,交给后续 window/host 会话组合。
        let mut screen_ops = Vec::new();
        if self.windows.get(&0).is_none_or(|w| w.visible) {
            let main = self
                .windows
                .get(&0)
                .and_then(|w| w.layer_set.clone())
                .unwrap_or_else(|| self.config.main_layer_set.clone());
            // 显式主组缺失时不做“合成所有顶层”的猜测(D15)。
            //
            // 主组**直绘**(DIV-GFX-008/041,Confirmed):引擎主窗口没有离屏
            // RT,组的合成结果直接写后缓冲;组的 `A` 不会作为整幅 alpha 再乘
            // 一次。若先合成到 RT 再按 RT alpha 贴回,淡入淡出类效果会被
            // 额外压暗(实测 CGS100 淡出 100→77,EVID-J3-07)。
            // 主组是图层组 → 子层直绘;主组被显式指定为单个图像层 →
            // 直接画该层(手册 `WINDOW[CGID]` 允许指定任意图层)。
            if self.nodes.get(&main).is_some_and(|n| n.layer.is_group()) {
                self.emit_children_ops(&main, &mut screen_ops, &emitted_groups);
            } else {
                self.emit_node_ops(&main, &mut screen_ops, &emitted_groups);
            }
        }
        passes.push(Pass {
            target: PassTarget::Screen,
            ops: screen_ops,
            clear: Some(self.config.clear),
        });

        f.passes = passes;
        f.windows = self.windows.values().cloned().collect();
        f.layers = self
            .nodes
            .values()
            .map(|n| n.layer.clone())
            .collect::<Vec<_>>();
        // 比较期不分配:直接比较借用字段。
        f.layers
            .sort_by(|a, b| (a.z, a.seq, &a.id).cmp(&(b.z, b.seq, &b.id)));
        f.text = self
            .text_layers
            .iter()
            .filter(|(_, b)| !b.bytes.is_empty())
            .map(|(id, b)| TextRun {
                id: id.clone(),
                bytes: b.bytes.clone(),
                flags: b.flags.clone(),
                style: b.style.clone(),
                origin: Some(b.origin),
                reveal: b.reveal,
                atlas: None,
                frame: b.frame,
            })
            .collect();
        // 自动文本层的排版:只接受 `_TEXT_FLAG` 一手证据;缺失 ⇒ 不产出。
        f.text_layout = self.text_layers.iter().find_map(|(id, b)| {
            if b.bytes.is_empty() {
                return None;
            }
            let flags = b.flags.as_deref()?;
            let glyphs = layout_text(&b.bytes, flags, b.origin, &b.style)?;
            if glyphs.is_empty() {
                return None;
            }
            let target = if !id.0.is_empty() && self.nodes.contains_key(id) {
                Some(id.clone())
            } else {
                None
            };
            Some(TextLayoutFrame {
                target,
                origin: b.origin,
                style: b.style.clone(),
                chars: glyphs.len(),
                reveal: b.reveal,
                line_height: b.style.line_height(),
                glyphs,
                flags_source: TextFlagsSource::TextFlag,
            })
        });
        // `CGACT[TEXT]` 绘制项(按目标层 ID 稳定排序;层内保持命令序;
        // 跨帧持久,DIV-GFX-058)。
        f.text_draws = self
            .text_draws
            .values()
            .flat_map(|t| t.draws.iter().cloned())
            .collect();
        // 每层文本修订号(后端合成缓存键;内容不变不重烤)。
        f.text_revisions = self
            .text_draws
            .iter()
            .map(|(id, t)| (id.clone(), t.revision))
            .collect();
        f.diagnostics = self.diags.values().cloned().collect();
        f
    }

    // ------------------------------------------------------------ INFO 查询

    /// 回答 `CGINFO/FONTINFO/WINDOWINFO` 的只读查询(供 `GraphicsHost` 同步调用)。
    ///
    /// 只实现手册明确、且当前状态足以回答的字段;需要像素/鼠标/字体列表/
    /// 资产元数据的字段返回 `Unimplemented`,由 VM **不写** `LET`(不猜 0)。
    /// 多字段同时置位时顺序未取证,返回 `gfx.info.multi`。
    pub fn query_info(&self, command: &str, args: &[ArgValue]) -> Result<Value, Diag> {
        self.query_info_with(command, args, None)
    }

    /// 同 [`GfxState::query_info`];额外提供纹理采样器(ONMOUSE/COLOR)。
    pub fn query_info_with(
        &self,
        command: &str,
        args: &[ArgValue],
        sampler: Option<&mut dyn TextureSampler>,
    ) -> Result<Value, Diag> {
        match command {
            "CGINFO" => self.query_cginfo(args, sampler),
            "FONTINFO" => self.query_fontinfo(args),
            "WINDOWINFO" => self.query_windowinfo(args),
            other => Err(error::unimplemented(
                error::code::INFO_UNSUPPORTED,
                format!("未知 INFO 命令 `{other}`"),
            )),
        }
    }

    fn query_cginfo(
        &self,
        args: &[ArgValue],
        sampler: Option<&mut dyn TextureSampler>,
    ) -> Result<Value, Diag> {
        let resolved = self.params.resolve("CGINFO", args);
        let mut id: Option<Vec<u8>> = None;
        let mut idno: Option<i64> = None;
        let mut flags: BTreeMap<&str, i64> = BTreeMap::new();
        // `SET/SET2` 是查询坐标(COLOR/ONMOUSE*),不是“查询字段”。
        let mut set: Option<i64> = None;
        let mut set2: Option<i64> = None;
        for r in resolved {
            match r.name {
                Some("ID") => match r.value {
                    Value::Str(b) => id = Some(b.clone()),
                    _ => return Err(arg_type_diag("CGINFO", "ID", "字符串")),
                },
                Some("IDNO") => match int_arg(r.value) {
                    Some(n) => idno = Some(n),
                    None => return Err(arg_type_diag("CGINFO", "IDNO", "整数")),
                },
                Some("LET") => {}
                Some("SET") => {
                    set = Some(
                        int_arg(r.value).ok_or_else(|| arg_type_diag("CGINFO", "SET", "整数"))?,
                    );
                }
                Some("SET2") => {
                    set2 = Some(
                        int_arg(r.value).ok_or_else(|| arg_type_diag("CGINFO", "SET2", "整数"))?,
                    );
                }
                Some(name) => {
                    let Some(v) = int_arg(r.value) else {
                        return Err(arg_type_diag("CGINFO", name, "整数"));
                    };
                    if v != 0 {
                        flags.insert(name, v);
                    }
                }
                None => {
                    return Err(error::unimplemented(
                        error::code::ARG_MISSING,
                        format!("CGINFO 槽 {} 不在 ysc 参数表内", r.slot),
                    ));
                }
            }
        }
        let Some((flag, _)) = single_flag(&flags, "CGINFO")? else {
            return Err(error::unimplemented(
                error::code::INFO_MULTI,
                "CGINFO 未给出查询字段",
            ));
        };
        let Some(id) = id else {
            return Err(error::recoverable(error::code::ARG_MISSING, "CGINFO 缺 ID"));
        };
        let parsed = path::normalize(&id, idno, id.ends_with(b"/"));
        if flag == "EXIST" {
            return Ok(Value::Int(i64::from(self.nodes.contains_key(&parsed.path))));
        }
        let Some(node) = self.nodes.get(&parsed.path) else {
            return Err(error::unimplemented(
                error::code::INFO_MISSING,
                format!("CGINFO[{}] 目标 `{}` 不存在", flag, parsed.path.display()),
            ));
        };
        let layer = &node.layer;
        Ok(match flag {
            "E" => Value::Int(i64::from(layer.visible)),
            "A" => Value::Int(i64::from(layer.alpha)),
            "X" => Value::Int(layer.transform.x),
            "Y" => Value::Int(layer.transform.y),
            "Z" => Value::Int(layer.z),
            "MODE" => Value::Int(i64::from(layer.blend.code())),
            "T" => Value::Int(i64::from(layer.transition.as_ref().map_or(256, |t| t.t))),
            "TA" => Value::Int(i64::from(layer.transition.as_ref().map_or(0, |t| t.ta))),
            "TID" => Value::Str(
                layer
                    .transition
                    .as_ref()
                    .and_then(|t| t.tid_raw.clone())
                    .unwrap_or_default(),
            ),
            "MFILE" => Value::Str(match &layer.texture {
                TextureRef::Asset(key) => key.clone(),
                TextureRef::Snapshot(id) => {
                    self.snapshot_files.get(id).cloned().unwrap_or_default()
                }
                _ => Vec::new(),
            }),
            "SX" => Value::Int(self.query_layer_size(layer, true)?),
            "SY" => Value::Int(self.query_layer_size(layer, false)?),
            "ONMOUSE" | "ONMOUSE2" | "ONMOUSE3" => {
                let sampler = sampler.ok_or_else(|| {
                    error::unimplemented(
                        error::code::INFO_UNSUPPORTED,
                        format!("CGINFO[{flag}] 需要纹理采样器(宿主未注入)"),
                    )
                })?;
                self.query_onmouse(layer, flag, set, set2, sampler)?
            }
            other => {
                // COLOR/TEX/LINT 等无手册语义或未取证。
                return Err(error::unimplemented(
                    error::code::INFO_UNSUPPORTED,
                    format!("CGINFO[{other}] 需要像素/未分类语义,未取证"),
                ));
            }
        })
    }

    /// `CGINFO[ONMOUSE/ONMOUSE2/ONMOUSE3]`(REA 0.495 `0x43de01/0x43dec8/0x43df7f`)。
    ///
    /// 引擎只比较**已加载表面**的部件矩形与鼠标坐标,然后按行跨读取像素:
    /// - `ONMOUSE`  → `r | g<<8 | b<<16`(0..0xFFFFFF);
    /// - `ONMOUSE2` → `a`(0..255);
    /// - `ONMOUSE3` → `r | g<<8 | b<<16 | a<<24`(32 位结果,按 i32 符号扩展);
    /// - 不在任何部件内 / 越界 → `-1`。
    ///
    /// 本内核没有引擎的“部件链表”,图像层按单部件处理:矩形 =
    /// `(X, Y, 显示宽, 显示高)`;显示尺寸与纹理尺寸不同时按最近邻反算纹理坐标。
    fn query_onmouse(
        &self,
        layer: &Layer,
        flag: &str,
        set: Option<i64>,
        set2: Option<i64>,
        sampler: &mut dyn TextureSampler,
    ) -> Result<Value, Diag> {
        if matches!(layer.kind, LayerKind::Group { .. }) {
            return Err(error::unimplemented(
                error::code::INFO_UNSUPPORTED,
                "CGINFO[ONMOUSE*] 查询图层组:组无单一表面,未取证",
            ));
        }
        // `COPY/COPY2` 快照层没有独立像素面可采样;若快照源是资产
        // (`snapshot_files`),按源资产采样——按钮系统正是这样缓存
        // `BT.OFF...` 图层的(REA:引擎读的是已复制表面,像素同源)。
        let texture = match &layer.texture {
            TextureRef::Snapshot(id) => self
                .snapshot_files
                .get(id)
                .map(|key| TextureRef::Asset(key.clone()))
                .unwrap_or_else(|| layer.texture.clone()),
            other => other.clone(),
        };
        let texture_size = sampler.size(&texture);
        let display = layer.image_size().or(texture_size);
        let Some(display) = display else {
            return Err(error::unimplemented(
                error::code::INFO_ASSET_SIZE,
                "CGINFO[ONMOUSE*] 图层尺寸未知(无 SX/SY 且无纹理)",
            ));
        };
        if display.width == 0 || display.height == 0 {
            return Ok(Value::Int(-1));
        }
        let (mut mx, mut my) = self.pointer;
        if let Some(v) = set {
            mx = v;
        }
        if let Some(v) = set2 {
            my = v;
        }
        let x0 = layer.transform.x;
        let y0 = layer.transform.y;
        let lx = mx - x0;
        let ly = my - y0;
        if lx < 0 || ly < 0 || lx >= i64::from(display.width) || ly >= i64::from(display.height) {
            return Ok(Value::Int(-1));
        }
        // 显示尺寸 → 纹理坐标(最近邻,与后端采样口径一致)。
        let (tx, ty) = match texture_size {
            Some(size) if size.width > 0 && size.height > 0 => {
                let tx = lx * i64::from(size.width) / i64::from(display.width);
                let ty = ly * i64::from(size.height) / i64::from(display.height);
                (tx, ty)
            }
            _ => (lx, ly),
        };
        let Some(px) = sampler.sample(&texture, tx, ty) else {
            return Ok(Value::Int(-1));
        };
        let rgb = i64::from(px[0]) | (i64::from(px[1]) << 8) | (i64::from(px[2]) << 16);
        Ok(Value::Int(match flag {
            "ONMOUSE2" => i64::from(px[3]),
            "ONMOUSE3" => i64::from((rgb as i32) | (i64::from(px[3]) as i32) << 24),
            // `rgb` 是 24 位无符号;引擎的 32 位结果对 ONMOUSE 不会溢出符号位。
            _ => rgb,
        }))
    }

    fn query_layer_size(&self, layer: &Layer, width: bool) -> Result<i64, Diag> {
        if let Some(s) = layer.group_size().or_else(|| layer.image_size()) {
            return Ok(i64::from(if width { s.width } else { s.height }));
        }
        match &layer.texture {
            TextureRef::SolidBlack1x1 => Ok(1),
            TextureRef::None => Err(error::unimplemented(
                error::code::INFO_UNSUPPORTED,
                "无纹理层的尺寸未知",
            )),
            // 尺寸由 VM 解析资产头后注入(见 `set_asset_size`);未注入时
            // 仍返回专用错误码,让 VM 走回退路径(DIV-GFX-024)。
            TextureRef::Asset(key) => match self.asset_sizes.get(key) {
                Some(size) => Ok(i64::from(if width { size.width } else { size.height })),
                None => Err(error::unimplemented(
                    error::code::INFO_ASSET_SIZE,
                    "资源图尺寸尚未由资产元数据提供者注入(TODO DIV-GFX-024)",
                )),
            },
            TextureRef::Snapshot(id) => match self.snapshot_sizes.get(id) {
                Some(size) => Ok(i64::from(if width { size.width } else { size.height })),
                None => Err(error::unimplemented(
                    error::code::INFO_UNSUPPORTED,
                    format!("快照纹理 {} 尺寸未记录(源层无尺寸元数据)", id.0),
                )),
            },
            TextureRef::Offscreen(id) => {
                let Some(n) = self.nodes.get(id) else {
                    return Err(error::unimplemented(
                        error::code::INFO_MISSING,
                        format!("离屏组 `{}` 不存在", id.display()),
                    ));
                };
                let Some(s) = n.layer.group_size() else {
                    return Err(error::unimplemented(
                        error::code::INFO_UNSUPPORTED,
                        "离屏组尺寸未指定",
                    ));
                };
                Ok(i64::from(if width { s.width } else { s.height }))
            }
        }
    }

    fn query_fontinfo(&self, args: &[ArgValue]) -> Result<Value, Diag> {
        let resolved = self.params.resolve("FONTINFO", args);
        let mut flags: BTreeMap<&str, i64> = BTreeMap::new();
        let mut no: Option<i64> = None;
        for r in resolved {
            match r.name {
                Some("LET") => {}
                Some("NO") => no = int_arg(r.value),
                Some(name) => {
                    let Some(v) = int_arg(r.value) else {
                        return Err(arg_type_diag("FONTINFO", name, "整数"));
                    };
                    if v != 0 {
                        flags.insert(name, v);
                    }
                }
                None => {
                    return Err(error::unimplemented(
                        error::code::ARG_MISSING,
                        format!("FONTINFO 槽 {} 不在 ysc 参数表内", r.slot),
                    ));
                }
            }
        }
        let Some((flag, _)) = single_flag(&flags, "FONTINFO")? else {
            return Err(error::unimplemented(
                error::code::INFO_MULTI,
                "FONTINFO 未给出查询字段",
            ));
        };
        Ok(match flag {
            "COLOR" => Value::Int(i64::from(self.style.color)),
            "COLOR2" => match self.style.color2 {
                Some(c) => Value::Int(i64::from(c)),
                None => {
                    return Err(error::unimplemented(
                        error::code::INFO_UNSUPPORTED,
                        "FONTINFO[COLOR2] 未设置:引擎默认值未取证",
                    ));
                }
            },
            "SX" => Value::Int(self.style.size_x),
            "SY" => Value::Int(self.style.size_y),
            "BOLD" => Value::Int(i64::from(self.style.bold)),
            "ANTIALIASING" => Value::Int(i64::from(self.style.antialias)),
            "DRAWMODE" => Value::Int(self.style.draw_mode),
            // 引擎默认 GRADATION=1 / ROTATION=0(FUN_00464fac);FONT 写入后
            // 读回原值(DIV-GFX-036 的可解部分)。
            "GRADATION" => Value::Int(self.style.gradation),
            "ROTATION" => Value::Int(self.style.rotation),
            "NAME" => {
                // `NAME NO=n`:第 n 项字体名(1 基;越界 = 空串,引擎按
                // EnumFontFamiliesExA 枚举失败即空,不报错)。
                if let Some(n) = no {
                    // 1 基;0/负数/越界 = 空串(枚举失败即空,不报错)。
                    let idx = if n >= 1 { (n - 1) as usize } else { usize::MAX };
                    return Ok(Value::Str(
                        self.font_families.get(idx).cloned().unwrap_or_default(),
                    ));
                }
                match &self.style.name {
                    Some(n) => Value::Str(n.clone()),
                    None => {
                        return Err(error::unimplemented(
                            error::code::INFO_UNSUPPORTED,
                            "FONTINFO[NAME] 当前未设置字体名;系统默认名未取证",
                        ));
                    }
                }
            }
            // `NUM` = 清单条数;`LANG NO=n` = 第 n 项字符集(本宿主清单统一
            // 0 = DEFAULT_CHARSET;`es_Font.yst:166` 以非 0 过滤)。两者在
            // 清单未注入时同样应答(NUM=0/LANG=0),脚本走「无字体可选」
            // 路径 —— 与引擎在无字体环境的行为一致,不再挂号。
            "NUM" => Value::Int(self.font_families.len() as i64),
            "LANG" => {
                let _ = no;
                Value::Int(0)
            }
            other => {
                // TODO(DIV-GFX-036):PITCH/X/Y 需要未取证语义;GRADATION/
                // ROTATION 已在上方分支按默认值/原值写回(AU5 勘误)。
                return Err(error::unimplemented(
                    error::code::INFO_UNSUPPORTED,
                    format!("FONTINFO[{other}] 未取证语义"),
                ));
            }
        })
    }

    fn query_windowinfo(&self, args: &[ArgValue]) -> Result<Value, Diag> {
        let resolved = self.params.resolve("WINDOWINFO", args);
        let mut flags: BTreeMap<&str, i64> = BTreeMap::new();
        let mut no: Option<i64> = None;
        for r in resolved {
            match r.name {
                Some("LET") => {}
                Some("NO") => no = int_arg(r.value),
                Some(name) => {
                    let Some(v) = int_arg(r.value) else {
                        return Err(arg_type_diag("WINDOWINFO", name, "整数"));
                    };
                    if v != 0 {
                        flags.insert(name, v);
                    }
                }
                None => {
                    return Err(error::unimplemented(
                        error::code::ARG_MISSING,
                        format!("WINDOWINFO 槽 {} 不在 ysc 参数表内", r.slot),
                    ));
                }
            }
        }
        let Some((flag, _)) = single_flag(&flags, "WINDOWINFO")? else {
            return Err(error::unimplemented(
                error::code::INFO_MULTI,
                "WINDOWINFO 未给出查询字段",
            ));
        };
        let no = no.unwrap_or(0);
        if !(0..=3).contains(&no) {
            return Err(error::unimplemented(
                error::code::INFO_UNSUPPORTED,
                format!("WINDOWINFO[NO={no}] 超出手册 0..3"),
            ));
        }
        let window = self.windows.get(&(no as u8));
        if flag == "EXIST" {
            return Ok(Value::Int(i64::from(window.is_some())));
        }
        let Some(window) = window else {
            return Err(error::unimplemented(
                error::code::INFO_MISSING,
                format!("WINDOWINFO[NO={no}] 窗口不存在"),
            ));
        };
        Ok(match flag {
            "E" => Value::Int(i64::from(window.visible)),
            "X" => Value::Int(window.x),
            "Y" => Value::Int(window.y),
            "SX" => Value::Int(i64::from(
                window.size.map_or(self.config.viewport.width, |s| s.width),
            )),
            "SY" => Value::Int(i64::from(
                window
                    .size
                    .map_or(self.config.viewport.height, |s| s.height),
            )),
            // ---- 窗口宿主字段(手册 window/windowinfo.html 全部有页;
            // 返回源逐条对 REA 0x4586ec 的分派,见各分支注释) ----
            // `WSX/WSY` = GetWindowRect 的宽/高(含边框;REA 0x458a2a/0x458a4f)。
            // 未注入时按客户区同值应答(无边框假设;headless 与官方单窗口一致)。
            "WSX" => Value::Int(i64::from(window.outer_size.map_or(
                window.size.map_or(self.config.viewport.width, |s| s.width),
                |s| s.width,
            ))),
            "WSY" => Value::Int(i64::from(window.outer_size.map_or(
                window.size.map_or(self.config.viewport.height, |s| s.height),
                |s| s.height,
            ))),
            // `DISPLAYNO` = 显示器序号(REA 0x458b0a 读 `[wnd+0x5b4]`)。
            "DISPLAYNO" => Value::Int(window.display_no),
            // `FULLSCREEN` / `FULLSCREENBUTTON`(REA 0x458970/0x458992 读
            // `[0x8015e0]/[0x8015dc]`;宿主注入,缺省 0)。
            "FULLSCREEN" => Value::Int(window.fullscreen),
            "FULLSCREEN2" => Value::Int(window.fullscreen_button),
            // `MINIMIZE` = IsIconic(REA 0x458833..)。
            "MINIMIZE" => Value::Int(i64::from(window.minimized)),
            // `CAPTION`(REA 0x458b59 读 `[wnd+0x14]`;未设置 = -1)。
            "CAPTION" => Value::Int(window.caption),
            // `ACTIVE` = 最近有输入的时间戳比现在新(REA 0x4587a5..
            // `[task+0x28]` vs 当前);headless 单窗口恒 1(窗口聚焦即本进程),
            // 桌面宿主按聚焦状态注入 —— 未注入字段缺省视为激活。
            "ACTIVE" => Value::Int(1),
            other => {
                return Err(error::unimplemented(
                    error::code::INFO_UNSUPPORTED,
                    format!("WINDOWINFO[{other}] 未取证语义(宿主窗口状态外)"),
                ));
            }
        })
    }

    // ------------------------------------------------------------ 命令分派

    fn dispatch(&mut self, name: &str, args: &[ArgValue]) {
        if !self.params.known(name) {
            self.push_diag(
                name,
                error::unimplemented(
                    error::code::COMMAND_UNKNOWN,
                    format!("命令 `{name}` 不在工程 ysc 表中,无法解析参数名"),
                ),
            );
            return;
        }
        match name {
            "CG" => self.apply_cg(args),
            "CGACT" => self.apply_cgact(args),
            "CGEND" => self.apply_cgend(args),
            "FONT" => self.apply_font(args),
            // INFO 查询由 VM 的 op_info 在脚本执行期通过 GraphicsHost 读取;
            // 事件只是事实记录,不应在这里再挂“未实现”诊断。
            "CGINFO" | "FONTINFO" => {}
            other => self.push_diag(
                other,
                error::unimplemented(
                    error::code::COMMAND_UNIMPLEMENTED,
                    "图形域命令未接线(首屏未使用)",
                ),
            ),
        }
    }

    // ------------------------------------------------------------ WINDOW

    fn dispatch_window(&mut self, name: &str, args: &[ArgValue]) {
        if !self.params.known(name) {
            self.push_diag(
                name,
                error::unimplemented(
                    error::code::COMMAND_UNKNOWN,
                    format!("窗口域命令 `{name}` 不在工程 ysc 表中"),
                ),
            );
            return;
        }
        match name {
            "WINDOW" => self.apply_window(args),
            "WINDOWEND" => self.apply_windowend(args),
            // 同 CGINFO/FONTINFO:由 VM 的 op_info 同步查询。
            "WINDOWINFO" => {}
            other => self.push_diag(
                other,
                error::unimplemented(error::code::COMMAND_UNIMPLEMENTED, "窗口域命令未接线"),
            ),
        }
    }

    fn apply_window(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("WINDOW", args);
        let mut no: Option<i64> = None;
        let mut cgid: Option<Vec<u8>> = None;
        let mut e: Option<i64> = None;
        let mut a: Option<i64> = None;
        let mut x: Option<i64> = None;
        let mut y: Option<i64> = None;
        let mut sx: Option<i64> = None;
        let mut sy: Option<i64> = None;
        let mut z: Option<i64> = None;
        let mut unknown: Vec<String> = Vec::new();
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("WINDOW", r.slot);
                continue;
            };
            match name {
                "NO" => match int_arg(&r.value) {
                    Some(n) => no = Some(n),
                    None => self.arg_type("WINDOW", name, "整数", &r.value),
                },
                "CGID" => match &r.value {
                    Value::Str(b) => cgid = Some(b.clone()),
                    _ => self.arg_type("WINDOW", name, "字符串", &r.value),
                },
                "E" => match int_arg(&r.value) {
                    Some(n) => e = Some(n),
                    None => self.arg_type("WINDOW", name, "整数", &r.value),
                },
                "A" => match int_arg(&r.value) {
                    Some(n) => a = Some(n),
                    None => self.arg_type("WINDOW", name, "整数", &r.value),
                },
                "X" => x = int_arg(&r.value),
                "Y" => y = int_arg(&r.value),
                "SX" => sx = int_arg(&r.value),
                "SY" => sy = int_arg(&r.value),
                "Z" => z = int_arg(&r.value),
                // 窗口宿主字段:已取证的部分记录进 WindowState(供
                // `WINDOWINFO` 读回),其余仍按 DIV-GFX-037 挂 host。
                // `CAPTION`/`FULLSCREEN`/`FULLSCREENBUTTON`/`DISPLAYNO` 的
                // 读回源见 `query_windowinfo`(REA 0x458b59/0x458970/0x458992/
                // 0x458b0a);`MENUID/REGIONID/DRAGID/CLOSEMODE/RESIZE/MAXIMIZE`
                // 语义未取证,维持原状。
                "CAPTION" => {
                    if let Some(v) = int_arg(&r.value) {
                        if let Some(w) = self.windows.get_mut(&(no.unwrap_or(0) as u8)) {
                            w.caption = v;
                        }
                    }
                }
                "FULLSCREEN" => {
                    if let Some(v) = int_arg(&r.value) {
                        if let Some(w) = self.windows.get_mut(&(no.unwrap_or(0) as u8)) {
                            w.fullscreen = v;
                        }
                    }
                }
                "FULLSCREENBUTTON" => {
                    if let Some(v) = int_arg(&r.value) {
                        if let Some(w) = self.windows.get_mut(&(no.unwrap_or(0) as u8)) {
                            w.fullscreen_button = v;
                        }
                    }
                }
                "MENUID" | "REGIONID" | "DRAGID" | "ACTIVE" | "MINIMIZE" | "CLOSEMODE"
                | "RESIZE" | "MAXIMIZE" | "WSX" | "WSY" => {}
                other => unknown.push(other.to_string()),
            }
        }
        let no = no.unwrap_or(0);
        if !(0..=3).contains(&no) {
            self.push_diag(
                "WINDOW",
                error::recoverable(
                    error::code::WINDOW_UNSUPPORTED,
                    format!("WINDOW[NO={no}] 超出手册 0..3"),
                ),
            );
            return;
        }
        if !unknown.is_empty() {
            let detail = format!("窗口参数未分类: {}", unknown.join(","));
            self.push_diag(
                "WINDOW",
                error::unimplemented(error::code::WINDOW_UNSUPPORTED, detail),
            );
        }
        let mut pending: Option<Diag> = None;
        let alpha = match a {
            Some(v) => match i16::try_from(v) {
                Ok(v) => Some(v),
                Err(_) => {
                    pending = Some(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("WINDOW[A={v}] 超出 s16 存储范围"),
                    ));
                    None
                }
            },
            None => None,
        };
        let size = match (sx, sy) {
            (Some(w), Some(h)) => match (u32::try_from(w), u32::try_from(h)) {
                (Ok(w), Ok(h)) => Some(SizeU32 {
                    width: w,
                    height: h,
                }),
                _ => {
                    pending = Some(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("WINDOW[SX={w} SY={h}] 超出 u32 范围"),
                    ));
                    None
                }
            },
            (Some(_), None) | (None, Some(_)) => {
                pending = Some(error::unimplemented(
                    error::code::WINDOW_UNSUPPORTED,
                    "WINDOW 只给 SX/SY 之一:窗口尺寸部分更新未取证",
                ));
                None
            }
            (None, None) => None,
        };
        let entry = self
            .windows
            .entry(no as u8)
            .or_insert_with(|| WindowState::new(no as u8, None));
        if let Some(b) = cgid {
            // CGID = 显示对象图层组;空串 = 未指定(主窗口回退到配置默认)。
            entry.layer_set = if b.is_empty() {
                None
            } else {
                Some(path::normalize(&b, None, b.ends_with(b"/")).path)
            };
        }
        if let Some(v) = alpha {
            entry.alpha = v;
            // 手册 window/window.html:A 在 β4 只表示显示/非显示。
            entry.visible = v != 0;
        }
        if let Some(v) = e {
            entry.visible = v != 0;
        }
        if let Some(v) = x {
            entry.x = v;
        }
        if let Some(v) = y {
            entry.y = v;
        }
        if let Some(v) = z {
            entry.z = v;
        }
        if let Some(s) = size {
            entry.size = Some(s);
        }
        if let Some(d) = pending {
            self.push_diag("WINDOW", d);
        }
    }

    fn apply_windowend(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("WINDOWEND", args);
        let mut no: Option<i64> = None;
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("WINDOWEND", r.slot);
                continue;
            };
            if name == "NO" {
                no = int_arg(&r.value);
            }
        }
        let no = no.unwrap_or(0);
        if no == 0 {
            self.push_diag(
                "WINDOWEND",
                error::recoverable(
                    error::code::WINDOW_UNSUPPORTED,
                    "WINDOWEND 不能删除主窗口(手册 window/windowend.html)",
                ),
            );
            return;
        }
        if !(1..=3).contains(&no) {
            self.push_diag(
                "WINDOWEND",
                error::recoverable(
                    error::code::WINDOW_UNSUPPORTED,
                    format!("WINDOWEND[NO={no}] 超出手册 1..3"),
                ),
            );
            return;
        }
        self.windows.remove(&(no as u8));
    }

    // ------------------------------------------------------------ CG

    fn apply_cg(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CG", args);
        let mut id: Option<Vec<u8>> = None;
        let mut idno: Option<i64> = None;
        let mut params: Vec<(String, EngineValue)> = Vec::new();
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CG", r.slot);
                continue;
            };
            match name {
                "ID" => match &r.value {
                    Value::Str(b) => id = Some(b.clone()),
                    _ => self.arg_type("CG", "ID", "字符串", &r.value),
                },
                "IDNO" => match int_arg(&r.value) {
                    Some(n) => idno = Some(n),
                    None => self.arg_type("CG", "IDNO", "整数", &r.value),
                },
                _ => params.push((name.to_string(), EngineValue::from_value(&r.value))),
            }
        }
        let Some(id) = id else {
            self.push_diag(
                "CG",
                error::recoverable(error::code::ARG_MISSING, "CG 缺 ID(无法定位图层)"),
            );
            return;
        };
        let trailing = id.ends_with(b"/");
        let parsed = path::normalize(&id, idno, trailing);
        let is_dir = parsed.is_dir;

        let mut file_diags = Vec::new();
        let file = take_file(&params, &mut file_diags);
        for d in file_diags {
            self.push_diag("CG", d);
        }

        let existing = self.nodes.get(&parsed.path).map(|n| n.layer.is_group());
        if is_dir {
            if existing == Some(false) {
                self.push_diag(
                    "CG",
                    error::recoverable(
                        error::code::LAYER_KIND,
                        format!("`{}` 已是图像层,不能当图层组使用", parsed.path.display()),
                    ),
                );
                return;
            }
            if existing.is_none() {
                self.create_group(&parsed.path);
            }
            if file.is_some() {
                self.push_diag(
                    "CG",
                    error::unimplemented(
                        error::code::LAYER_KIND,
                        "FILE 只对图像层有效;图层组忽略 FILE",
                    ),
                );
            }
            let diags = {
                let node = self
                    .nodes
                    .get_mut(&parsed.path)
                    .expect("group just ensured");
                apply_layer_params(&mut node.layer, &params, true)
            };
            for d in diags {
                self.push_diag("CG", d);
            }
            return;
        }

        if existing == Some(true) {
            self.push_diag(
                "CG",
                error::recoverable(
                    error::code::LAYER_KIND,
                    format!("`{}` 已是图层组,不能当图像层使用", parsed.path.display()),
                ),
            );
            return;
        }

        if existing.is_none() {
            // 引擎处理体是“取/建节点”:即使没有 FILE 也建一个无纹理图层,
            // 绘制遍历按 `+0x90 == 0` 跳过。手册例 4 称“图层不存在 = 错误”,
            // 这里按引擎实际 get-or-create 处理(留档,不改变可见结果)。
            let texture = file.clone().unwrap_or(TextureRef::None);
            self.create_image(&parsed.path, texture, None);
            if file.is_none() {
                self.push_diag(
                    "CG",
                    error::info(
                        error::code::TEXTURE_MISSING,
                        "CG 未给 FILE:按引擎 get-or-create 建无纹理图层(遍历跳过)",
                    ),
                );
            }
        } else if let Some(texture) = file {
            // 手册:FILE 替换只换图,不重置 A/位置等属性。
            if let Some(node) = self.nodes.get_mut(&parsed.path) {
                node.layer.texture = texture;
                // 新纹理没有旧快照/图像加工的状态;属性随后重算。
                node.layer.reset_status();
            }
            // 旧快照/图像加工随旧纹理作废。
            self.copy_log.retain(|c| c.dest != parsed.path);
            self.image_log
                .retain(|i| i.source != parsed.path && i.target.as_ref() != Some(&parsed.path));
            // 新纹理 = 干净像素面:之前 `CGACT[TEXT]` 的绘制项随之作废。
            self.clear_layer_texts(&parsed.path);
        }

        let diags = {
            let node = self
                .nodes
                .get_mut(&parsed.path)
                .expect("layer just ensured");
            apply_layer_params(&mut node.layer, &params, false)
        };
        for d in diags {
            self.push_diag("CG", d);
        }
    }

    // ------------------------------------------------------------ CGEND

    fn apply_cgend(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CGEND", args);
        let mut id: Option<Vec<u8>> = None;
        let mut idno: Option<i64> = None;
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CGEND", r.slot);
                continue;
            };
            match name {
                "ID" => match &r.value {
                    Value::Str(b) => id = Some(b.clone()),
                    _ => self.arg_type("CGEND", "ID", "字符串", &r.value),
                },
                "IDNO" => match int_arg(&r.value) {
                    Some(n) => idno = Some(n),
                    None => self.arg_type("CGEND", "IDNO", "整数", &r.value),
                },
                _ => {}
            }
        }
        let Some(id) = id else {
            self.push_diag(
                "CGEND",
                error::recoverable(error::code::ARG_MISSING, "CGEND 缺 ID"),
            );
            return;
        };
        let parsed = path::normalize(&id, idno, id.ends_with(b"/"));
        if path::is_root(&parsed.path) {
            self.push_diag(
                "CGEND",
                error::recoverable(error::code::LAYER_INVALID_ID, "不能删除根 `:/`"),
            );
            return;
        }
        if !self.nodes.contains_key(&parsed.path) {
            self.push_diag(
                "CGEND",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("删除不存在的图层 `{}`", parsed.path.display()),
                ),
            );
            return;
        }
        let removed = self.remove_subtree(&parsed.path);
        self.drop_ops_for(&removed);
        // 文本层被删除 ⇒ 文本缓冲/本帧绘制项一并清(DIV-GFX-034 的窗口删除路径)。
        self.text_draws.retain(|id, _| !removed.contains(id));
        self.text_layers.retain(|id, _| !removed.contains(id));
        if parsed.path == Self::w0() && !self.nodes.contains_key(&Self::w0()) {
            // W0 是相对路径的默认基目录;删掉后立刻重建,保证后续相对 ID 可用。
            self.ensure_w0();
        }
    }

    // ------------------------------------------------------------ CGACT

    fn apply_cgact(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("CGACT", args);
        let mut ids: [Option<Vec<u8>>; 4] = [None, None, None, None];
        let mut idnos: [Option<i64>; 4] = [None, None, None, None];
        let mut params: Vec<(String, EngineValue)> = Vec::new();
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("CGACT", r.slot);
                continue;
            };
            let idx = match name {
                "ID" => Some(0),
                "ID2" => Some(1),
                "ID3" => Some(2),
                "ID4" => Some(3),
                _ => None,
            };
            if let Some(i) = idx {
                match &r.value {
                    Value::Str(b) => ids[i] = Some(b.clone()),
                    _ => self.arg_type("CGACT", name, "字符串", &r.value),
                }
                continue;
            }
            let idx = match name {
                "IDNO" => Some(0),
                "IDNO2" => Some(1),
                "IDNO3" => Some(2),
                "IDNO4" => Some(3),
                _ => None,
            };
            if let Some(i) = idx {
                match int_arg(&r.value) {
                    Some(n) => idnos[i] = Some(n),
                    None => self.arg_type("CGACT", name, "整数", &r.value),
                }
                continue;
            }
            params.push((name.to_string(), EngineValue::from_value(&r.value)));
        }

        let paths: Vec<Option<LayerId>> = (0..4)
            .map(|i| {
                ids[i]
                    .as_ref()
                    .map(|b| path::normalize(b, idnos[i], b.ends_with(b"/")).path)
            })
            .collect();
        let Some(source) = paths[0].clone() else {
            self.push_diag(
                "CGACT",
                error::recoverable(error::code::ARG_MISSING, "CGACT 缺 ID"),
            );
            return;
        };
        let dest = paths[1].clone().unwrap_or_else(|| source.clone());
        // TODO(DIV-GFX-031):ID3/ID4 多图层运算、Z2/BNO 修饰语义未取证。
        if paths[2].is_some() || paths[3].is_some() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "ID3/ID4 的多图层运算语义未取证",
                ),
            );
        }
        let action_count = count_image_actions(&params);
        if action_count > 1 {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    format!("CGACT 同时给出 {action_count} 个图像动作;只记录第一个"),
                ),
            );
        }

        let set = find_value(&params, "SET");
        let set2 = find_value(&params, "SET2");
        let set_str = find_str(&params, "SETSTR");
        let bno = find_int(&params, "BNO");
        // 区域坐标按**像素取整**读:`es.CGACT_SIZE` 的源区域是缩放后的实际
        // 范围,实测 `CGACT[… SIZE=1 X=0 Y=0 X2=1100 Y2=824.5 …]` —— 带
        // 小数。旧实现用 `find_int`(`as_int` 要求 `fract()==0`)会让
        // `Y2=824.5` 静默消失 ⇒ 区域只剩 3 个字段 ⇒
        // `gfx.imageop.region.partial` ⇒ 整条 SIZE 被跳过,sprite 不上屏。
        // 区域是像素边界 ⇒ 四舍五入成整数,与 `SX/SY` 同口径。
        //
        // **×100 定点回退**:`es_Sprite.yst` 的 `es.CGACT_SIZE` 在中画质
        // 分支(`quo!=0`)发的是
        // `CGACT[… SIZE2=1 FX=@X1*100+@FLX FY=… FX2=… FY2=… FSX=@SIZE_X*100 FSY=…]`
        // —— 没有任何 `X/SX` 整数键。`@FLX` 的注释原文是「表示位置
        // @LEFT_X,@LEFT_Y の小数点値を掃い、それを CGACT[SIZE2=1 ～] に
        // 伝える」,即把小数折回百分位;`FX/FY/FX2/FY2/FSX/FSY` 因此是
        // `X/Y/X2/Y2/SX/SY` 的百分位定点形式(引擎绘制侧按 ÷100.0 读
        // `+0x1b0..`,见 docs/render-parity.md §7)。
        // 手册 `cg/cgact.html` 只列整数键(无 FX/FSX 页)⇒ `fixed(no-manual)`,
        // 证据锚点:语料 `es_Sprite.yst:8538 es.CGACT_SIZE` + §7 反汇编。
        // 不实现这条回退的后果:整条 SIZE2 判「尺寸缺失」⇒ 图层被标记
        // 不可执行 ⇒ 立绘/CG 永久不上屏。
        let fixed_px = |name: &str| {
            find_f64(&params, name).map(|v| (v / 100.0).round() as i64)
        };
        let x = find_px(&params, "X").or_else(|| fixed_px("FX"));
        let y = find_px(&params, "Y").or_else(|| fixed_px("FY"));
        let x2 = find_px(&params, "X2").or_else(|| fixed_px("FX2"));
        let y2 = find_px(&params, "Y2").or_else(|| fixed_px("FY2"));
        // SIZE/SIZE2 的目标尺寸(手册 cg/cgact.html 例 3/4)。
        //
        // **必须按浮点读**:`es_Sprite.yst` 的 `es.CGACT_SIZE` 传进来的
        // `SX/SY` 是缩放后的实际尺寸,常带小数(实测
        // `CGACT[ID=SPLBUF401 ID2=SPL401 SIZE=1 SX=1101 SY=825.5]`)。
        // 旧实现用 `find_int`(`as_int` 要求 `fract()==0`)⇒ 825.5 被判
        // 「尺寸缺失」⇒ `gfx.imageop.size.missing` ⇒ sprite 层永远拿不到
        // 画面,整个 sprite 系统不上屏。整数由 `as_f64` 一并覆盖。
        // `FSX/FSY` 是同一尺寸的百分位定点形式(见上)。
        let sx = find_f64(&params, "SX").or_else(|| find_f64(&params, "FSX").map(|v| v / 100.0));
        let sy = find_f64(&params, "SY").or_else(|| find_f64(&params, "FSY").map(|v| v / 100.0));

        let copy_action = ["COPY", "COPY2", "TEXCOPY", "TEXSSCOPY"]
            .iter()
            .any(|n| flag(&params, n));
        let swap_action = flag(&params, "SWAP") || flag(&params, "SWAP2");
        if (copy_action || swap_action) && action_count > 0 {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "COPY/SWAP 与图像动作并存:引擎执行顺序未取证",
                ),
            );
        }

        for (name, kind) in [
            ("COPY", CopyKind::Copy),
            ("COPY2", CopyKind::Copy2),
            ("TEXCOPY", CopyKind::TexCopy),
            ("TEXSSCOPY", CopyKind::TexSsCopy),
        ] {
            if flag(&params, name) {
                self.cgact_copy(kind, &source, &dest, x, y, x2, y2);
                return;
            }
        }
        if flag(&params, "SWAP") {
            self.cgact_swap(&source, &dest, false);
            return;
        }
        if flag(&params, "SWAP2") {
            self.cgact_swap(&source, &dest, true);
            return;
        }

        // `CGACT[TEXT=1]`:手册 cg/cgact.html 例 25/26 在图层上绘制字符串。
        // 这是 ERIS 对话文本的真实路径(`es_Font.yst` 每字符一次)。
        if flag(&params, "TEXT") {
            self.cgact_text(&source, set, set_str, x, y, bno);
            return;
        }

        if let Some((name, action)) = first_image_action(&params) {
            self.cgact_image(
                name,
                action,
                &source,
                paths[1].clone(),
                set,
                set2,
                set_str,
                sx,
                sy,
                x,
                y,
                x2,
                y2,
            );
            return;
        }

        // 没有任何动作参数:CGACT 的属性修改(例如只给 SET)语义未取证。
        if !params.is_empty() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "CGACT 无动作参数(仅属性/坐标)语义未取证",
                ),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cgact_copy(
        &mut self,
        kind: CopyKind,
        source: &LayerId,
        dest: &LayerId,
        x: Option<i64>,
        y: Option<i64>,
        x2: Option<i64>,
        y2: Option<i64>,
    ) {
        if !self.nodes.contains_key(source) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("复制源 `{}` 不存在", source.display()),
                ),
            );
            return;
        }
        let source_is_group = self.nodes[source].layer.is_group();
        if !self.nodes.contains_key(dest) {
            // 手册 cg/cgact.html 例 30:目标层可作为复制结果出现。
            self.create_image(dest, TextureRef::None, None);
        }
        if self.nodes.get(dest).is_some_and(|n| n.layer.is_group()) {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::COPY_UNSUPPORTED,
                    "复制目标为图层组:复制到组合成纹理的语义未取证",
                ),
            );
            return;
        }
        let copy_id = CopyId(self.next_copy());
        let seq = self.next_texture_seq();
        let region = if x.is_some() || y.is_some() || x2.is_some() || y2.is_some() {
            CopyRegion::Partial { x, y, x2, y2 }
        } else {
            CopyRegion::Full
        };
        // 命令时刻的源基础纹理(`DIV-GFX-053`)。
        //
        // 官方 COPY 在命令处理器里**立即**执行(EVID-T-15);源层可能在同帧
        // 稍后被 `CGEND` 删除(ERIS 按钮快照:建 `BT.OFF.TMP` → 逐按钮
        // `COPY2` 切片 → 全部按钮建完后 `CGEND[BT.OFF.TMP]`)。本内核的纹理
        // pass 在帧末回放,那时源节点已不存在;记录当时的 `TextureRef` 才能
        // 按官方语义重放。组源不需要(它按组 id 自合成)。
        let source_texture = if source_is_group {
            None
        } else {
            self.nodes
                .get(source)
                .map(|n| n.layer.texture.clone())
                .filter(|t| !matches!(t, TextureRef::None))
        };
        // 命令时刻的源层已画文本(DIV-GFX-058:官方写表面,COPY 天然携带;
        // 命令之后再画的文本不进快照)。组源为空(组内层文本由后端按组当前
        // 状态合成)。
        let texts = if source_is_group {
            Vec::new()
        } else {
            self.text_draws
                .get(source)
                .map(|t| t.draws.clone())
                .unwrap_or_default()
        };
        self.copy_log.retain(|c| c.dest != *dest);
        self.image_log
            .retain(|i| i.target.as_ref().is_none_or(|t| t != dest));
        // EVID-T-11/T-12:COPY/COPY2 在命令执行时取源层当前状态;区域是源
        // 纹理坐标(含端点),裁剪块写到目标 (0,0),无区域 = 整图复制。
        // TEXCOPY/TEXSSCOPY 是纹理句柄级操作,本内核没有对应证据,保持跳过。
        let status = match kind {
            CopyKind::Copy | CopyKind::Copy2 => OpStatus::Executable,
            CopyKind::TexCopy | CopyKind::TexSsCopy => {
                OpStatus::Unimplemented("gfx.copy.texcopy.unsupported")
            }
        };
        self.push_copy_log(CopyOp {
            seq,
            id: copy_id,
            kind,
            source: if source_is_group {
                CopySource::Group(source.clone())
            } else {
                CopySource::Layer(source.clone())
            },
            source_texture,
            texts,
            dest: dest.clone(),
            region,
            timing: CopyTiming::CommandCurrent,
            status,
        });
        if kind == CopyKind::Copy {
            let attrs = self.nodes[source].layer.clone();
            if let Some(node) = self.nodes.get_mut(dest) {
                copy_attributes(&attrs, &mut node.layer);
            }
        }
        // DIV-GFX-058:可执行 COPY 整面覆写目标层像素 ⇒ 目标层已画文本随之
        // 作废(引擎烘焙语义)。源 = 目标的自复制不覆写,保持原文本。
        if status == OpStatus::Executable && dest != source {
            self.clear_layer_texts(dest);
        }
        // 快照继承源层的尺寸/资产键(COPY2 后脚本会查询目标层的 SX/SY;
        // 源层尺寸来自 VM 注入的资产元数据,见 `asset_sizes`)。
        let mut snap_size = None;
        let mut snap_file = None;
        if let Some(src_node) = self.nodes.get(source) {
            snap_size = match &src_node.layer.texture {
                TextureRef::Asset(key) => {
                    snap_file = Some(key.clone());
                    self.asset_sizes.get(key).copied()
                }
                TextureRef::Snapshot(id) => self.snapshot_sizes.get(id).copied(),
                TextureRef::Offscreen(id) => self.nodes.get(id).and_then(|n| n.layer.group_size()),
                TextureRef::None | TextureRef::SolidBlack1x1 => src_node.layer.image_size(),
            };
        }
        if let Some(size) = snap_size {
            self.snapshot_sizes.insert(copy_id, size);
        }
        if let Some(file) = snap_file {
            self.snapshot_files.insert(copy_id, file);
        }
        if let Some(node) = self.nodes.get_mut(dest) {
            node.layer.texture = TextureRef::Snapshot(copy_id);
            node.layer.set_texture_status(match status {
                OpStatus::Executable => LayerStatus::Executable,
                OpStatus::Unimplemented(why) => LayerStatus::Unimplemented(why),
            });
        }
        if status != OpStatus::Executable {
            let why = match status {
                OpStatus::Unimplemented(why) => why,
                OpStatus::Executable => unreachable!(),
            };
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::COPY_UNSUPPORTED,
                    format!("{} 不执行: {why}", kind.name()),
                ),
            );
        }
    }

    fn cgact_swap(&mut self, source: &LayerId, dest: &LayerId, image_only: bool) {
        if !self.nodes.contains_key(source) || !self.nodes.contains_key(dest) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!(
                        "SWAP 源/目标不存在: `{}` / `{}`",
                        source.display(),
                        dest.display()
                    ),
                ),
            );
            return;
        }
        if self.nodes[source].layer.is_group() || self.nodes[dest].layer.is_group() {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::COPY_UNSUPPORTED,
                    "图层组 SWAP 语义未取证(仅图像层)",
                ),
            );
            return;
        }
        // TODO(DIV-GFX-032):SWAP/SWAP2 与引擎模式 1/2 的精确对应仍是
        // Likely,需 `FUN_0042f57d` 调用点确认。
        // 引擎 FUN_0042f57d:
        // - 总是交换纹理/图像名与 +0x68/+0x70(X/Y);
        // - 模式 2 额外交换 +0x89(E)/+0x80(A)/+0x82(T)/+0x84(TA)/+0x87(MODE)。
        // 手册:SWAP=图像+属性,SWAP2=仅图像。模式 1/2 与 SWAP/SWAP2 的
        // 精确对应为 Likely;这里按“SWAP2 只换图+坐标、SWAP 再换可见属性”实现。
        let src = self.nodes[source].layer.clone();
        let dst = self.nodes[dest].layer.clone();
        {
            let Some(s) = self.nodes.get_mut(source) else {
                self.push_diag(
                    "CGACT",
                    error::recoverable(
                        error::code::LAYER_MISSING,
                        format!("SWAP 源在交换中消失: `{}`", source.display()),
                    ),
                );
                return;
            };
            s.layer.texture = dst.texture.clone();
            s.layer.texture_status = dst.texture_status;
            s.layer.transform.x = dst.transform.x;
            s.layer.transform.y = dst.transform.y;
            s.layer.refresh_status();
        }
        {
            let Some(d) = self.nodes.get_mut(dest) else {
                self.push_diag(
                    "CGACT",
                    error::recoverable(
                        error::code::LAYER_MISSING,
                        format!("SWAP 目标在交换中消失: `{}`", dest.display()),
                    ),
                );
                return;
            };
            d.layer.texture = src.texture.clone();
            d.layer.texture_status = src.texture_status;
            d.layer.transform.x = src.transform.x;
            d.layer.transform.y = src.transform.y;
            d.layer.refresh_status();
        }
        if !image_only {
            let swap_attrs = |a: &mut Layer, b: &Layer| {
                a.visible = b.visible;
                a.alpha = b.alpha;
                a.blend = b.blend;
                a.transition = b.transition.clone();
            };
            let Some(s) = self.nodes.get_mut(source) else {
                self.push_diag(
                    "CGACT",
                    error::recoverable(
                        error::code::LAYER_MISSING,
                        format!("SWAP 源在属性交换中消失: `{}`", source.display()),
                    ),
                );
                return;
            };
            swap_attrs(&mut s.layer, &dst);
            let Some(d) = self.nodes.get_mut(dest) else {
                self.push_diag(
                    "CGACT",
                    error::recoverable(
                        error::code::LAYER_MISSING,
                        format!("SWAP 目标在属性交换中消失: `{}`", dest.display()),
                    ),
                );
                return;
            };
            swap_attrs(&mut d.layer, &src);
        }
        // DIV-GFX-058:SWAP/SWAP2 都交换纹理 ⇒ 烘焙文本随表面走。
        // (TODO DIV-GFX-032 的模式 1/2 归属为 Likely,但“面互换”两种模式
        // 一致,这里不做模式区分。)
        let src_texts = self.text_draws.remove(source);
        let dst_texts = self.text_draws.remove(dest);
        if let Some(mut t) = src_texts {
            for d in &mut t.draws {
                d.target = dest.clone();
            }
            self.text_revision_seq = self.text_revision_seq.wrapping_add(1);
            t.revision = self.text_revision_seq;
            self.text_draws.insert(dest.clone(), t);
        }
        if let Some(mut t) = dst_texts {
            for d in &mut t.draws {
                d.target = source.clone();
            }
            self.text_revision_seq = self.text_revision_seq.wrapping_add(1);
            t.revision = self.text_revision_seq;
            self.text_draws.insert(source.clone(), t);
        }
    }

    /// `CGACT[TEXT=1]`:把一段字符串登记为图层的**持久**绘制项。
    ///
    /// 引擎在命令处理器里把字画进图层像素(手册「操作を行うと元のイメージ
    /// には戻せません」;REA `0x404398 → 0x4046fc → 0x404914 → 0x4056b8`
    /// 按 SET 1/9/3 次合成进表面字节),画一次即常驻。ERIS 的对话逐字路径
    /// (`es_Text.yst:1031`)每帧重发同一集合,同层同参的重复命令按
    /// [`TextDraw::dedup_key`] 精确去重(可见结果不变,且规避
    /// `FONT[DRAWMODE=1]` 的重复合成加深);按钮标签等一次性绘制则跨帧保留
    /// (DIV-GFX-058)。
    fn cgact_text(
        &mut self,
        target: &LayerId,
        set: Option<EngineValue>,
        set_str: Option<Vec<u8>>,
        x: Option<i64>,
        y: Option<i64>,
        bno: Option<i64>,
    ) {
        if !self.nodes.contains_key(target) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("CGACT[TEXT] 目标层 `{}` 不存在", target.display()),
                ),
            );
            return;
        }
        if self.nodes.get(target).is_some_and(|n| n.layer.is_group()) {
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    "CGACT[TEXT] 目标为图层组:写入组合成纹理的语义未取证",
                ),
            );
            return;
        }
        let Some(bytes) = set_str.filter(|b| !b.is_empty()) else {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::ARG_MISSING,
                    "CGACT[TEXT] 缺 SETSTR:引擎不绘制空字符串,但参数缺失不是同义",
                ),
            );
            return;
        };
        let set = set.as_ref().and_then(EngineValue::as_int).unwrap_or(0);
        let draw = TextDraw {
            target: target.clone(),
            bytes,
            x: x.unwrap_or(0),
            y: y.unwrap_or(0),
            // 引擎按命令到达时的 FONT 状态绘制(REA 0x404a1b 读 +0x28/+0x33/+0x34)。
            style: self.style.clone(),
            set,
            bno,
        };
        let (dropped, changed) = {
            let texts = self.text_draws.entry(target.clone()).or_default();
            let before = texts.dropped;
            let changed = texts.push(draw);
            (texts.dropped - before, changed)
        };
        if changed {
            self.text_revision_seq = self.text_revision_seq.wrapping_add(1);
            let seq = self.text_revision_seq;
            if let Some(texts) = self.text_draws.get_mut(target) {
                texts.revision = seq;
            }
        }
        if dropped > 0 {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::TEXT_UNSUPPORTED,
                    format!(
                        "单层文本绘制项达到上限 {MAX_TEXT_DRAWS_PER_LAYER},丢弃最旧 {dropped} 项",
                    ),
                ),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cgact_image(
        &mut self,
        name: &str,
        action: ImageAction,
        source: &LayerId,
        target: Option<LayerId>,
        set: Option<EngineValue>,
        set2: Option<EngineValue>,
        set_str: Option<Vec<u8>>,
        sx: Option<f64>,
        sy: Option<f64>,
        x: Option<i64>,
        y: Option<i64>,
        x2: Option<i64>,
        y2: Option<i64>,
    ) {
        if !self.nodes.contains_key(source) {
            self.push_diag(
                "CGACT",
                error::recoverable(
                    error::code::LAYER_MISSING,
                    format!("图像加工源 `{}` 不存在", source.display()),
                ),
            );
            return;
        }
        let target = target.or_else(|| Some(source.clone()));
        if let Some(t) = &target {
            if !self.nodes.contains_key(t) {
                self.create_image(t, TextureRef::None, None);
            }
            if self.nodes.get(t).is_some_and(|n| n.layer.is_group()) {
                self.push_diag(
                    "CGACT",
                    error::unimplemented(
                        error::code::IMAGEOP_UNSUPPORTED,
                        "图像加工目标为图层组:写入组合成纹理的语义未取证",
                    ),
                );
                return;
            }
        }
        // TODO(DIV-GFX-013):其余图像加工像素算法未实现;只记录参数。
        //
        // RECTPAINT/2/3 的像素语义已在 EVID-J-02..05 取证:
        // `FUN_0042f809`(0x42f809) 行 334-342 把动作 0x18/0x19/0x1a
        // 分派到 `FUN_0043185c`(0x43185c),后者按模式 1/2/3 填充
        // RGB / alpha / ARGB。这里只放行**原地、无 ID2、整层或完整矩形、
        // 基图为 Asset/SolidBlack1x1** 的情形;其余保持 Unimplemented。
        let explicit_target = target.is_some();
        let target = target.or_else(|| Some(source.clone()));
        let status = if matches!(
            action,
            ImageAction::RectPaint | ImageAction::RectPaint2 | ImageAction::RectPaint3
        ) {
            if explicit_target && target.as_ref() != Some(source) {
                OpStatus::Unimplemented("gfx.imageop.id2.unsupported")
            } else {
                match rect_paint_precheck(
                    self.nodes.get(source),
                    &set,
                    &set2,
                    &set_str,
                    sx,
                    sy,
                    x,
                    y,
                    x2,
                    y2,
                ) {
                    Ok(()) => OpStatus::Executable,
                    Err(reason) => OpStatus::Unimplemented(reason),
                }
            }
        } else if matches!(action, ImageAction::Reverse | ImageAction::Horizontal) {
            // EVID-T-13:REVERSE = RGB 取反,HORIZONTAL = 水平镜像;两者都
            // 只对可解析的基图(资产/1×1 黑)放行。ID2 与区域变体未测,
            // 保持显式跳过。
            if explicit_target && target.as_ref() != Some(source) {
                OpStatus::Unimplemented("gfx.imageop.id2.unsupported")
            } else if x.is_some() || y.is_some() || x2.is_some() || y2.is_some() {
                OpStatus::Unimplemented("gfx.imageop.region.unsupported")
            } else {
                match self.nodes.get(source) {
                    Some(n)
                        if matches!(
                            n.layer.texture,
                            TextureRef::Asset(_) | TextureRef::SolidBlack1x1
                        ) && n.layer.texture_status == LayerStatus::Executable =>
                    {
                        OpStatus::Executable
                    }
                    _ => OpStatus::Unimplemented("gfx.imageop.copy_source.unsupported"),
                }
            }
        } else if action.is_scale() {
            // `CGACT[SIZE/SIZE2/SIZE3/SIZE4/SIZE5]`:源区域缩放后写入目标层
            // (ID2 或原地)。源可以是资产/快照/1×1 黑;区域可选(缺省整图)。
            match (sx, sy) {
                (Some(w), Some(h)) if w > 0.0 && h > 0.0 => match self.nodes.get(source) {
                    Some(n)
                        if matches!(
                            n.layer.texture,
                            TextureRef::Asset(_)
                                | TextureRef::Snapshot(_)
                                | TextureRef::SolidBlack1x1
                        ) && n.layer.texture_status == LayerStatus::Executable =>
                    {
                        OpStatus::Executable
                    }
                    _ => OpStatus::Unimplemented("gfx.imageop.copy_source.unsupported"),
                },
                _ => OpStatus::Unimplemented("gfx.imageop.size.missing"),
            }
        } else {
            OpStatus::Unimplemented("gfx.imageop.unsupported")
        };
        let executable = status == OpStatus::Executable;
        let seq = self.next_texture_seq();
        let image_action = action.clone();
        self.push_image_log(ImageOp {
            seq,
            source: source.clone(),
            target: target.clone(),
            action,
            set,
            set2,
            set_str,
            sx,
            sy,
            x,
            y,
            x2,
            y2,
            status,
        });
        // `CGACT[SIZE/SIZE2/SIZE3/SIZE4/SIZE5]` 把源区域缩放到目标层 ⇒ 目标层
        // 的绘制尺寸随之变为 `SX×SY`(手册 `cg/cgact.html` 例 3/4;
        // `es.CGACT_SIZE` 之后脚本会用 `CGINFO[SX/SY]` 读这个新尺寸)。尺寸记录
        // 在层上,`CGINFO` 因此是通用查询,不需要任何命名约定。
        if executable && image_action.is_scale() {
            if let (Some(w), Some(h)) = (sx, sy) {
                if w > 0.0 && h > 0.0 {
                    if let Some(t) = target.as_ref() {
                        if let Some(node) = self.nodes.get_mut(t) {
                            // 缩放尺寸带小数(实测 `SY=825.5`):纹理实际像素
                            // 只能是整数,`CGINFO[SX/SY]` 读回的也应是这个
                            // 取整后的层尺寸。四舍五入与 `as u32`(截断)在
                            // `.5` 上差 1 像素,选四舍五入以保持面积最接近。
                            node.layer.kind = LayerKind::Image {
                                size: Some(SizeU32 {
                                    width: w.round() as u32,
                                    height: h.round() as u32,
                                }),
                            };
                        }
                    }
                }
            }
        }
        if !executable {
            let reason = match status {
                OpStatus::Unimplemented(reason) => reason,
                OpStatus::Executable => unreachable!(),
            };
            if let Some(node) = self.nodes.get_mut(source) {
                node.layer
                    .set_texture_status(LayerStatus::Unimplemented(reason));
            }
            if let Some(t) = target {
                if let Some(node) = self.nodes.get_mut(&t) {
                    node.layer
                        .set_texture_status(LayerStatus::Unimplemented(reason));
                }
            }
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::IMAGEOP_UNSUPPORTED,
                    format!("图像加工 `{name}` 未执行: {reason};只记录原始参数"),
                ),
            );
        } else {
            // DIV-GFX-058:已执行的图像加工重写目标层像素(引擎命令处理器
            // 立即执行)⇒ 目标层已画文本作废(`BT.CGCLEAR` 的
            // `RECTPAINT3 SET=0` 清标签面正是这条路径,es_Button.yst:4547)。
            if let Some(t) = target {
                self.clear_layer_texts(&t);
            }
        }
    }

    // ------------------------------------------------------------ FONT / TEXT

    fn apply_font(&mut self, args: &[ArgValue]) {
        let resolved = self.resolved_owned("FONT", args);
        for r in resolved {
            let Some(name) = r.name.as_deref() else {
                self.arg_slot_missing("FONT", r.slot);
                continue;
            };
            match name {
                "NAME" => match &r.value {
                    Value::Str(b) if b.is_empty() => {
                        // 空 `FONT[NAME=""]`:官方 FONT 处理器对 NAME 做名字查找
                        // ,查不到(pin 空串必然查不到)落到**槽默认 family**
                        // (REA FONT 表:NAME 默认值 = 槽默认 family;回退表
                        // `0x403b8e..0x403bdc`,docs/font-provenance.md §6)。
                        // 名字槽一旦出现就覆盖全局字体索引 ⇒ 同时清 NO/BNO
                        // (DIV-GFX-059)。
                        self.style.name = None;
                        self.style.font_no = None;
                        self.style.font_bno = None;
                    }
                    Value::Str(b) => self.style.name = Some(b.clone()),
                    _ => self.arg_type("FONT", name, "字符串", &r.value),
                },
                "COLOR" => match int_arg(&r.value) {
                    // 引擎 clamp(REA FONT 处理体 `0x444620..0x44464a`:
                    // 负值 → 0,超过 0xFFFFFF → 0xFFFFFF;**不是位掩码**)。
                    Some(n) => self.style.color = clamp_color(n),
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "COLOR2" => match int_arg(&r.value) {
                    // `es._txcolor` 的宏默认第二参数是 **-1**
                    // (`macro.yst`: `TXCOLOR ... def2="-1"`),脚本只传一个
                    // 颜色时(`\_txcolor(@FONT.OUTLINE)` / `_txcolor(@FONT.SHADE.COLOR)`)
                    // 会照发 `FONT[COLOR2=-1]`。**-1 = “未给 COLOR2”的哨兵**:
                    // 表示该 pass 用**纯色**(COLOR),不做渐变。
                    //
                    // P8 曾按 clamp(REA `0x444620..0x44464a`)把 -1 落成纯黑,
                    // 结果描边/影子被画成 `蓝→黑` 渐变(官方截图里描边是纯紫蓝、
                    // 没有黑色)。哨兵判定必须在 clamp 之前 —— 与 `_txcolor`
                    // 的 `IF[@_PINT(2)>=-1 …]` 放行 -1 的写法一致。
                    Some(n) if n < 0 => self.style.color2 = None,
                    Some(n) => self.style.color2 = Some(clamp_color(n)),
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "SX" => match int_arg(&r.value) {
                    Some(n) => self.style.size_x = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "SY" => match int_arg(&r.value) {
                    Some(n) => self.style.size_y = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "BOLD" => self.style.bold = flag_value(&r.value),
                "ANTIALIASING" => self.style.antialias = flag_value(&r.value),
                "DRAWMODE" => match int_arg(&r.value) {
                    Some(n) => self.style.draw_mode = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                // GRADATION/ROTATION:取值 + clamp(REA 0x444713/0x4447db);
                // 渲染效果未取证(方向/旋转读取点未观察到),只保存原值,
                // 由后端以 `Unverified` 处理(DIV-GFX-035 / DIV-FONT-003/004)。
                "GRADATION" => match int_arg(&r.value) {
                    Some(n) => self.style.gradation = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "ROTATION" => match int_arg(&r.value) {
                    Some(n) => self.style.rotation = n,
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                // NO:字体列表序号,值保留;真实字体由后端 `yu-font` 发现链
                // (`face_by_no`)解析(本 crate 无字体 I/O)。
                "NO" => match int_arg(&r.value) {
                    Some(n) => {
                        self.style.font_no = Some(n);
                        // NO 越界是引擎 clamp 到 `< 字体总数`;列表归字体层,
                        // 这里不判断。
                    }
                    None => self.arg_type("FONT", name, "整数", &r.value),
                },
                "BNO" => {
                    // 手册 font/font.html 无 BNO;真实脚本只用它做调试字体槽。
                    self.style.font_bno = int_arg(&r.value);
                    self.push_diag(
                        "FONT",
                        error::unimplemented(
                            error::code::TEXT_UNSUPPORTED,
                            "FONT[BNO] 无手册页/字体列表语义未取证;值已保留",
                        ),
                    );
                }
                _ => self.push_diag(
                    "FONT",
                    error::unimplemented(
                        error::code::PARAM_NONVISUAL,
                        format!("FONT 参数 `{name}` 未分类"),
                    ),
                ),
            }
        }
        // 引擎 clamp(REA 0x444428..0x444b1c);负值/越界不报错。
        // 颜色已在解析时按带符号 clamp 处理(COLOR/COLOR2);这里的其余
        // 字段 clamp 与官方一致(见 [`TextStyle::clamp`])。
        self.style.clamp();
    }

    // ------------------------------------------------------------ 内部

    /// 层像素被重写:清空该层文本内容并发全局新修订号(后端缓存随之失效)。
    /// 不删条目 —— 删除后重建会让修订号从头开始,和旧缓存撞键。
    fn clear_layer_texts(&mut self, id: &LayerId) {
        let has = self.text_draws.get(id).is_some_and(|t| !t.draws.is_empty());
        if !has {
            return;
        }
        self.text_revision_seq = self.text_revision_seq.wrapping_add(1);
        let seq = self.text_revision_seq;
        if let Some(texts) = self.text_draws.get_mut(id) {
            texts.draws.clear();
            texts.seen.clear();
            texts.dropped = 0;
            texts.revision = seq;
        }
    }

    fn w0() -> LayerId {
        LayerId::new(b":/W0".to_vec())
    }

    fn resolved_owned(&self, command: &str, args: &[ArgValue]) -> Vec<OwnedArg> {
        self.params
            .resolve(command, args)
            .into_iter()
            .map(|r| OwnedArg {
                name: r.name.map(str::to_string),
                slot: r.slot,
                value: r.value.clone(),
            })
            .collect()
    }

    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    fn next_copy(&mut self) -> u64 {
        let c = self.copy_seq;
        self.copy_seq += 1;
        c
    }

    fn next_texture_seq(&mut self) -> u64 {
        let s = self.texture_seq;
        self.texture_seq += 1;
        s
    }

    fn ensure_w0(&mut self) {
        let w0 = Self::w0();
        if self.nodes.contains_key(&w0) {
            return;
        }
        let layer = Layer {
            id: w0.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Group {
                size: Some(viewport_to_size(self.config.viewport)),
            },
            texture: TextureRef::None,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            texture_status: LayerStatus::Executable,
            param_status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.nodes.insert(
            w0,
            Node {
                layer,
                parent: None,
                children: BTreeSet::new(),
            },
        );
    }

    fn create_group(&mut self, path: &LayerId) {
        if self.nodes.contains_key(path) {
            return;
        }
        self.ensure_parents(path);
        let layer = Layer {
            id: path.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Group { size: None },
            texture: TextureRef::None,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            texture_status: LayerStatus::Executable,
            param_status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.insert_node(path, layer);
    }

    fn create_image(&mut self, path: &LayerId, texture: TextureRef, size: Option<SizeU32>) {
        if self.nodes.contains_key(path) {
            return;
        }
        self.ensure_parents(path);
        let layer = Layer {
            id: path.clone(),
            seq: self.next_seq(),
            z: 0,
            alpha: 256,
            blend: BlendMode::Normal,
            visible: true,
            kind: LayerKind::Image { size },
            texture,
            parts: Vec::new(),
            transform: Default::default(),
            mask: None,
            transition: None,
            clip: None,
            status: LayerStatus::Executable,
            texture_status: LayerStatus::Executable,
            param_status: LayerStatus::Executable,
            extra: BTreeMap::new(),
        };
        self.insert_node(path, layer);
    }

    /// 补齐父路径。迭代向上收集缺失祖先后从上往下建,避免深路径递归爆栈。
    fn ensure_parents(&mut self, path: &LayerId) {
        let mut missing = Vec::new();
        let mut cur = path::parent(path);
        while let Some(p) = cur {
            if path::is_root(&p) || self.nodes.contains_key(&p) {
                break;
            }
            missing.push(p.clone());
            cur = path::parent(&p);
        }
        for p in missing.into_iter().rev() {
            self.create_group(&p);
        }
    }

    fn insert_node(&mut self, path: &LayerId, layer: Layer) {
        let parent = path::parent(path).filter(|p| !path::is_root(p));
        self.nodes.insert(
            path.clone(),
            Node {
                layer,
                parent: parent.clone(),
                children: BTreeSet::new(),
            },
        );
        if let Some(parent) = parent {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.insert(path.clone());
            }
        }
    }

    fn remove_subtree(&mut self, path: &LayerId) -> BTreeSet<LayerId> {
        let mut removed = BTreeSet::new();
        let mut stack = vec![path.clone()];
        while let Some(p) = stack.pop() {
            if let Some(node) = self.nodes.remove(&p) {
                removed.insert(p.clone());
                stack.extend(node.children);
            }
        }
        if let Some(parent) = path::parent(path).filter(|p| !path::is_root(p)) {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.remove(path);
            }
        }
        removed
    }

    fn drop_ops_for(&mut self, removed: &BTreeSet<LayerId>) {
        // 快照是独立纹理:源图层被删后,已完成的复制仍需留给后端物化。
        self.copy_log.retain(|c| !removed.contains(&c.dest));
        // 图像加工同理:只要目标还在,操作历史仍描述目标纹理。
        self.image_log
            .retain(|i| i.target.as_ref().is_none_or(|t| !removed.contains(t)));
    }

    fn sorted_children(&self, parent: Option<&LayerId>) -> Vec<LayerId> {
        let ids: Vec<LayerId> = if let Some(parent) = parent {
            self.nodes
                .get(parent)
                .map(|n| n.children.iter().cloned().collect())
                .unwrap_or_default()
        } else {
            self.nodes
                .values()
                .filter(|n| n.parent.is_none())
                .map(|n| n.layer.id.clone())
                .collect()
        };
        // 预先取 (z,seq,id) 键,排序比较期不再查 BTreeMap / 分配。
        let mut keyed: Vec<(i64, u64, LayerId)> = ids
            .into_iter()
            .map(|id| {
                let layer = &self.nodes[&id].layer;
                (layer.z, layer.seq, id)
            })
            .collect();
        keyed.sort();
        keyed.into_iter().map(|(_, _, id)| id).collect()
    }

    /// 组是否有可见内容(显式栈;深树不递归)。
    fn group_has_visible_content(&self, id: &LayerId) -> bool {
        let Some(node) = self.nodes.get(id) else {
            return false;
        };
        let mut stack: Vec<LayerId> = node.children.iter().cloned().collect();
        let mut seen = BTreeSet::new();
        while let Some(c) = stack.pop() {
            if !seen.insert(c.clone()) {
                continue;
            }
            let Some(n) = self.nodes.get(&c) else {
                continue;
            };
            if n.layer.is_group() {
                // 组内容不直接进父目标;E=0 的组不合成,跳过其子树。
                if n.layer.visible {
                    stack.extend(n.children.iter().cloned());
                }
            } else {
                if n.layer.visible && !matches!(n.layer.texture, TextureRef::None) {
                    return true;
                }
                // 引擎先递归子层再判本层 E:隐藏图像层的子层仍可能可见。
                stack.extend(n.children.iter().cloned());
            }
        }
        false
    }

    /// 所有组离屏 pass,后序(子组先于父组)。显式栈遍历全树,包括
    /// 挂在图像层下面的组。
    fn build_group_passes(&self, out: &mut Vec<Pass>, emitted: &mut BTreeSet<LayerId>) {
        // 后序:进入节点时先压“展开”标记,再压子节点;展开时记录组。
        let mut order: Vec<LayerId> = Vec::new();
        let mut stack: Vec<(LayerId, bool)> = self
            .sorted_children(None)
            .into_iter()
            .rev()
            .map(|id| (id, false))
            .collect();
        while let Some((id, expanded)) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if expanded {
                if node.layer.is_group() {
                    order.push(id);
                }
                continue;
            }
            if node.layer.is_group() {
                stack.push((id.clone(), true));
            }
            for child in self.sorted_children(Some(&id)).into_iter().rev() {
                stack.push((child, false));
            }
        }

        // 主窗口组**直绘**到后缓冲(DIV-GFX-008/041),不再产出离屏 pass:
        // 否则子层会被画两遍(先组 RT、再屏幕),报告计数与像素都不对。
        let main = self
            .windows
            .get(&0)
            .and_then(|w| w.layer_set.clone())
            .unwrap_or_else(|| self.config.main_layer_set.clone());
        for id in order {
            if id == main {
                continue;
            }
            let mut ops = Vec::new();
            self.emit_children_ops(&id, &mut ops, emitted);
            if ops.is_empty() {
                continue;
            }
            out.push(Pass {
                target: PassTarget::Offscreen(id.clone()),
                ops,
                // TODO(DIV-GFX-008):引擎非调试路径的组 RT 清屏/尺寸未取证;
                // 当前按透明黑 + 视口由后端收紧。
                clear: Some(Clear::explicit([0, 0, 0, 0])),
            });
            emitted.insert(id);
        }
    }

    /// 把一个子节点写进当前目标:组 → 已生成的离屏纹理;
    /// 图像 → 自身子树(引擎先递归子层,再画本层)。
    fn emit_node_ops(&self, id: &LayerId, out: &mut Vec<Op>, emitted: &BTreeSet<LayerId>) {
        let Some(node) = self.nodes.get(id) else {
            return;
        };
        if node.layer.is_group() {
            if node.layer.visible && emitted.contains(id) && self.group_has_visible_content(id) {
                if let Some(op) = self.group_transition_op(id, &node.layer) {
                    out.push(op);
                } else {
                    out.push(Op::Group(crate::group::GroupDraw {
                        layer: node.layer.clone(),
                        offscreen: id.clone(),
                    }));
                }
            }
            return;
        }
        self.emit_image_subtree(id, out, emitted);
    }

    /// 组(layer set)上的 `T/TA/TID`:引擎按层表统一解析配对(组也是层),
    /// 把组离屏纹理当作 `YS_Texture1`;配对可以是图像层或组。
    /// 未取证到独立微场景前标 `Likely`,但不再静默当普通组画掉。
    fn group_transition_op(&self, id: &LayerId, layer: &Layer) -> Option<Op> {
        let t = layer.transition.as_ref()?;
        let fade = t.fade_value()?;
        let tid = t.tid.as_ref()?;
        let to = self.transition_pair(tid)?;
        let mut l = layer.clone();
        l.set_texture_status(LayerStatus::Executable);
        Some(Op::Transition(TransitionDraw {
            from: TextureRef::Offscreen(id.clone()),
            to: Some(to),
            fade,
            t: t.t,
            ta: t.ta,
            tid: t.tid.clone(),
            from_layer: Some(id.clone()),
            // 组过渡的 `from` 是组离屏纹理,配对层即 TID 本身。
            to_layer: Some(tid.clone()),
            layer: l,
        }))
    }

    fn emit_children_ops(&self, parent: &LayerId, out: &mut Vec<Op>, emitted: &BTreeSet<LayerId>) {
        for child in self.sorted_children(Some(parent)) {
            self.emit_node_ops(&child, out, emitted);
        }
    }

    /// 图像节点及其非组后代的绘制顺序 = 引擎后序:子层全部先于本层。
    /// 组子节点只输出 `Op::Group`(其内部在本组的 Offscreen pass)。
    fn emit_image_subtree(&self, id: &LayerId, out: &mut Vec<Op>, emitted: &BTreeSet<LayerId>) {
        enum Frame {
            Enter(LayerId),
            Group(LayerId),
            Emit(LayerId),
        }
        let mut stack = vec![Frame::Enter(id.clone())];
        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Enter(id) => {
                    let Some(node) = self.nodes.get(&id) else {
                        continue;
                    };
                    stack.push(Frame::Emit(id.clone()));
                    for child in self.sorted_children(Some(&id)).into_iter().rev() {
                        if self.nodes.get(&child).is_some_and(|n| n.layer.is_group()) {
                            stack.push(Frame::Group(child));
                        } else {
                            stack.push(Frame::Enter(child));
                        }
                    }
                    let _ = node;
                }
                Frame::Group(id) => {
                    let Some(node) = self.nodes.get(&id) else {
                        continue;
                    };
                    if node.layer.visible
                        && emitted.contains(&id)
                        && self.group_has_visible_content(&id)
                    {
                        if let Some(op) = self.group_transition_op(&id, &node.layer) {
                            out.push(op);
                        } else {
                            out.push(Op::Group(crate::group::GroupDraw {
                                layer: node.layer.clone(),
                                offscreen: id,
                            }));
                        }
                    }
                }
                Frame::Emit(id) => {
                    let Some(node) = self.nodes.get(&id) else {
                        continue;
                    };
                    if node.layer.visible && !matches!(node.layer.texture, TextureRef::None) {
                        out.push(self.op_for_layer(&node.layer));
                    }
                }
            }
        }
    }

    fn op_for_layer(&self, layer: &Layer) -> Op {
        if !layer.parts.is_empty() {
            let mut l = layer.clone();
            l.set_texture_status(LayerStatus::Unimplemented("gfx.parts.unsupported"));
            return Op::Layer(l);
        }
        // 空 TID = 引擎不查配对(`0x413e7b` 空串直接跳过);
        // 脚本把它当“结束过渡/初始化”,因此这里按普通层输出。
        //
        // EVID-T-03:配对层存在且纹理可绘制 + TA>=1 才走双纹理;
        // 任一不满足都回退单纹理普通层(asm 0x415558-0x415577 → 0x416f8d)。
        if let Some(t) = &layer.transition {
            if let (Some(fade), Some(tid)) = (t.fade_value(), t.tid.as_ref()) {
                // 引擎语义(asm `0x415558-0x415577` → 双纹理 pass):
                // `YS_Texture1` = **本层当前纹理**,`YS_Texture2` = `TID` 解析出的
                // 配对层纹理。脚本先把真实像素做进本层、把 mask 做进配对层
                // (boot logo 用 `CGACT[SIZE]`/`CGACT[REVERSE]`),因此这里只按
                // 层状态取纹理,不含任何图层命名约定(EVID-T-10)。
                let from = layer.texture.clone();
                let to = self.transition_pair(tid);
                if let Some(to) = to {
                    let mut l = layer.clone();
                    l.set_texture_status(LayerStatus::Executable);
                    return Op::Transition(TransitionDraw {
                        from,
                        to: Some(to),
                        fade,
                        t: t.t,
                        ta: t.ta,
                        tid: t.tid.clone(),
                        from_layer: Some(layer.id.clone()),
                        to_layer: Some(tid.clone()),
                        layer: l,
                    });
                }
            }
        }
        if let Some(mask) = &layer.mask {
            return Op::Mask(MaskDraw {
                layer: layer.clone(),
                mask: mask.clone(),
            });
        }
        Op::Layer(layer.clone())
    }

    /// `TID` 配对层 → 可取样纹理(引擎 `YS_Texture2`)。
    ///
    /// 组 → 离屏纹理;图像层 → 该层当前纹理。空纹理(= 层不存在内容)或
    /// 无可见内容的组视为“配对不可用”,由调用方回退单纹理普通层
    /// (asm `0x415558-0x415577` 同族判定)。
    fn transition_pair(&self, tid: &LayerId) -> Option<TextureRef> {
        self.nodes.get(tid).and_then(|n| {
            if n.layer.is_group() {
                if self.group_has_visible_content(tid) {
                    Some(TextureRef::Offscreen(tid.clone()))
                } else {
                    None
                }
            } else {
                match &n.layer.texture {
                    TextureRef::None => None,
                    texture => Some(texture.clone()),
                }
            }
        })
    }

    fn push_diag(&mut self, command: &str, d: Diag) {
        let key = (d.code.to_string(), command.to_string(), d.detail.clone());
        match self.diags.get_mut(&key) {
            Some(existing) => existing.count = existing.count.saturating_add(1),
            None => {
                self.diags.insert(
                    key,
                    error::FrameDiag::new(d.code.to_string(), d.severity, command, d.detail),
                );
            }
        }
    }

    fn push_copy_log(&mut self, op: CopyOp) {
        if self.copy_log.len() >= MAX_OP_LOG {
            let drop = self.copy_log.len() / 2;
            self.copy_log.drain(..drop);
            self.copy_log_dropped += drop as u64;
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::LOG_TRUNCATED,
                    format!("复制日志达到上限 {MAX_OP_LOG},最旧项被丢弃"),
                ),
            );
        }
        self.copy_log.push(op);
    }

    fn push_image_log(&mut self, op: ImageOp) {
        self.supersede_image_op(&op);
        if self.image_log.len() >= MAX_OP_LOG {
            let drop = self.image_log.len() / 2;
            self.image_log.drain(..drop);
            self.image_log_dropped += drop as u64;
            self.push_diag(
                "CGACT",
                error::unimplemented(
                    error::code::LOG_TRUNCATED,
                    format!("图像日志达到上限 {MAX_OP_LOG},最旧项被丢弃"),
                ),
            );
        }
        self.image_log.push(op);
    }

    /// 丢弃被新 `SIZE/SIZE2` 完全覆盖的旧缩放项(同源同目标)。
    ///
    /// 引擎的 `CGACT` 图像加工是**破坏性立即执行**(REA `0x42f809`:分配新缓冲 →
    /// 从层当前纹理读 → 写回层),缩放动画因此每帧真的重算一次。本内核不能在
    /// 命令时刻做像素工作(纹理在后端),只能把加工项逐帧重放;若不折叠,
    /// 立绘缩放/移动每帧追加一条 `SIZE` ⇒ `image_log` 线性增长 ⇒ `frame()`
    /// 为每条产出一个 pass ⇒ 渲染耗时随游玩时长线性上升(实测 passes
    /// 103→139、skip 累计到 336 的 resize 尖峰就是这条)。
    ///
    /// 只折叠 `SIZE/SIZE2`,因为后端对它们**只读源层**:
    /// `apply_image_op` 无区域时直接复用源纹理,有区域时 `scale_region` 从源
    /// 区域重建一整张目标纹理 —— 两种路径都不读目标旧像素。反过来
    /// `RECTPAINT`(RGB 填充保留目标 alpha)、`REVERSE/HORIZONTAL`(变换目标
    /// 自身)都以目标旧像素为输入,折叠会改变结果,故一律不折叠。
    ///
    /// 追加条件:`(源, 目标)` 相同、源 ≠ 目标(原地缩放会叠加)、且旧项之后
    /// 没有任何项把该目标当源读过(有 ⇒ 中间像素仍被引用)。
    fn supersede_image_op(&mut self, op: &ImageOp) {
        if !op.action.is_scale() {
            return;
        }
        let target = op.target.as_ref().unwrap_or(&op.source);
        if target == &op.source {
            return;
        }
        let Some(pos) = self.image_log.iter().rposition(|old| {
            old.action.is_scale()
                && old.source == op.source
                && old.target.as_ref().unwrap_or(&old.source) == target
        }) else {
            return;
        };
        let later_seq = self.image_log[pos].seq;
        let read_later = self
            .image_log
            .iter()
            .skip(pos + 1)
            .any(|later| &later.source == target)
            || self.copy_log.iter().any(|copy| {
                copy.seq > later_seq
                    && matches!(&copy.source, CopySource::Layer(id) if id == target)
            });
        if read_later {
            return;
        }
        self.image_log.remove(pos);
    }

    fn arg_slot_missing(&mut self, command: &str, slot: u8) {
        self.push_diag(
            command,
            error::unimplemented(
                error::code::ARG_MISSING,
                format!("槽 {slot} 不在 `{command}` 的 ysc 参数表内"),
            ),
        );
    }

    fn arg_type(&mut self, command: &str, name: &str, expected: &str, value: &Value) {
        self.push_diag(
            command,
            error::recoverable(
                error::code::ARG_TYPE,
                format!("参数 `{name}` 期望 {expected},实际 {}", value.type_name()),
            ),
        );
    }
}

// ---------------------------------------------------------------- 参数应用

fn viewport_to_size(v: Viewport) -> SizeU32 {
    SizeU32 {
        width: v.width,
        height: v.height,
    }
}

/// 引擎颜色 clamp:带符号,负值 → 0,超过 0xFFFFFF → 0xFFFFFF
/// (REA FONT 处理体 `0x444620..0x44464a` 的 `SBB/JL/XOR` 与
/// `MOV EAX,0xFFFFFF` 分支;`FONT[COLOR2=-1]` 因此落到纯黑)。
fn clamp_color(n: i64) -> u32 {
    n.clamp(0, 0x00FF_FFFF) as u32
}

fn int_arg(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.is_finite() => Some(*f as i64),
        _ => None,
    }
}

/// INFO 查询必须只置位一个字段;多字段写回顺序未取证。
fn single_flag<'a>(
    flags: &BTreeMap<&'a str, i64>,
    command: &str,
) -> Result<Option<(&'a str, i64)>, Diag> {
    if flags.len() > 1 {
        return Err(error::unimplemented(
            error::code::INFO_MULTI,
            format!(
                "{command} 同时置位 {} 个查询字段;多字段 LET 写回顺序未取证",
                flags.len()
            ),
        ));
    }
    Ok(flags.iter().next().map(|(k, v)| (*k, *v)))
}

fn arg_type_diag(command: &str, name: &str, expected: &str) -> Diag {
    error::recoverable(
        error::code::ARG_TYPE,
        format!("{command}[{name}] 期望 {expected}"),
    )
}

fn flag_value(v: &Value) -> bool {
    match v {
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Str(b) => !b.is_empty(),
    }
}

fn find_value(params: &[(String, EngineValue)], name: &str) -> Option<EngineValue> {
    params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone())
}

fn find_int(params: &[(String, EngineValue)], name: &str) -> Option<i64> {
    find_value(params, name).and_then(|v| v.as_int())
}

/// 取浮点参数(整数原值也接受)。
///
/// `CGACT[SIZE/SIZE2]` 的 `SX/SY` 是缩放后的尺寸,实测带小数
/// (`SY=825.5`);`find_int` 的 `fract()==0` 守卫会把它们判成「未给」。
fn find_f64(params: &[(String, EngineValue)], name: &str) -> Option<f64> {
    find_value(params, name).and_then(|v| v.as_f64())
}

/// 取**像素坐标**参数:接受整数与浮点(浮点四舍五入成整数像素)。
///
/// 与 [`find_int`] 的区别:后者要求 `fract()==0`,会把
/// `CGACT[… Y2=824.5]` 这类真实参数判成「未给」,进而让区域只剩 3 个字段
/// 并触发 `gfx.imageop.region.partial`(sprite 整条 SIZE 被跳过)。
fn find_px(params: &[(String, EngineValue)], name: &str) -> Option<i64> {
    find_value(params, name).and_then(|v| v.as_f64()).map(|f| f.round() as i64)
}

fn find_str(params: &[(String, EngineValue)], name: &str) -> Option<Vec<u8>> {
    find_value(params, name).and_then(|v| v.as_bytes().map(|b| b.to_vec()))
}

fn flag(params: &[(String, EngineValue)], name: &str) -> bool {
    find_value(params, name).is_some_and(|v| match v {
        EngineValue::Int(i) => i != 0,
        EngineValue::Float(f) => f != 0.0,
        EngineValue::Str(b) => !b.is_empty(),
    })
}

fn take_file(params: &[(String, EngineValue)], diags: &mut Vec<Diag>) -> Option<TextureRef> {
    for (name, value) in params {
        if name != "FILE" {
            continue;
        }
        return match value {
            EngineValue::Str(b) if b.is_empty() => Some(TextureRef::SolidBlack1x1),
            EngineValue::Str(b) => Some(TextureRef::Asset(b.clone())),
            _ => {
                diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `FILE` 期望字符串",
                ));
                None
            }
        };
    }
    None
}

fn mark_param_unimplemented(layer: &mut Layer, reason: &'static str) {
    if matches!(layer.param_status, LayerStatus::Executable) {
        layer.param_status = LayerStatus::Unimplemented(reason);
        layer.refresh_status();
    }
}

fn copy_attributes(src: &Layer, dst: &mut Layer) {
    dst.alpha = src.alpha;
    dst.blend = src.blend;
    dst.visible = src.visible;
    dst.z = src.z;
    dst.transform = src.transform.clone();
    dst.parts = src.parts.clone();
    dst.mask = src.mask.clone();
    dst.transition = src.transition.clone();
    dst.clip = src.clip;
    dst.extra = src.extra.clone();
    dst.texture_status = src.texture_status;
    dst.param_status = src.param_status;
    dst.refresh_status();
}

/// RECTPAINT/2/3 的可执行前置条件(EVID-J-02..05 / EVID-T-11)。
///
/// 只放行已取证的组合:原地加工、SET 为整数、无 SET2/SETSTR/SX/SY、
/// 区域要么全省略(整张纹理)要么四个坐标齐全、基图为 Asset / 1×1 黑图 /
/// 可执行 COPY 产生的快照(后端的快照表面)。ID2、部分区域、组目标等
/// 没有像素级证据的组合返回具体 reason,保持挂账。
#[allow(clippy::too_many_arguments)]
fn rect_paint_precheck(
    node: Option<&Node>,
    set: &Option<EngineValue>,
    set2: &Option<EngineValue>,
    set_str: &Option<Vec<u8>>,
    sx: Option<f64>,
    sy: Option<f64>,
    x: Option<i64>,
    y: Option<i64>,
    x2: Option<i64>,
    y2: Option<i64>,
) -> Result<(), &'static str> {
    if set.as_ref().and_then(EngineValue::as_int).is_none() {
        return Err("gfx.imageop.set.missing");
    }
    if set2.is_some() {
        return Err("gfx.imageop.set2.unsupported");
    }
    if set_str.is_some() {
        return Err("gfx.imageop.setstr.unsupported");
    }
    if sx.is_some() || sy.is_some() {
        return Err("gfx.imageop.sx.unsupported");
    }
    let region_fields = [x.is_some(), y.is_some(), x2.is_some(), y2.is_some()]
        .into_iter()
        .filter(|present| *present)
        .count();
    if region_fields != 0 && region_fields != 4 {
        return Err("gfx.imageop.region.partial");
    }
    if let (Some(x), Some(y), Some(x2), Some(y2)) = (x, y, x2, y2) {
        if x2 < x || y2 < y {
            return Err("gfx.imageop.region.invalid");
        }
    }
    let node = node.ok_or("gfx.imageop.target.unsupported")?;
    match &node.layer.texture {
        TextureRef::Asset(_) | TextureRef::SolidBlack1x1 | TextureRef::Snapshot(_) => {}
        TextureRef::Offscreen(_) => return Err("gfx.imageop.target.group"),
        TextureRef::None => return Err("gfx.imageop.texture.none"),
    }
    // 先前的未实现加工/复制已经让基图不可信;不在其上叠加填充。
    if node.layer.texture_status != LayerStatus::Executable {
        return Err("gfx.imageop.target.unsupported");
    }
    Ok(())
}

/// 真实 ysc 表(0.495 ERIS)的全部图像动作参数。
static IMAGE_ACTIONS: &[(&str, ImageAction)] = &[
    ("RECTPAINT", ImageAction::RectPaint),
    ("RECTPAINT2", ImageAction::RectPaint2),
    ("RECTPAINT3", ImageAction::RectPaint3),
    ("TEXT", ImageAction::Text),
    ("SIZE", ImageAction::SizeNearest),
    ("SIZE2", ImageAction::SizeBilinear),
    ("SIZE3", ImageAction::Other("size3")),
    ("SIZE4", ImageAction::Other("size4")),
    ("SIZE5", ImageAction::Other("size5")),
    ("ROTATION", ImageAction::Rotation),
    ("ROTATION2", ImageAction::Other("rotation2")),
    ("RESIZE", ImageAction::Resize),
    ("TRIM", ImageAction::Trim),
    ("MERGE", ImageAction::Merge),
    ("MERGE2", ImageAction::Merge2),
    ("TRANSITION", ImageAction::Other("transition")),
    ("TRANSITION2", ImageAction::Other("transition2")),
    ("XOR", ImageAction::Other("xor")),
    ("AND", ImageAction::Other("and")),
    ("SAIDO", ImageAction::Other("saido")),
    ("MEIDO", ImageAction::Other("meido")),
    ("SEPIA", ImageAction::Other("sepia")),
    ("AKARUSA", ImageAction::Other("akarusa")),
    ("MONO", ImageAction::Other("mono")),
    ("VERTICAL", ImageAction::Other("vertical")),
    ("HORIZONTAL", ImageAction::Horizontal),
    ("MOSAIC", ImageAction::Other("mosaic")),
    ("BLUR", ImageAction::Other("blur")),
    ("BLUR2", ImageAction::Other("blur2")),
    ("BLUR3", ImageAction::Other("blur3")),
    ("BLUR4", ImageAction::Other("blur4")),
    ("BLUR5", ImageAction::Other("blur5")),
    ("BLUR6", ImageAction::Other("blur6")),
    ("DIFFUSION", ImageAction::Other("diffusion")),
    ("REVERSE", ImageAction::Reverse),
    ("WHIRLPOOL", ImageAction::Other("whirlpool")),
    ("RASTER", ImageAction::Other("raster")),
    ("RASTER1", ImageAction::Other("raster1")),
    ("RASTER2", ImageAction::Other("raster2")),
    ("RASTER3", ImageAction::Other("raster3")),
];

fn value_truthy(v: &EngineValue) -> bool {
    match v {
        EngineValue::Int(i) => *i != 0,
        EngineValue::Float(f) => *f != 0.0,
        EngineValue::Str(b) => !b.is_empty(),
    }
}

/// 一次扫描得到“被置位的参数名”,避免每个动作重复全表查找。
fn active_params(params: &[(String, EngineValue)]) -> BTreeSet<&str> {
    params
        .iter()
        .filter(|(_, v)| value_truthy(v))
        .map(|(n, _)| n.as_str())
        .collect()
}

fn count_image_actions(params: &[(String, EngineValue)]) -> usize {
    let active = active_params(params);
    IMAGE_ACTIONS
        .iter()
        .filter(|(candidate, _)| active.contains(candidate))
        .count()
}

fn first_image_action(params: &[(String, EngineValue)]) -> Option<(&'static str, ImageAction)> {
    let active = active_params(params);
    // 参数顺序即编译器窗口顺序;取第一个出现的动作。
    for (name, _) in params {
        if !active.contains(name.as_str()) {
            continue;
        }
        for (candidate, action) in IMAGE_ACTIONS {
            if name == candidate {
                return Some((candidate, action.clone()));
            }
        }
    }
    None
}

/// 应用 `CG` 属性参数。返回诊断(不直接写 self,避免借用冲突)。
fn apply_layer_params(
    layer: &mut Layer,
    params: &[(String, EngineValue)],
    is_group: bool,
) -> Vec<Diag> {
    let mut diags = Vec::new();
    // 每次属性更新都从“本组参数”重新判定;纹理级状态保留(CGACT 之后仍有效)。
    layer.param_status = LayerStatus::Executable;
    let mut sx: Option<i64> = None;
    let mut sy: Option<i64> = None;
    for (name, value) in params {
        match name.as_str() {
            "FILE" | "ID" | "IDNO" => {}
            "E" => match value.as_int() {
                Some(n) => layer.visible = n != 0,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `E` 期望整数",
                )),
            },
            "A" => match value.as_int() {
                Some(n) => match i16::try_from(n) {
                    Ok(v) => layer.alpha = v,
                    Err(_) => diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `A={n}` 超出 s16 存储范围"),
                    )),
                },
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `A` 期望整数",
                )),
            },
            "X" => match value.as_int() {
                Some(n) => layer.transform.x = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `X` 期望整数",
                )),
            },
            "Y" => match value.as_int() {
                Some(n) => layer.transform.y = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `Y` 期望整数",
                )),
            },
            "Z" => match value.as_int() {
                Some(n) => layer.z = n,
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `Z` 期望整数",
                )),
            },
            "SX" => sx = value.as_int(),
            "SY" => sy = value.as_int(),
            "MODE" => match value.as_int() {
                Some(n) => match BlendMode::from_code(n) {
                    Some(m) => {
                        layer.blend = m;
                        match m.confidence() {
                            BlendConfidence::Confirmed | BlendConfidence::Likely => {}
                            BlendConfidence::Unverified => {
                                mark_param_unimplemented(layer, "gfx.blend.unverified");
                                diags.push(error::unimplemented(
                                    error::code::BLEND_UNVERIFIED,
                                    format!("MODE={n}({}) 固定管线状态已取证,语义未对拍", m.name()),
                                ));
                            }
                            BlendConfidence::Unimplemented => {
                                mark_param_unimplemented(layer, "gfx.blend.unsupported");
                                diags.push(error::unimplemented(
                                    error::code::BLEND_UNSUPPORTED,
                                    format!("MODE={n}({}) 着色器路径未取证", m.name()),
                                ));
                            }
                        }
                    }
                    None => {
                        layer.extra.insert("MODE".to_string(), EngineValue::Int(n));
                        mark_param_unimplemented(layer, "gfx.blend.unsupported");
                        diags.push(error::unimplemented(
                            error::code::BLEND_UNSUPPORTED,
                            format!("MODE={n} 超出手册 0..18"),
                        ));
                    }
                },
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `MODE` 期望整数",
                )),
            },
            "T" | "TA" => {
                let Some(n) = value.as_int() else {
                    diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `{name}` 期望整数"),
                    ));
                    continue;
                };
                let Ok(v) = i16::try_from(n) else {
                    diags.push(error::recoverable(
                        error::code::ARG_TYPE,
                        format!("参数 `{name}={n}` 超出 s16 存储范围"),
                    ));
                    continue;
                };
                let t = layer.transition.get_or_insert_with(Transition::default);
                if name == "T" {
                    t.t = v;
                } else {
                    t.ta = v;
                }
            }
            // TODO(DIV-GFX-030):引擎可能按 `TID + "." + N` 解析 TIDNO;
            // 未重新锚定前只留档,不当作 TID 使用。
            "TIDNO" => {
                layer.extra.insert("TIDNO".to_string(), value.clone());
                diags.push(error::unimplemented(
                    error::code::PARAM_NONVISUAL,
                    "CG[TIDNO] 的 IDNO 变体未取证;已原样保留",
                ));
            }
            "TID" => match value.as_bytes() {
                // 引擎 `FUN_00413dc8` 0x413e7b:空串直接跳过配对查询。
                // 脚本 `TID=""` 的意图是“结束过渡/初始化”(es_Button.yst:4526)。
                Some([]) => {
                    let t = layer.transition.get_or_insert_with(Transition::default);
                    t.tid = None;
                    t.tid_raw = Some(Vec::new());
                }
                Some(b) => {
                    let p = path::normalize(b, None, false).path;
                    let t = layer.transition.get_or_insert_with(Transition::default);
                    t.tid = Some(p);
                    t.tid_raw = Some(b.to_vec());
                }
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `TID` 期望字符串",
                )),
            },
            "MID" => match value.as_bytes() {
                // 与 TID 同法:空串 = 不设置外部掩码(+0x210 空串跳过)。
                Some([]) => layer.mask = None,
                Some(b) => {
                    // TODO(DIV-GFX-005):MID 取值通道(α/亮度)/尺寸未取证。
                    layer.mask = Some(path::normalize(b, None, false).path);
                    mark_param_unimplemented(layer, "gfx.mask.unsupported");
                    diags.push(error::unimplemented(
                        error::code::MASK_UNSUPPORTED,
                        "外部掩码 MID 的取值通道(α/亮度)未取证",
                    ));
                }
                None => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    "参数 `MID` 期望字符串",
                )),
            },
            other => {
                if let Some(fp) = FilterParam::from_name(other) {
                    // TODO(DIV-GFX-003):槽位→+0x1b0..+0x200 绑定/变换公式未取证。
                    layer.transform.filter.set(fp, value.clone());
                    mark_param_unimplemented(layer, "gfx.filter.unsupported");
                    diags.push(error::unimplemented(
                        error::code::FILTER_UNSUPPORTED,
                        format!("滤镜/仿射参数 `{other}` 的槽位→偏移绑定未取证"),
                    ));
                } else {
                    layer.extra.insert(other.to_string(), value.clone());
                    diags.push(error::unimplemented(
                        error::code::PARAM_NONVISUAL,
                        format!("参数 `{other}` 已原样保留,视觉语义未分类"),
                    ));
                }
            }
        }
    }

    // 持久字段重算:本命令没重复给 FX/MID/MODE 时,旧值仍可能让本层不可执行。
    // (诊断只在参数出现时发,避免每帧重复计数。)
    if layer.extra.contains_key("MODE") {
        mark_param_unimplemented(layer, "gfx.blend.unsupported");
    } else {
        match layer.blend.confidence() {
            BlendConfidence::Unverified => mark_param_unimplemented(layer, "gfx.blend.unverified"),
            BlendConfidence::Unimplemented => {
                mark_param_unimplemented(layer, "gfx.blend.unsupported")
            }
            _ => {}
        }
    }
    if !layer.transform.filter.is_empty() {
        mark_param_unimplemented(layer, "gfx.filter.unsupported");
    }
    if layer.mask.is_some() {
        mark_param_unimplemented(layer, "gfx.mask.unsupported");
    }

    if is_group {
        let current = layer.group_size();
        let sx = sx.or(current.map(|s| i64::from(s.width)));
        let sy = sy.or(current.map(|s| i64::from(s.height)));
        if let (Some(sx), Some(sy)) = (sx, sy) {
            match (u32::try_from(sx), u32::try_from(sy)) {
                (Ok(w), Ok(h)) => {
                    layer.kind = LayerKind::Group {
                        size: Some(SizeU32 {
                            width: w,
                            height: h,
                        }),
                    };
                }
                _ => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    format!("图层组 SX={sx} SY={sy} 超出 u32 范围"),
                )),
            }
        }
    } else {
        let size_params_present = sx.is_some() || sy.is_some();
        if let Some(v) = sx {
            layer.extra.insert("SX".to_string(), EngineValue::Int(v));
        }
        if let Some(v) = sy {
            layer.extra.insert("SY".to_string(), EngineValue::Int(v));
        }
        match (sx, sy) {
            (Some(w), Some(h)) => match (u32::try_from(w), u32::try_from(h)) {
                (Ok(w), Ok(h)) => {
                    layer.kind = LayerKind::Image {
                        size: Some(SizeU32 {
                            width: w,
                            height: h,
                        }),
                    };
                }
                _ => diags.push(error::recoverable(
                    error::code::ARG_TYPE,
                    format!("图像层 SX={w} SY={h} 超出 u32 范围"),
                )),
            },
            (Some(_), None) | (None, Some(_)) => diags.push(error::unimplemented(
                error::code::IMAGE_SIZE_UNSUPPORTED,
                "图像层只给 SX/SY 之一:部分尺寸更新未取证",
            )),
            (None, None) => {}
        }
        // 引擎层矩形(+0x78/+0x7c)独立于纹理。`FILE=""` 的 1×1 黑图
        // 按 SX/SY 铺满(真实脚本 es_ERIS.yst:579);资源图的缩放/裁剪
        // 没有真实样本,保持 Unimplemented。
        if let Some(_size) = layer.image_size() {
            if !matches!(&layer.texture, TextureRef::SolidBlack1x1 | TextureRef::None) {
                mark_param_unimplemented(layer, "gfx.image.size.unsupported");
                if size_params_present {
                    diags.push(error::unimplemented(
                        error::code::IMAGE_SIZE_UNSUPPORTED,
                        "资源图 + SX/SY 的缩放/裁剪语义未取证;仅 FILE=\"\" 黑图按尺寸绘制",
                    ));
                }
            }
        }
    }
    layer.refresh_status();
    diags
}
