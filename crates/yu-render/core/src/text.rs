//! 文本栅格化与合成(两条引擎文本路径共用)。
//!
//! # 路径
//!
//! 1. **`CGACT[TEXT=1]`**(ERIS 对话文本的真实路径,手册 `cg/cgact.html`
//!    例 25/26):每个绘制项在目标图层局部坐标按 `FONT` 快照绘制字符串。
//!    字符步进用字形 advance(引擎逐字符 `TextOutA`,DIV-FONT-010);
//!    `SET` 选择合成重复次数(REA `FUN_004056b8` 的 1/9/3,`Likely`)。
//! 2. **自动文本层**(`TEXT[ID]` + `WORD`/`_`/`RETURNCODE`):`gfx` 已按
//!    `_TEXT_FLAG` 逐字 `'1'/'2'` 排好版,这里只负责把每个字符栅格化到位。
//!
//! # 证据
//!
//! - 单字覆盖 = GDI `CreateFontIndirectA`+`TextOutA` 8-bit mask
//!   (REA `FUN_00445421`/`FUN_004450c0`);本实现用 `yu-font` 的 swash 覆盖,
//!   不宣称与 GDI 字节级一致(DIV-FONT-002)。
//! - `DRAWMODE=0` 覆盖含透明像素,`1` 源合成(手册 `font/font.html`)。
//! - 基线 = 单元顶边 + 字体 ascent(近似 GDI 的 `lfHeight` 单元,DIV-GFX-056)。

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

use yu_font::cache::CacheLimits;
use yu_font::{
    cp932, AaMode, DrawMode, FontRenderer, FontSource, Gradation, Rgb, Rotation,
    ENGINE_DEFAULT_FAMILIES,
};
use yu_font::{StylePolicy, TextStyle as FontStyle};
use yu_gfx::{LayerId, TextDraw, TextDrawKey, TextFontRequest, TextLayoutFrame, TextureRef};

use crate::texture::Texture;

/// 一次文本合成的统计(进 `RenderReport`)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextPaintStats {
    /// 处理的绘制项(字符串)数。
    pub draws: usize,
    /// 成功栅格化并写入的字符数。
    pub glyphs: usize,
    /// 字体缺字/不可解码字符数。
    pub missing: usize,
    /// 去重后的失败原因(稳定排序)。
    pub diags: Vec<String>,
}

impl TextPaintStats {
    fn note(&mut self, what: &str) {
        if !self.diags.iter().any(|d| d == what) {
            self.diags.push(what.to_string());
            self.diags.sort();
        }
    }

    fn merge(&mut self, other: TextPaintStats) {
        self.draws += other.draws;
        self.glyphs += other.glyphs;
        self.missing += other.missing;
        for d in other.diags {
            self.note(&d);
        }
    }
}

/// 文本合成器:持有字体源/栅格器/字形缓存(懒加载系统字体)。
pub struct TextComposer {
    renderer: Option<FontRenderer>,
    /// 首次初始化失败原因(`None` = 尚未尝试)。
    failure: Option<String>,
    /// 字体身份 → face 解析缓存(键是请求的稳定显示形式)。
    faces: BTreeMap<String, Option<(yu_font::FaceId, FaceFallback)>>,
    /// 首次系统字体枚举耗时(毫秒)。只在加载后的第一帧报告一次
    /// (`text.font_init`),用于解释“第一次打开テキスト页签”的尖峰。
    load_ms: Option<u128>,
    /// 「基图 + 文本」合成结果缓存:`CGACT[TEXT]` 是跨帧持久的烘焙
    /// (DIV-GFX-058),内容没变就不该每帧重烤。键 = 层 + 文本修订号 +
    /// 基面身份;自动文本层(布局每帧变化)不进缓存。
    compose_cache: BTreeMap<LayerId, ComposeCacheEntry>,
    /// `face -> 是否存在真实粗体面`(weight>=600)。**缓存**:
    /// `FontSource::resolve_style` 内部走 fontdb 查询,每帧每文本项调用一次
    /// 会拖出秒级 render 尖峰(实测 1061ms),因此只在首次解析时求值。
    real_bold: BTreeMap<yu_font::FaceId, bool>,
    /// 「要求 BOLD 但本机无粗体面」待报告(发现时置位,发出后清除;
    /// 不逐帧重发,避免刷帧日志)。
    pending_bold_note: bool,
    /// 系统字体源的摘要(face 数 + 引擎默认 family 实际落到的 face/文件)。
    /// `ensure()` 成功时填一次;日志按 `YU_LOG`/`--log-level` 的 info 级输出。
    source_summary: Option<String>,
    /// 强制字体 family(诊断/换字体用):非 `None` 时**所有** `FONT` 请求
    /// 都解析到这个 family,绕过脚本给的 `NAME`/`NO`/`BNO`。
    ///
    /// 查不到时不用引擎回退表兜底 —— 强制指定就该显式失败,否则用户会
    /// 以为换上了、其实画的还是默认字体。
    force_font: Option<String>,
    /// 合成缓存当前字节占用(`ComposeCacheEntry::bytes` 之和)。
    cache_bytes: u64,
    /// LRU 时钟(每次合成触碰/插入时递增)。
    cache_clock: u64,
    /// 帧内合成统计(`YU_RENDER_TIMING` 诊断;`begin_frame_stats` 清零)。
    frame_stats: ComposeFrameStats,
}

/// 一帧内合成缓存路径的计数与分段耗时(诊断用)。
#[derive(Clone, Copy, Debug, Default)]
pub struct ComposeFrameStats {
    pub hits: u32,
    pub incremental: u32,
    pub full_bakes: u32,
    pub bake_ms: f64,
    pub hash_ms: f64,
    pub paint_ms: f64,
    pub base_ms: f64,
    /// 本帧最大一次 bake 的合成纹理尺寸(诊断)。
    pub max_dims: (u32, u32),
}

/// 一条合成缓存(见 [`TextComposer::compose_cache`])。
struct ComposeCacheEntry {
    revision: u64,
    base_key: u64,
    texture: Rc<Texture>,
    /// 已烘焙进 `texture` 的绘制键(顺序 = 烘焙顺序)。
    ///
    /// 官方语义是 `CGACT[TEXT]` 命令时刻直接写层表面的持久烘焙
    /// (DIV-GFX-058):已画上去的像素不会因后续命令改变。因此新绘制集合
    /// 若以它为**严格前缀追加**,就只增量画尾部 —— 与整体重烤逐位等价,
    /// 成本从 O(整页) 降到 O(新字符);这是“逐字吐字越到后面越卡”的主修。
    baked: Vec<TextDrawKey>,
    /// `texture` 内容指纹(烘焙/增量时算一次;命中路径零重算)。
    /// 算法 = [`content_hash_rgba`](与 wgpu `hash_rgba` 同一口径)。
    hash: u64,
    /// LRU 时钟(`compose_cache_clock` 递增;逐条淘汰的次序依据)。
    last_use: u64,
    /// `texture.rgba` 字节数(预算记账)。
    bytes: u64,
}

/// 合成缓存的**字节预算**:超过时按 LRU 逐条淘汰最旧条目。
///
/// 实测语料(角色加载演示)一屏可建 480-779 个文本精灵层;旧实现
/// 「256 条超限整体清空」会让缓存彻底失效,每帧全量重烤 + 整图 FNV
/// (单帧 200-700ms 反复出现)。按字节预算逐条淘汰既不无界占内存,
/// 也不再把整屏图层全拍死。
const COMPOSE_CACHE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
/// 条目数硬上限(元数据防无界;每条还有 `bytes` 记账)。
const COMPOSE_CACHE_MAX_ENTRIES: usize = 8192;

/// 「宽 + 高 + RGBA 字节」的 FNV-1a 内容指纹。
///
/// wgpu 后端的 `hash_rgba` 委托到这里:单一实现,保证烘焙期算出的指纹与
/// `Override` 跨帧比对、GPU 上传缓存键的口径完全一致。
#[must_use]
pub fn content_hash_rgba(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for byte in width.to_le_bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    for byte in height.to_le_bytes() {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    for &byte in rgba {
        h ^= u64::from(byte);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 纹理内容哈希(FNV-1a;仅用于帧内图像加工覆盖纹理的缓存键)。
pub fn texture_content_hash(texture: &Texture) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &b in &texture.rgba {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 文本合成基面的身份哈希(资产 key / 1×1 黑 / 快照 id / 无纹理尺寸 +
/// 帧内图像加工覆盖哈希)。与 `TextDraw` 的层修订号一起做缓存键。
pub fn text_base_key(texture: &TextureRef, override_hash: Option<u64>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64; // FNV-1a
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    match texture {
        TextureRef::Asset(key) => {
            feed(b"A");
            feed(key);
        }
        TextureRef::SolidBlack1x1 => feed(b"B"),
        TextureRef::Snapshot(id) => {
            feed(b"S");
            feed(&id.0.to_le_bytes());
        }
        TextureRef::Offscreen(id) => {
            feed(b"O");
            feed(id.as_bytes());
        }
        TextureRef::None => feed(b"E"),
    }
    if let Some(hash) = override_hash {
        feed(b"X");
        feed(&hash.to_le_bytes());
    }
    h
}

/// 字体身份解析的回落种类(`font_style` 第三项;进 `text_diags`)。
#[derive(Clone, Debug, PartialEq, Eq)]
enum FaceFallback {
    /// 正常解析。
    None,
    /// `BNO` 调试槽未取证,按默认字体绘制(DIV-FONT-007)。
    Bno,
    /// 字体名在本机不存在(或为空):按引擎回退表默认 family(DIV-GFX-059)。
    Name(Vec<u8>),
}

/// 枚举结果摘要:face 总数、耗时、引擎默认 family 实际命中的是哪一项以及
/// 它落在哪个字体文件。
///
/// 只在 `ensure()` 成功那一次生成。返回值同时进 `info` 日志和
/// [`TextComposer::source_summary`](供宿主在默认 `warn` 级别下无条件打印)。
fn font_source_summary(source: &FontSource, elapsed_ms: u128) -> String {
    let head = format!(
        "[font] system fonts: {} faces enumerated in {elapsed_ms}ms",
        source.len()
    );
    match source.default_face() {
        Ok(face) => format!(
            "{head}; engine default family \"{}\" -> {}",
            source.default_family_hit().unwrap_or("<unresolved>"),
            source.describe_face(face)
        ),
        Err(_) => format!(
            "{head}; none of the engine default families is installed: {}",
            ENGINE_DEFAULT_FAMILIES.join(", ")
        ),
    }
}

/// 把 `FONT` 请求渲染成人读形式(日志用):`NAME="ＭＳ ゴシック"` /
/// `NO=3` / `BNO=1` / `Default`(脚本未指定,走引擎回退表)。
fn describe_request(request: &TextFontRequest) -> String {
    match request {
        TextFontRequest::Name(bytes) => {
            let text: String = cp932::decode(bytes).chars.into_iter().collect();
            format!("NAME=\"{text}\"")
        }
        TextFontRequest::No(no) => format!("NO={no}"),
        TextFontRequest::Bno(no) => format!("BNO={no}"),
        TextFontRequest::Default => "Default(script gave no font)".to_string(),
    }
}

impl Default for TextComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextComposer {
    /// 懒加载:首次绘制时才枚举系统字体(标题/无文本帧零成本)。
    #[must_use]
    pub fn new() -> Self {
        Self {
            renderer: None,
            failure: None,
            faces: BTreeMap::new(),
            load_ms: None,
            compose_cache: BTreeMap::new(),
            real_bold: BTreeMap::new(),
            pending_bold_note: false,
            source_summary: None,
            force_font: None,
            cache_bytes: 0,
            cache_clock: 0,
            frame_stats: ComposeFrameStats::default(),
        }
    }

    /// 用调用方提供的字体源构造(测试/自带字体)。
    #[must_use]
    pub fn with_source(source: FontSource) -> Self {
        Self {
            renderer: Some(FontRenderer::new(source, CacheLimits::default())),
            failure: None,
            faces: BTreeMap::new(),
            load_ms: None,
            compose_cache: BTreeMap::new(),
            real_bold: BTreeMap::new(),
            pending_bold_note: false,
            source_summary: None,
            force_font: None,
            cache_bytes: 0,
            cache_clock: 0,
            frame_stats: ComposeFrameStats::default(),
        }
    }

    /// 字体是否可用(会触发一次懒加载)。
    pub fn available(&mut self) -> bool {
        self.ensure()
    }

    /// 预枚举系统字体(桌面宿主启动时调用)。
    ///
    /// `fontdb::Database::load_system_fonts()` 在本机实测 ~130ms;不预热
    /// 的话,第一次 `CGACT[TEXT]`(設定→テキスト页签)会卡这一下。
    /// 只初始化字体源,不改变任何渲染语义。
    pub fn warm(&mut self) -> bool {
        let ok = self.ensure();
        // 预热耗时由 `Engine::new` 的 startup 日志记录;不要让它出现在
        // 第一帧的 `text_diags` 里(那会把启动成本误报成帧内成本)。
        let _ = self.load_ms.take();
        ok
    }

    /// 不可用原因(`font.missing` 等)。
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// 系统字体源摘要(face 数 + 引擎默认 family 实际落到的 face/文件)。
    ///
    /// 日志按 `YU_LOG` / `--log-level` 输出(字体诊断是 `info` 级)。
    #[must_use]
    pub fn source_summary(&self) -> Option<&str> {
        self.source_summary.as_deref()
    }

    /// 强制所有 `FONT` 请求解析到这个 family(诊断/换字体用)。
    ///
    /// 传 `None` 恢复脚本原语义。已有解析缓存会被清空 —— 否则同一 `FONT`
    /// 请求还命中旧 face,看上去像没生效。family 查不到时不用引擎回退表
    /// 兜底,解析失败并通过 `text.font_missing` 显式暴露。
    pub fn set_force_font(&mut self, family: Option<String>) {
        self.force_font = family;
        self.faces.clear();
    }

    /// 当前的强制字体 family。
    #[must_use]
    pub fn force_font(&self) -> Option<&str> {
        self.force_font.as_deref()
    }

    /// 已安装字体族清单(CP932 字节,去重保序;与渲染器同源的 `fontdb`
    /// family 列表)。未初始化过字体源时为空 —— 调用方需要清单时先
    /// [`Self::warm`]。
    pub fn font_families(&mut self) -> Vec<Vec<u8>> {
        if !self.ensure() {
            return Vec::new();
        }
        let renderer = self.renderer.as_ref().expect("ensured above");
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for family in renderer.source().family_names() {
            // 名字编码成 CP932 原始字节(脚本比较/写回 GSD 用的形态);
            // 编不进 CP932 的 family(罕见非日文字形名)跳过 —— 引擎的
            // EnumFontFamiliesExA 也只回报 CP932 可表达的名字。
            let Ok(bytes) = yu_format::Codec::Cp932.encode(&family, b"fontlist") else {
                continue;
            };
            if seen.insert(bytes.clone()) {
                out.push(bytes);
            }
        }
        out
    }

    /// 引擎默认 family 的 CP932 字节(fontdb 实际命中的引擎内置候选;
    /// 未初始化/未命中 = `None`)。宿主把它作为字体清单的**唯一**条目
    /// 注入(`FONTINFO[NUM]`=1):逐字形回退(font linking)接入前,清单
    /// 里的其它 family 一旦被脚本选走且缺假名字形,日文会整体不渲染
    /// (GDI 有自动回退,我们还没有)——单选项 = 默认字体,设定页字体
    /// 列表可渲染、写入 GSD 的也是安全值。
    pub fn default_family_cp932(&mut self) -> Option<Vec<u8>> {
        if !self.ensure() {
            return None;
        }
        let renderer = self.renderer.as_ref().expect("ensured above");
        let name = renderer.source().default_family_hit()?;
        yu_format::Codec::Cp932
            .encode(name, b"fontlist")
            .ok()
            .filter(|b| !b.is_empty())
    }

    fn ensure(&mut self) -> bool {
        if self.renderer.is_some() {
            return true;
        }
        if let Some(why) = &self.failure {
            let _ = why;
            return false;
        }
        let started = Instant::now();
        match FontSource::load_system_fonts() {
            Ok(source) => {
                let summary = font_source_summary(&source, started.elapsed().as_millis());
                log::info!(target: "yu-font", "{}", summary);
                self.source_summary = Some(summary);
                self.renderer = Some(FontRenderer::new(source, CacheLimits::default()));
                self.load_ms = Some(started.elapsed().as_millis());
                true
            }
            Err(diag) => {
                log::error!(
                    target: "yu-font",
                    "[font] system font enumeration failed: {} {}",
                    diag.code,
                    diag.detail
                );
                self.failure = Some(format!("{}: {}", diag.code, diag.detail));
                false
            }
        }
    }

    /// 解析 `FONT` 字体身份请求(`NAME` → `NO` → `BNO` → 引擎默认回退表)。
    ///
    /// 设了 [`TextComposer::set_force_font`] 时整段被它短路:不管脚本要什么,
    /// 都解析到那个 family(诊断用);查不到就返回 `None` 显式失败。
    fn resolve_face(
        &mut self,
        request: &TextFontRequest,
    ) -> Option<(yu_font::FaceId, FaceFallback)> {
        let key = format!("{request:?}");
        if let Some(hit) = self.faces.get(&key) {
            return hit.clone();
        }
        if !self.ensure() {
            return None;
        }
        let renderer = self.renderer.as_ref()?;
        let source = renderer.source();
        let forced = self.force_font.clone();
        let face = match forced {
            Some(family) => {
                let hit = source.find_family(&family);
                if hit.is_none() {
                    log::warn!(
                        target: "yu-font",
                        "[font] forced family \"{family}\" not installed; \
                         every FONT request will fail (drop --font to restore script semantics)"
                    );
                } else if let Some(id) = hit {
                    // 脚本自己算居中(X = 框宽/2 - 字符数*SX/2),假定半角字宽
                    // 恒为 SX/2。比例字体不满足该假定 ⇒ 文本整体偏移。
                    // 这是 `--font` 的诊断代价,不是渲染缺陷,必须说清楚。
                    if !source.fullwidth_is_monospace(id) {
                        log::warn!(
                            target: "yu-font",
                            "[font] forced family \"{family}\" has PROPORTIONAL metrics; \
                             script-computed centering (X = box/2 - len*SX/2) assumes every \
                             glyph is exactly SX wide, so forced-font text will look off-centre"
                        );
                    }
                }
                hit.map(|face| (face, FaceFallback::None))
            }
            None => match request {
                TextFontRequest::Name(bytes) => {
                    let text: String = cp932::decode(bytes).chars.into_iter().collect();
                    match source.find_family(&text) {
                        Some(face) => Some((face, FaceFallback::None)),
                        // 名字查不到(空串/本机未装):引擎按默认 family 回退表
                        // 选第一个已安装 family(`0x403b8e..0x403bdc`),不是错误。
                        None => source
                            .default_face()
                            .ok()
                            .map(|face| (face, FaceFallback::Name(bytes.clone()))),
                    }
                }
                TextFontRequest::No(no) => {
                    if *no >= 0 {
                        source
                            .face_by_no(*no as usize)
                            .map(|face| (face, FaceFallback::None))
                    } else {
                        None
                    }
                }
                // BNO 的字体列表语义未取证(DIV-FONT-007):不解析,退回默认。
                TextFontRequest::Bno(_) => source
                    .default_face()
                    .ok()
                    .map(|face| (face, FaceFallback::Bno)),
                TextFontRequest::Default => source
                    .default_face()
                    .ok()
                    .map(|face| (face, FaceFallback::None)),
            },
        };
        self.faces.insert(key.clone(), face.clone());
        // 每个不同的 FONT 请求只在首次解析时记一条:脚本重发同一字体不刷日志。
        // 这是「游戏实际用了什么字体」的直接答案(请求 → 文件 → TTC 面)。
        let want = describe_request(request);
        match &face {
            Some((id, fallback)) => {
                let how = match fallback {
                    FaceFallback::None => "exact",
                    FaceFallback::Bno => "bno-fallback",
                    FaceFallback::Name(_) => "name-fallback",
                };
                let line = format!(
                    "[font] {want} -> {how} {}",
                    source.describe_face(*id)
                );
                if matches!(fallback, FaceFallback::None) {
                    log::info!(target: "yu-font", "{line}");
                } else {
                    log::warn!(target: "yu-font", "{line}");
                }
            }
            None => log::warn!(target: "yu-font", "[font] {want} -> unresolvable"),
        }
        face
    }

    /// gfx 字体快照 → `yu-font` 样式(带 face 解析)。
    ///
    /// 返回值第三项 = 是否走了 `BNO` 的“默认字体”回退(DIV-FONT-007
    /// 未取证);调用方据此发一次性报告,不静默回退。
    fn font_style(&mut self, style: &yu_gfx::TextStyle) -> Option<(FontStyle, u16, FaceFallback)> {
        let request = style.font_request();
        let (face, fallback) = self.resolve_face(&request)?;
        // SX/SY=0 → 20:引擎默认样式(`FUN_00464fac`;见
        // `docs/font-contract.md` §3.3“引擎默认 SX=SY=20”)。
        let sx = style.size_x.clamp(0, 0x100) as u16;
        let sy = style.size_y.clamp(0, 0x100) as u16;
        let sx = if sx == 0 { 20 } else { sx };
        let sy = if sy == 0 { 20 } else { sy };
        let mut s = FontStyle::engine_default(face);
        s.sx = sx;
        s.sy = sy;
        s.bold = style.bold;
        s.aa = if style.antialias {
            AaMode::Grayscale
        } else {
            AaMode::None
        };
        s.color = Rgb::from_rgb_u32(style.color & 0x00FF_FFFF);
        // COLOR2 缺省回退 COLOR:引擎样式字段默认 `0xffffff`
        // (`docs/font-provenance.md`:COLOR2 槽 `+0x2c`,初值 `0x5d1708`,
        // 默认 `0xffffff`);脚本未给 COLOR2 时按单色绘制。该“未给”分支
        // 本身无单独手册例,保留回退并在审计表挂账。
        s.color2 = Rgb::from_rgb_u32(style.color2.unwrap_or(style.color) & 0x00FF_FFFF);
        s.gradation =
            Gradation::from_script(style.gradation).unwrap_or(Gradation::Direction { octant: 1 });
        s.rotation = Rotation::from_script(style.rotation).unwrap_or(Rotation::None);
        s.draw_mode = if style.draw_mode != 0 {
            DrawMode::Blend
        } else {
            DrawMode::Replace
        };
        // BOLD:只接受真实粗体面。本机 family 无 weight>=600 的 face 时
        // **不做合成加粗**(实测合成膨胀让墨迹 +70%,与官方截图不符),
        // 按原 face 绘制并显式报告 `text.font.bold_missing`。
        if s.bold {
            let has_real = match self.real_bold.get(&face) {
                Some(&v) => v,
                None => {
                    // 纯内存表扫描(不触发 fontdb 加载 ⇒ 无单帧尖峰)。
                    let v = self
                        .renderer
                        .as_ref()
                        .expect("ensured above")
                        .source()
                        .family_has_bold(face);
                    self.real_bold.insert(face, v);
                    if !v {
                        self.pending_bold_note = true;
                    }
                    v
                }
            };
            if !has_real {
                s.bold = false;
            }
        }
        Some((s, sy, fallback))
    }

    /// 在 `dst`(RGBA8,`w×h`)上绘制一个 `CGACT[TEXT]` 项。
    fn paint_one(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        draw: &TextDraw,
        stats: &mut TextPaintStats,
    ) {
        if !self.ensure() {
            if let Some(why) = self.failure.clone() {
                stats.note(&format!("text.font_unavailable: {why}"));
            }
            stats.draws += 1;
            return;
        }
        if let Some(ms) = self.load_ms.take() {
            stats.note(&format!("text.font_init: {ms}ms"));
        }
        let Some((font_style, sy, fallback)) = self.font_style(&draw.style) else {
            stats.draws += 1;
            stats.note("text.font_missing");
            return;
        };
        if self.pending_bold_note {
            self.pending_bold_note = false;
            stats.note(
                "text.font.bold_missing: 本机 family 无粗体面(weight>=600);按原 face 绘制,不做合成加粗",
            );
        }
        match &fallback {
            FaceFallback::None => {}
            FaceFallback::Bno => stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            ),
            FaceFallback::Name(bytes) => stats.note(&format!(
                "text.font.name_fallback: {}",
                String::from_utf8_lossy(bytes)
            )),
        }
        let ascent = {
            let renderer = self.renderer.as_ref().expect("ensured above");
            match renderer.source().ascent_px(font_style.face, sy) {
                Ok(a) => a.round() as i32,
                Err(_) => {
                    stats.draws += 1;
                    stats.note("text.metrics_unavailable");
                    return;
                }
            }
        };
        let blended = font_style.draw_mode == DrawMode::Blend;
        // 引擎按 **偏移** 多次合成(3×3 邻域),不是同位置重复画
        // (REA `FUN_004056b8`;见 `TextDraw::composite_offsets`)。
        // 同位置叠 9 次会把 AA 边缘压实成实心 ⇒ 字变粗变白。
        let (offsets, passes) = draw.composite_offsets();
        let decoded = cp932::decode(&draw.bytes);
        if decoded.invalid > 0 {
            stats.missing += decoded.invalid;
            stats.note("text.decode_invalid");
        }
        let mut pen_x = draw.x as i32;
        let baseline = draw.y as i32 + ascent;
        let renderer = self.renderer.as_mut().expect("ensured above");
        let mut drew_any = false;
        let mut dbg_glyph_ns: u128 = 0;
        let mut dbg_draw_ns: u128 = 0;
        for ch in &decoded.chars {
            let tg = std::time::Instant::now();
            let rendered = renderer.render_glyph(*ch, &font_style, StylePolicy::Reference);
            dbg_glyph_ns += tg.elapsed().as_nanos();
            match rendered {
                Ok(image) => {
                    let gx = pen_x + image.left;
                    let gy = baseline + image.top;
                    let td = std::time::Instant::now();
                    for &(dx, dy) in &offsets[..passes] {
                        image.draw_onto(dst, w, h, gx + dx, gy + dy, blended);
                    }
                    dbg_draw_ns += td.elapsed().as_nanos();
                    let step = ((image.advance as f32) / 256.0).round() as i32;
                    pen_x += step.max(0);
                    stats.glyphs += 1;
                    drew_any = true;
                }
                Err(diag) => {
                    stats.missing += 1;
                    stats.note(&format!("text.glyph_missing: {}", diag.code));
                }
            }
        }
        let _ = drew_any;
        if std::env::var("YU_P8_TIMING").is_ok() && dbg_glyph_ns + dbg_draw_ns > 500_000 {
            log::trace!(
                target: "yu-render",
                "[p8] draw chars={} glyph={}us onto={}us passes={}",
                decoded.chars.len(),
                dbg_glyph_ns / 1000,
                dbg_draw_ns / 1000,
                passes
            );
        }
        stats.draws += 1;
    }

    /// 按 `CGACT[TEXT]` 绘制项在 RGBA8 缓冲上合成(原地)。
    pub fn paint_draws(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        draws: &[TextDraw],
    ) -> TextPaintStats {
        let refs: Vec<&TextDraw> = draws.iter().collect();
        self.paint_draw_refs(dst, w, h, &refs)
    }

    /// 同 [`Self::paint_draws`],但借用绘制项(增量烘焙免深拷贝)。
    pub fn paint_draw_refs(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        draws: &[&TextDraw],
    ) -> TextPaintStats {
        let mut stats = TextPaintStats::default();
        if w == 0 || h == 0 || dst.len() < (w as usize) * (h as usize) * 4 {
            return stats;
        }
        for draw in draws {
            self.paint_one(dst, w, h, draw, &mut stats);
        }
        stats
    }

    /// 按 `gfx` 已排版的 `_TEXT_FLAG` 单元在 RGBA8 缓冲上合成(原地)。
    pub fn paint_layout(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        layout: &TextLayoutFrame,
    ) -> TextPaintStats {
        self.paint_layout_with(dst, w, h, layout, None)
    }

    /// 同 [`TextComposer::paint_layout`];`blend_override` 强制覆盖/合成。
    ///
    /// 屏幕叠加(没有文本图层承载基图)必须用**合成**,否则 `DRAWMODE=0`
    /// 会把字形框内的屏幕像素写成透明(`DIV-GFX-057`)。图层内合成时用
    /// `FONT[DRAWMODE]`。
    pub fn paint_layout_with(
        &mut self,
        dst: &mut [u8],
        w: u32,
        h: u32,
        layout: &TextLayoutFrame,
        blend_override: Option<bool>,
    ) -> TextPaintStats {
        let mut stats = TextPaintStats::default();
        if w == 0 || h == 0 || dst.len() < (w as usize) * (h as usize) * 4 {
            return stats;
        }
        if !self.ensure() {
            if let Some(why) = self.failure.clone() {
                stats.note(&format!("text.font_unavailable: {why}"));
            }
            return stats;
        }
        if let Some(ms) = self.load_ms.take() {
            stats.note(&format!("text.font_init: {ms}ms"));
        }
        let style = &layout.style;
        let Some((font_style, sy, fallback)) = self.font_style(style) else {
            stats.note("text.font_missing");
            return stats;
        };
        match &fallback {
            FaceFallback::None => {}
            FaceFallback::Bno => stats.note(
                "text.font.bno_fallback: BNO 字体列表语义未取证(DIV-FONT-007),按默认字体绘制",
            ),
            FaceFallback::Name(bytes) => stats.note(&format!(
                "text.font.name_fallback: {}",
                String::from_utf8_lossy(bytes)
            )),
        }
        let ascent = {
            let renderer = self.renderer.as_ref().expect("ensured above");
            match renderer.source().ascent_px(font_style.face, sy) {
                Ok(a) => a.round() as i32,
                Err(_) => {
                    stats.note("text.metrics_unavailable");
                    return stats;
                }
            }
        };
        // 自动文本层:1 次合成(引擎未取证重复;`_TEXT_FLAG` 只控排版)。
        let blended = blend_override.unwrap_or(font_style.draw_mode == DrawMode::Blend);
        let renderer = self.renderer.as_mut().expect("ensured above");
        let visible = layout.reveal.unwrap_or(usize::MAX);
        for glyph in layout.glyphs.iter().take(visible) {
            let Some(ch) = cp932::decode_char(&glyph.bytes) else {
                stats.missing += 1;
                stats.note("text.decode_invalid");
                continue;
            };
            match renderer.render_glyph(ch, &font_style, StylePolicy::Reference) {
                Ok(image) => {
                    let gx = glyph.x as i32 + image.left;
                    let gy = glyph.y as i32 + ascent + image.top;
                    image.draw_onto(dst, w, h, gx, gy, blended);
                    stats.glyphs += 1;
                }
                Err(diag) => {
                    stats.missing += 1;
                    stats.note(&format!("text.glyph_missing: {}", diag.code));
                }
            }
        }
        stats.draws = layout.chars;
        stats
    }

    /// 为图层合成"基图 + 文本"的副本。
    ///
    /// 返回 `None` 表示没有可绘制的文本;`Some` 时像素已包含文本。
    /// 图层局部坐标与基图一致(超出基图的部分被裁剪 —— 引擎写入图层像素,
    /// 同样受图层尺寸限制)。
    pub fn compose_layer(
        &mut self,
        base: &Texture,
        draws: &[TextDraw],
        layout: Option<&TextLayoutFrame>,
    ) -> (Option<Texture>, TextPaintStats) {
        let refs: Vec<&TextDraw> = draws.iter().collect();
        self.compose_layer_refs(base, &refs, layout)
    }

    /// 同 [`Self::compose_layer`],但借用绘制项(调用方可免去深拷贝)。
    pub fn compose_layer_refs(
        &mut self,
        base: &Texture,
        draws: &[&TextDraw],
        layout: Option<&TextLayoutFrame>,
    ) -> (Option<Texture>, TextPaintStats) {
        if draws.is_empty() && layout.is_none() {
            return (None, TextPaintStats::default());
        }
        let mut out = base.clone();
        let mut stats = self.paint_draw_refs(&mut out.rgba, out.width, out.height, draws);
        if let Some(layout) = layout {
            stats.merge(self.paint_layout(&mut out.rgba, out.width, out.height, layout));
        }
        (Some(out), stats)
    }

    /// 同 [`Self::compose_layer_cached`],但按「层 + 文本修订号 + 基面身份」
    /// 缓存结果并支持**增量烘焙**(P8:持久文本是烘焙语义,内容不变不重合成;
    /// 新字符只画增量,与官方“命令时刻写层表面”一致)。
    ///
    /// - `base_dims`:基图尺寸(调用方**免克隆**地窥视;命中/增量路径不碰基图)。
    /// - `resolve_base`:整体重烤/自动文本层时才调用(整图克隆只发生在真正
    ///   需要时);`Err` 原样上抛给调用方记诊断。
    /// - 返回的 [`ComposedLayer::hash`] 供 wgpu 侧直接构造 `Override`,
    ///   命中路径不再逐帧重扫整图(800×600 ≈ 1.9 MB FNV)。
    ///
    /// 只在 `layout.is_none()`(纯 `CGACT[TEXT]`)时缓存:自动文本层的布局
    /// 每帧变化,缓存会读到旧内容。统计只计实际画上的字符。
    pub fn compose_layer_cached(
        &mut self,
        layer: &LayerId,
        revision: u64,
        base_key: u64,
        base_dims: (u32, u32),
        draws: &[&TextDraw],
        layout: Option<&TextLayoutFrame>,
        resolve_base: impl FnOnce() -> Result<Rc<Texture>, String>,
    ) -> Result<(Option<ComposedLayer>, TextPaintStats), String> {
        if let Some(layout) = layout {
            let base = resolve_base()?;
            let (out, stats) = self.compose_layer_refs(&base, &[], Some(layout));
            return Ok((
                out.map(|texture| {
                    let hash = content_hash_rgba(texture.width, texture.height, &texture.rgba);
                    ComposedLayer {
                        texture: Rc::new(texture),
                        hash,
                    }
                }),
                stats,
            ));
        }
        if draws.is_empty() {
            return Ok((None, TextPaintStats::default()));
        }
        // 命中:同修订 + 同基面身份 + 同尺寸 ⇒ 零重烤、零哈希、零克隆。
        if let Some(hit) = self.compose_cache.get_mut(layer) {
            if hit.revision == revision
                && hit.base_key == base_key
                && hit.texture.width == base_dims.0
                && hit.texture.height == base_dims.1
            {
                self.cache_clock += 1;
                hit.last_use = self.cache_clock;
                self.frame_stats.hits += 1;
                let composed = ComposedLayer {
                    texture: hit.texture.clone(),
                    hash: hit.hash,
                };
                return Ok((Some(composed), TextPaintStats::default()));
            }
        }
        // 增量:基面身份/尺寸未变,且新集合严格延伸已烘焙前缀 ⇒ 只画尾部。
        // 语义与整体重烤逐位等价:烘焙顺序保持绘制顺序,已画像素不再变。
        let cached = self.compose_cache.remove(layer);
        if let Some(hit) = cached {
            let extends = hit.base_key == base_key
                && hit.texture.width == base_dims.0
                && hit.texture.height == base_dims.1
                && draws.len() > hit.baked.len()
                && draws[..hit.baked.len()]
                    .iter()
                    .zip(&hit.baked)
                    .all(|(draw, key)| key.matches_draw(draw));
            if extends {
                self.frame_stats.incremental += 1;
                let t0 = Instant::now();
                let mut texture = match Rc::try_unwrap(hit.texture) {
                    Ok(texture) => texture,
                    Err(rc) => (*rc).clone(),
                };
                let stats = self.paint_draw_refs(
                    &mut texture.rgba,
                    texture.width,
                    texture.height,
                    &draws[hit.baked.len()..],
                );
                let paint_t = t0.elapsed();
                let h0 = Instant::now();
                let hash = content_hash_rgba(texture.width, texture.height, &texture.rgba);
                let hash_t = h0.elapsed();
                self.frame_stats.paint_ms += paint_t.as_secs_f64() * 1e3;
                self.frame_stats.hash_ms += hash_t.as_secs_f64() * 1e3;
                let baked = draws.iter().map(|d| d.dedup_key()).collect();
                let bytes = texture.rgba.len() as u64;
                let texture = Rc::new(texture);
                self.cache_bytes = self.cache_bytes + bytes - hit.bytes;
                self.cache_clock += 1;
                self.compose_cache.insert(
                    layer.clone(),
                    ComposeCacheEntry {
                        revision,
                        base_key,
                        texture: texture.clone(),
                        baked,
                        hash,
                        last_use: self.cache_clock,
                        bytes,
                    },
                );
                return Ok((Some(ComposedLayer { texture, hash }), stats));
            }
            // 前缀不成立(清页/中段改动/换基面):丢弃旧条目,走整体重烤。
            self.cache_bytes = self.cache_bytes.saturating_sub(hit.bytes);
        }
        self.frame_stats.full_bakes += 1;
        let b0 = Instant::now();
        let r0 = Instant::now();
        let base = resolve_base()?;
        self.frame_stats.base_ms += r0.elapsed().as_secs_f64() * 1e3;
        let (out, stats) = self.compose_layer_refs(&base, draws, None);
        let Some(texture) = out else {
            return Ok((None, stats));
        };
        let h0 = Instant::now();
        let hash = content_hash_rgba(texture.width, texture.height, &texture.rgba);
        let hash_t = h0.elapsed();
        let bake_t = b0.elapsed();
        self.frame_stats.bake_ms += bake_t.as_secs_f64() * 1e3;
        self.frame_stats.hash_ms += hash_t.as_secs_f64() * 1e3;
        if (texture.width, texture.height) > self.frame_stats.max_dims {
            self.frame_stats.max_dims = (texture.width, texture.height);
        }
        let bytes = texture.rgba.len() as u64;
        let texture = Rc::new(texture);
        self.cache_bytes += bytes;
        self.cache_clock += 1;
        self.compose_cache.insert(
            layer.clone(),
            ComposeCacheEntry {
                revision,
                base_key,
                texture: texture.clone(),
                baked: draws.iter().map(|d| d.dedup_key()).collect(),
                hash,
                last_use: self.cache_clock,
                bytes,
            },
        );
        self.evict_compose_cache();
        Ok((Some(ComposedLayer { texture, hash }), stats))
    }

    /// 合成缓存超预算时的 LRU 淘汰(逐条,不再整体清空)。
    fn evict_compose_cache(&mut self) {
        while self.compose_cache.len() > COMPOSE_CACHE_MAX_ENTRIES
            || self.cache_bytes > COMPOSE_CACHE_BUDGET_BYTES
        {
            let Some(victim) = self
                .compose_cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_use)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            if let Some(entry) = self.compose_cache.remove(&victim) {
                self.cache_bytes = self.cache_bytes.saturating_sub(entry.bytes);
            }
        }
    }

    /// 清零帧内合成统计(诊断;`YU_RENDER_TIMING` 下每帧调用)。
    pub fn begin_frame_stats(&mut self) {
        self.frame_stats = ComposeFrameStats::default();
    }

    /// 取走帧内合成统计(诊断)。
    #[must_use]
    pub fn take_frame_stats(&mut self) -> ComposeFrameStats {
        std::mem::replace(&mut self.frame_stats, ComposeFrameStats::default())
    }

    /// 丢弃一个层的合成分缓存(层删除后调用;保守清理由上限保护兜底)。
    pub fn forget_compose_cache(&mut self, layer: &LayerId) {
        if let Some(entry) = self.compose_cache.remove(layer) {
            self.cache_bytes = self.cache_bytes.saturating_sub(entry.bytes);
        }
    }
}

/// [`TextComposer::compose_layer_cached`] 的输出。
#[derive(Clone)]
pub struct ComposedLayer {
    /// 合成结果(基图 + 已烘焙文本)。
    pub texture: Rc<Texture>,
    /// 内容指纹(与 wgpu `hash_rgba` 同一算法;命中路径零重算)。
    pub hash: u64,
}

