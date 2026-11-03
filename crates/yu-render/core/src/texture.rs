//! 纹理解析:资产 key → `yu-vfs` 字节 → `image` 解码 → RGBA8。
//!
//! 扩展名探测顺序来自 [`CompositorFrame::image_probe_order`]
//! (`yu_gfx::CompositorFrame::image_probe_order`),不做任何内置假设。
//! 探测候选按顺序检查 `DataSource::exists`;**第一个存在的候选胜出**;
//! 该候选解码失败即视为该资产失败(与引擎“找到文件后加载”一致),不再
//! 回退到后续扩展名。若 key 自带扩展名,先尝试原 key。
//!
//! 失败一律产生 `asset.*` 诊断:
//!
//! | code | 含义 |
//! |---|---|
//! | `asset.missing` | 所有探测候选都不存在 |
//! | `asset.decode` | 找到字节但 `image` 解码失败 |
//! | `asset.unsupported` | 引擎格式码有定义但无解码器(`.psb`/未知码) |

use std::collections::BTreeMap;

use yu_gfx::{ImageFormat, TextureRef};
use yu_log::{Boundary, Diag, Severity};
use yu_vfs::{Codec, DataSource, EntryKind, LookupKey};

/// `asset.*` 诊断码。
pub mod code {
    /// 所有探测候选都不存在。
    pub const MISSING: &str = "asset.missing";
    /// 找到文件但解码失败。
    pub const DECODE: &str = "asset.decode";
    /// 格式已识别但没有解码器(PSB/未知码)。
    pub const UNSUPPORTED: &str = "asset.unsupported";
    /// `YMV` 影片容器/帧不合法(魔数、表越界、帧号越界)。
    pub const MOVIE: &str = "asset.movie";
}

/// 一张已解码的 RGBA8 纹理(行优先、无预乘)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` 字节。
    pub rgba: Vec<u8>,
}

impl Texture {
    /// 手册 `cg/cg.html`:`FILE=""` 生成的 1×1 黑图(不透明)。
    pub fn solid_black_1x1() -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        Some([
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ])
    }

    fn from_rgba8(img: image::RgbaImage) -> Self {
        Self {
            width: img.width(),
            height: img.height(),
            rgba: img.into_raw(),
        }
    }
}

/// 一次资产失败(报告/摘要用;与 `yu_log::Diag` 内容一致)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetIssue {
    /// 原始 `TextureRef::Asset` key(CP932 字节,未做扩展名探测)。
    pub key: Vec<u8>,
    /// `asset.*` 码。
    pub code: String,
    /// 人类可读细节(含尝试过的候选项)。
    pub detail: String,
}

impl AssetIssue {
    /// 稳定显示形式:可打印 ASCII 原样,其余字节 `\xNN`。
    pub fn key_display(&self) -> String {
        display_bytes(&self.key)
    }
}

/// 失败缓存条数上限(失败条目字节 0,不受像素预算约束)。
const MAX_FAILED_CACHE_ENTRIES: usize = 4096;

/// 缓存条目。
struct Cached {
    /// 实际命中的探测路径(缺失时 `None`)。
    resolved: Option<Vec<u8>>,
    /// 已解码纹理。存 `Rc`:CPU 图像加工的源可以零拷贝共享(`resolve_shared`)。
    result: Result<std::rc::Rc<Texture>, Diag>,
}

/// 纹理解析 + 缓存。
///
/// 缓存键是原始资产 key,因此同一 key 的重复引用只解码一次。失败的条目也
/// 缓存,避免每帧重复探测;`begin_frame()` 清空“本帧问题清单”,但保留缓存。
pub struct TextureStore {
    source: Box<dyn DataSource>,
    codec: Codec,
    probe_order: Vec<ImageFormat>,
    cache: BTreeMap<Vec<u8>, Cached>,
    issues: BTreeMap<(Vec<u8>, String), AssetIssue>,
    solid: Texture,
    decoded: u64,
    /// 本帧解码耗时/字节(VFS 读 + 图像解码;`begin_frame` 清零)。
    ///
    /// 首次显示角色/立绘时的卡顿主要在这里(磁盘 + PNG 解码),它发生在
    /// `plan` 里,和 MERGE 合成混在一起看不出来 ⇒ 单独记账。
    decode_us: u64,
    decode_bytes: u64,
    /// 缓存像素预算(字节;0 = 不限制)。只回收**无外部引用**的条目
    /// (图层/快照仍在用的纹理不会被踢掉)。
    budget_bytes: usize,
    /// 当前缓存内像素字节。
    resident_bytes: usize,
    /// 访问/插入时钟(LRU)。
    clock: u64,
    /// `key → (字节, 最近访问序)`(预算记账;与 `cache` 同步增删)。
    meta: BTreeMap<Vec<u8>, (usize, u64)>,
    /// `key → 最近一次内容写入的序号`(与 LRU 的 `clock` 分开)。
    ///
    /// `insert_cached` 每次覆盖条目都会推进;`touch` 只动 LRU,不改这里。
    /// 后端用它判断「同一 key 的 CPU 侧像素是否被替换」——不能用
    /// `Rc::as_ptr`(旧 `Rc` 释放后新分配可能复用同一地址)或 `meta` 的
    /// LRU 时钟(`touch` 会推进)。
    inserted: BTreeMap<Vec<u8>, u64>,
    /// 本帧新插入的 key(预算执行时豁免;防止预热批次在渲染前被回收)。
    fresh: std::collections::BTreeSet<Vec<u8>>,
    /// 已回收条目数(诊断)。
    evicted: u64,
    /// 超预算且无可回收条目时只告警一次(回落到预算内后复位)。
    budget_warned: bool,
    /// 编码不一致时的唯一同名回退(默认开;`YURIS_ASSET_NAME_FALLBACK=off`
    /// 可关)。只影响“原始字节 miss 且 Unicode 唯一匹配”的情形。
    name_fallback: bool,
    /// 走了名称编码回退的解析次数(诊断)。
    name_fallback_hits: u64,
    /// `YMV` 每个资产**最近解析的帧号**(只保留一帧,见 [`Self::movie_frame`])。
    movie_keeper: BTreeMap<Vec<u8>, u32>,
    /// `YMV` **常驻容器**缓存(键 = `YMV[FILE]` 原字节)。
    ///
    /// 官方在 `YMV[FILE]` 时一次性读入并解混淆全部平面,之后换帧只切段;
    /// 这里同样只读一次包。`Rc` 共享,克隆 O(1);缺失/坏容器也缓存,
    /// 避免每帧重读重解析。
    movies: BTreeMap<Vec<u8>, MovieSlot>,
}

/// 一个影片资产的常驻状态(见 [`TextureStore::movie_container`])。
#[derive(Clone)]
enum MovieSlot {
    /// 资产不存在(两种候选都没命中)。
    Missing,
    /// 已解析并解混淆的容器。
    Ready {
        /// 命中的资产路径(诊断用)。
        path: Vec<u8>,
        container: std::rc::Rc<yu_format::ymv::DecryptedYmv>,
    },
    /// 文件存在但不是合法 `YSMV`(诊断随槽位缓存,只报一次)。
    Broken {
        path: Vec<u8>,
        code: String,
        detail: String,
    },
}

impl TextureStore {
    /// 本帧资产解码耗时(微秒)与像素字节。
    #[must_use]
    pub fn decode_stats(&self) -> (u64, u64) {
        (self.decode_us, self.decode_bytes)
    }
    pub fn new(source: Box<dyn DataSource>, codec: Codec, probe_order: Vec<ImageFormat>) -> Self {
        // 默认 512MiB:足够覆盖常见立绘/背景工作集,同时避免长会话无限增长。
        // `YURIS_ASSET_CACHE_MB=0` 关闭预算(取证/对照用)。
        let mb = std::env::var("YURIS_ASSET_CACHE_MB")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(512);
        let name_fallback = std::env::var("YURIS_ASSET_NAME_FALLBACK")
            .map_or(true, |v| !matches!(v.trim(), "0" | "off" | "false"));
        Self {
            source,
            codec,
            probe_order,
            cache: BTreeMap::new(),
            issues: BTreeMap::new(),
            solid: Texture::solid_black_1x1(),
            decoded: 0,
            decode_us: 0,
            decode_bytes: 0,
            budget_bytes: mb.saturating_mul(1024 * 1024),
            resident_bytes: 0,
            clock: 0,
            meta: BTreeMap::new(),
            inserted: BTreeMap::new(),
            fresh: std::collections::BTreeSet::new(),
            evicted: 0,
            budget_warned: false,
            name_fallback,
            name_fallback_hits: 0,
            movie_keeper: BTreeMap::new(),
            movies: BTreeMap::new(),
        }
    }

    /// 开关名称编码回退(测试/宿主诊断)。
    pub fn set_name_fallback(&mut self, enabled: bool) {
        self.name_fallback = enabled;
    }

    /// 名称编码回退命中次数。
    #[must_use]
    pub fn name_fallback_hits(&self) -> u64 {
        self.name_fallback_hits
    }

    /// 覆盖缓存预算(字节;0 = 不限制)。测试/宿主诊断用。
    pub fn set_budget_bytes(&mut self, bytes: usize) {
        self.budget_bytes = bytes;
        self.enforce_budget(false);
    }

    /// 当前缓存像素字节。
    #[must_use]
    pub fn resident_bytes(&self) -> usize {
        self.resident_bytes
    }

    /// 已回收的无引用条目数。
    #[must_use]
    pub fn evicted_count(&self) -> u64 {
        self.evicted
    }

    fn insert_cached(&mut self, key: Vec<u8>, cached: Cached) {
        self.clock = self.clock.wrapping_add(1);
        self.inserted.insert(key.clone(), self.clock);
        let bytes = cached
            .result
            .as_ref()
            .map(|texture| texture.rgba.len())
            .unwrap_or(0);
        if self.cache.insert(key.clone(), cached).is_some() {
            if let Some((old_bytes, _)) = self.meta.remove(&key) {
                self.resident_bytes = self.resident_bytes.saturating_sub(old_bytes);
            }
        }
        self.meta.insert(key.clone(), (bytes, self.clock));
        self.resident_bytes = self.resident_bytes.saturating_add(bytes);
        self.fresh.insert(key);
        // 不在插入时回收:一次 `preload_parallel` 批量里,后面的 key 会把
        // 同批尚未被图层引用的前序资产当场踢掉(→渲染时重新解码)。
        // 预算在帧边界统一执行(`begin_frame` / `set_budget_bytes`)。
    }

    /// 失败条目只保留最近 `MAX_FAILED_CACHE_ENTRIES` 条,避免坏/缺失
    /// 资产 key 在长会话里无界累积(它们字节为 0,不受像素预算约束)。
    fn enforce_failed_cap(&mut self) {
        let mut failed: Vec<(Vec<u8>, u64)> = self
            .meta
            .iter()
            .filter(|(_, (bytes, _))| *bytes == 0)
            .map(|(key, (_, used))| (key.clone(), *used))
            .collect();
        if failed.len() <= MAX_FAILED_CACHE_ENTRIES {
            return;
        }
        failed.sort_by_key(|(_, used)| *used);
        let excess = failed.len() - MAX_FAILED_CACHE_ENTRIES;
        for (key, _) in failed.into_iter().take(excess) {
            self.cache.remove(&key);
            self.meta.remove(&key);
            self.inserted.remove(&key);
            self.fresh.remove(&key);
        }
    }

    /// 超预算时按 LRU 回收**只有缓存引用**的条目(`Rc::strong_count == 1`)。
    /// 仍在图层/快照里的纹理跳过;`skip_fresh` = 本帧新插入的条目也跳过
    /// (预热/本帧刚解析的资产必须活到渲染结束)。实在没有可回收的就保持
    /// 超预算并留痕。
    fn enforce_budget(&mut self, skip_fresh: bool) {
        // 失败条目字节为 0,永远不参与字节预算;单独限条数,避免缺失/坏
        // 资产 key 在长时间游玩里只增不减(每帧探测一次就留一条)。
        self.enforce_failed_cap();
        if self.budget_bytes == 0 {
            return;
        }
        if self.resident_bytes <= self.budget_bytes {
            self.budget_warned = false;
            return;
        }
        while self.resident_bytes > self.budget_bytes {
            let victim = self
                .meta
                .iter()
                .filter(|(_, (bytes, _))| *bytes > 0)
                .filter(|(key, _)| !(skip_fresh && self.fresh.contains(*key)))
                .filter(|(key, _)| {
                    self.cache
                        .get(key.as_slice())
                        .and_then(|entry| entry.result.as_ref().ok())
                        .is_some_and(|texture| std::rc::Rc::strong_count(texture) == 1)
                })
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone());
            let Some(key) = victim else {
                if !self.budget_warned {
                    self.budget_warned = true;
                    log::warn!(
                        target: "yu-render",
                        "[asset] cache over budget ({}B > {}B); live surfaces still reference the remainder",
                        self.resident_bytes,
                        self.budget_bytes
                    );
                }
                break;
            };
            self.cache.remove(&key);
            self.fresh.remove(&key);
            self.inserted.remove(&key);
            if let Some((bytes, _)) = self.meta.remove(&key) {
                self.resident_bytes = self.resident_bytes.saturating_sub(bytes);
                self.evicted += 1;
                if std::env::var_os("YURIS_ASSET_TIMING").is_some() {
                    log::debug!(
                        target: "yu-render",
                        "[asset] evicted cached asset {} ({}B)",
                        display_bytes(&key),
                        bytes
                    );
                }
            }
        }
    }

    /// 开始新一帧:清空本帧问题清单与解码计时(缓存保留)。
    ///
    /// 缓存预算在这里统一执行:上一帧新插入/被引用的纹理全部豁免,只清掉
    /// 之前帧里已无引用的旧资产;预热批次因此不会被自我驱逐。
    pub fn begin_frame(&mut self) {
        self.issues.clear();
        self.decode_us = 0;
        self.decode_bytes = 0;
        self.enforce_budget(true);
        self.fresh.clear();
    }

    /// 解析一个资产 key。成功返回缓存中的纹理;失败返回 `asset.*` [`Diag`]。
    pub fn resolve(&mut self, key: &[u8]) -> Result<&Texture, Diag> {
        self.ensure(key)?;
        self.touch(key);
        let entry = self.cache.get(key).expect("cache entry inserted above");
        match &entry.result {
            Ok(tex) => Ok(tex.as_ref()),
            Err(diag) => {
                let issue = AssetIssue {
                    key: key.to_vec(),
                    code: diag.code.to_string(),
                    detail: diag.detail.clone(),
                };
                self.issues
                    .insert((key.to_vec(), issue.code.clone()), issue);
                Err(diag.clone())
            }
        }
    }

    /// `key` 最近一次内容写入的版本序号。
    ///
    /// 由 [`Self::insert_cached`] 推进;`touch` 的 LRU 更新不改它。GPU 后端
    /// 用它判断同一个 key 的 CPU 侧像素是否被替换(影片帧每帧经
    /// `insert_rgba` 覆盖 `:movie/current`)。
    #[must_use]
    pub fn generation(&self, key: &[u8]) -> Option<u64> {
        self.inserted.get(key).copied()
    }

    /// 解析资产并共享缓存纹理(`Rc` 克隆,O(1))。
    ///
    /// GPU 后端的 CPU 图像加工(`MERGE` 的上层源)用它:旧实现每次解析都把
    /// 整张资产 `rgba.clone()` 到新 `Rc`,一条 2048² 的部件每帧被复制几十次。
    pub fn resolve_shared(&mut self, key: &[u8]) -> Result<std::rc::Rc<Texture>, Diag> {
        self.ensure(key)?;
        self.touch(key);
        let entry = self.cache.get(key).expect("cache entry inserted above");
        match &entry.result {
            Ok(tex) => Ok(std::rc::Rc::clone(tex)),
            Err(diag) => {
                let issue = AssetIssue {
                    key: key.to_vec(),
                    code: diag.code.to_string(),
                    detail: diag.detail.clone(),
                };
                self.issues
                    .insert((key.to_vec(), issue.code.clone()), issue);
                Err(diag.clone())
            }
        }
    }

    /// 确保 key 已有缓存条目(成功/失败都会缓存,避免重复探测)。
    fn ensure(&mut self, key: &[u8]) -> Result<(), Diag> {
        if !self.cache.contains_key(key) {
            let cached = self.load(key);
            self.insert_cached(key.to_vec(), cached);
        }
        Ok(())
    }

    /// 记录一次缓存访问(LRU 预算用)。
    fn touch(&mut self, key: &[u8]) {
        self.clock = self.clock.wrapping_add(1);
        if let Some((_, used)) = self.meta.get_mut(key) {
            *used = self.clock;
        }
    }

    /// 解析 [`TextureRef`]。`Offscreen/Snapshot/None` 不归本模块物化。
    pub fn resolve_ref(&mut self, texture: &TextureRef) -> Result<&Texture, Diag> {
        match texture {
            TextureRef::Asset(key) => self.resolve(key),
            // `YMV[FRAME]`:按 `(资产, 帧)` 物化(颜色 JPEG + alpha 灰度 JPEG)。
            TextureRef::Movie { asset, frame } => self.resolve_movie(asset, *frame),
            TextureRef::SolidBlack1x1 => Ok(&self.solid),
            TextureRef::Offscreen(id) => Err(unimplemented(
                code::UNSUPPORTED,
                id.as_bytes().to_vec(),
                "离屏纹理由渲染器的 offscreen 表面提供,不经资产管线",
            )),
            TextureRef::Snapshot(id) => Err(unimplemented(
                code::UNSUPPORTED,
                id.0.to_le_bytes().to_vec(),
                "快照纹理:COPY 时序未取证,后端不物化",
            )),
            TextureRef::None => Err(unimplemented(code::UNSUPPORTED, Vec::new(), "图层没有纹理")),
        }
    }

    /// 物化 `YMV` 影片的某一帧(颜色平面 + alpha 平面 → RGBA8)。
    ///
    /// 缓存键是 `(资产, 帧)` 的合成键(以 NUL 开头,不可能与真实资产路径
    /// 冲突),因此同帧的重绘只解码一次。每个资产**只保留最近一帧**(见
    /// [`Self::ensure_movie`]):影片一次只显示一帧,若把 306 帧全留在缓存
    /// 里,一部 400×300 影片会占到 ~147MB 并与普通资产抢 LRU 预算。
    ///
    /// 官方语义(0.495 `0x45934c`):帧号已在 gfx 侧钳位;颜色段解到图层
    /// 表面、`alpha 标志 == 1` 时灰度段作 alpha(`0x4593f4`/`0x459964`)。
    /// 本实现在这里合成一张 RGBA 纹理,后端两类(soft/wgpu)都按普通资产
    /// 消费,不感知影片。
    pub fn resolve_movie(&mut self, asset: &[u8], frame: u32) -> Result<&Texture, Diag> {
        self.movie_frame(asset, frame)?;
        let key = movie_cache_key(asset, frame);
        let entry = self.cache.get(&key).expect("movie frame ensured above");
        Ok(entry
            .result
            .as_ref()
            .expect("movie_frame 只在该帧成功时返回 Ok"))
    }

    /// 同 [`Self::resolve_movie`],返回共享句柄(`Rc` 克隆,O(1))。
    pub fn resolve_movie_shared(
        &mut self,
        asset: &[u8],
        frame: u32,
    ) -> Result<std::rc::Rc<Texture>, Diag> {
        self.movie_frame(asset, frame)
    }

    /// 注入一张**由宿主解码**的纹理(键 = 合成资产 key)。
    ///
    /// 用于 `MOVIE` 全屏影片:解码在平台端口之后(pl_mpeg/系统媒体栈),
    /// 像素按帧送进来,渲染端只负责当普通资产采样 —— 与 `LOAD[ID]` 的
    /// 合成资产同一模式。同键重复注入 = 覆盖(帧在变),不新增缓存项。
    pub fn insert_rgba(&mut self, key: &[u8], width: u32, height: u32, rgba: Vec<u8>) {
        debug_assert_eq!(rgba.len(), width as usize * height as usize * 4);
        let texture = std::rc::Rc::new(Texture {
            width,
            height,
            rgba,
        });
        let cached = Cached {
            resolved: Some(key.to_vec()),
            result: Ok(texture),
        };
        self.insert_cached(key.to_vec(), cached);
    }

    /// 取影片**常驻容器**(按 `YMV[FILE]` 原字节缓存;缺失/坏容器也缓存)。
    ///
    /// 官方 `0x459094` 在 `YMV[FILE]` 命令内把整份影片读进内存并**解混淆
    /// 全部平面**,之后 `YMV[FRAME]`(`0x45934c`)只在这份缓冲上切段 ⇒
    /// 播放期每帧只有两张 JPEG 解码。若每帧回包重读 + 重做 XOR,6MB 读取
    /// 与 zlib 解压会把 30fps 打穿(`Rc` 共享同一份,克隆 O(1))。
    fn movie_container(&mut self, asset: &[u8]) -> MovieSlot {
        if let Some(hit) = self.movies.get(asset) {
            return hit.clone();
        }
        let mut candidates: Vec<Vec<u8>> = vec![asset.to_vec()];
        let mut with_ext = asset.to_vec();
        with_ext.extend_from_slice(b".ymv");
        candidates.push(with_ext);
        let mut slot = MovieSlot::Missing;
        for candidate in candidates {
            let Ok(lookup) = LookupKey::new(&candidate, self.codec) else {
                continue;
            };
            if !self.source.exists(&lookup) {
                continue;
            }
            let Ok(bytes) = self.source.open(&lookup) else {
                continue;
            };
            slot = match yu_format::ymv::DecryptedYmv::parse(&bytes, &candidate) {
                Ok(container) => MovieSlot::Ready {
                    path: candidate,
                    container: std::rc::Rc::new(container),
                },
                Err(diag) => MovieSlot::Broken {
                    path: candidate,
                    code: diag.code.to_string(),
                    detail: diag.detail.clone(),
                },
            };
            break;
        }
        self.movies.insert(asset.to_vec(), slot.clone());
        slot
    }

    /// 物化影片帧并返回共享句柄(两个 `resolve_movie*` 的共同实现)。
    ///
    /// 换到新帧时把该资产的**上一帧**从缓存摘掉(已交给图层/快照的 `Rc`
    /// 不受影响,只是不再命中);失败的帧同样缓存,避免每帧重探。
    fn movie_frame(&mut self, asset: &[u8], frame: u32) -> Result<std::rc::Rc<Texture>, Diag> {
        let key = movie_cache_key(asset, frame);
        if !self.cache.contains_key(&key) {
            let cached = self.load_movie(asset, frame);
            self.insert_cached(key.clone(), cached);
            if let Some(previous) = self.movie_keeper.insert(asset.to_vec(), frame) {
                if previous != frame {
                    let stale = movie_cache_key(asset, previous);
                    self.cache.remove(&stale);
                    self.inserted.remove(&stale);
                    if let Some((bytes, _)) = self.meta.remove(&stale) {
                        self.resident_bytes = self.resident_bytes.saturating_sub(bytes);
                    }
                    self.fresh.remove(&stale);
                }
            }
        }
        self.touch(&key);
        let entry = self.cache.get(&key).expect("cache entry inserted above");
        match &entry.result {
            Ok(tex) => Ok(std::rc::Rc::clone(tex)),
            Err(diag) => {
                // 问题清单按**资产** key 记(合成键对使用者无意义)。
                let issue = AssetIssue {
                    key: asset.to_vec(),
                    code: diag.code.to_string(),
                    detail: diag.detail.clone(),
                };
                self.issues
                    .insert((asset.to_vec(), issue.code.clone()), issue);
                Err(diag.clone())
            }
        }
    }

    /// 本帧已记录的资产问题(按 `(key, code)` 升序)。
    pub fn issues(&self) -> Vec<AssetIssue> {
        self.issues.values().cloned().collect()
    }

    /// 已成功解码的纹理数(缓存未命中且解码成功时 +1)。
    pub fn decoded_count(&self) -> u64 {
        self.decoded
    }

    /// 当前缓存条目数。
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// 某 key 命中的实际路径(未解析/缺失时 `None`)。
    pub fn resolved_path(&self, key: &[u8]) -> Option<&[u8]> {
        self.cache
            .get(key)
            .and_then(|entry| entry.resolved.as_deref())
    }

    /// 解码并记账(VFS 读 + 图像解码;仅缓存未命中时调用)。
    fn load(&mut self, key: &[u8]) -> Cached {
        let started = std::time::Instant::now();
        let cached = self.load_inner(key);
        let elapsed_us = started.elapsed().as_micros() as u64;
        self.decode_us = self.decode_us.saturating_add(elapsed_us);
        if let Ok(texture) = &cached.result {
            self.decode_bytes = self.decode_bytes.saturating_add(texture.rgba.len() as u64);
            if std::env::var_os("YURIS_ASSET_TIMING").is_some() {
                log::warn!(
                    target: "yu-render",
                    "[asset] key={} size={}x{} bytes={} decode={}us",
                    display_bytes(key),
                    texture.width,
                    texture.height,
                    texture.rgba.len(),
                    elapsed_us
                );
            }
        }
        cached
    }

    /// 编码不一致时的**唯一同名**回退:脚本 key 按本工程 codec 解码,
    /// 目录内候选按本 codec 与另一 codec 各试一次;两边 Unicode 全名相等
    /// (含扩展名)且只有一个候选时返回其真实字节路径,否则 `None`。
    ///
    /// 只用于原始字节 miss 之后;原始命中永远优先。歧义(两个不同字节名
    /// 指向同一 Unicode)不猜,保持 `asset.missing`。
    fn name_encoding_fallback(&self, candidate: &[u8]) -> Option<Vec<u8>> {
        if candidate.iter().all(|b| *b < 0x80) {
            return None;
        }
        let script = match self.codec {
            Codec::Cp932 => yu_format::Codec::Cp932,
            Codec::Gbk => yu_format::Codec::Gbk,
        };
        let other = match script {
            yu_format::Codec::Cp932 => yu_format::Codec::Gbk,
            yu_format::Codec::Gbk => yu_format::Codec::Cp932,
        };
        let (dir, name) = split_dir_name(candidate, self.codec);
        let want = script.decode(name, name).ok()?;
        let dir_key = LookupKey::new(dir, self.codec).ok()?;
        let entries = self.source.list(&dir_key).ok()?;
        let mut hit: Option<Vec<u8>> = None;
        for entry in entries {
            if entry.kind != EntryKind::File || entry.name == name {
                continue;
            }
            let matched = [script, other].iter().any(|codec| {
                codec
                    .decode(&entry.name, &entry.name)
                    .ok()
                    .is_some_and(|text| text.eq_ignore_ascii_case(&want))
            });
            if matched {
                if hit.is_some() {
                    return None; // 歧义:宁可 miss 也不猜
                }
                hit = Some(entry.name.clone());
            }
        }
        let entry = hit?;
        let mut full = dir.to_vec();
        full.extend_from_slice(&entry);
        Some(full)
    }

    /// 诊断:同目录里是否存在**同一 Unicode 名、不同字节编码**的条目。
    ///
    /// 只拼进 `asset.missing` 的 detail,不改变查找结果。脚本字面量是
    /// CP932;若 YPF/散盘在非日语区打包,文件名会按主机 ACP(GBK)落盘,
    /// 两边 Unicode 相同、字节不同 —— 本地重打包/汉化后资源“消失”的
    /// 典型形态。官方查找按原始字节哈希,同样不会命中。
    fn encoding_mismatch_hint(&self, key: &[u8]) -> Option<String> {
        if key.iter().all(|b| *b < 0x80) {
            return None;
        }
        let script = match self.codec {
            Codec::Cp932 => yu_format::Codec::Cp932,
            Codec::Gbk => yu_format::Codec::Gbk,
        };
        let other = match script {
            yu_format::Codec::Cp932 => yu_format::Codec::Gbk,
            yu_format::Codec::Gbk => yu_format::Codec::Cp932,
        };
        let key_text = script.decode(key, key).ok()?;
        let (dir, name) = split_dir_name(key, self.codec);
        let name_text = script.decode(name, name).ok()?;
        let name_stem = strip_image_ext(&name_text);
        let dir_key = LookupKey::new(dir, self.codec).ok()?;
        let entries = self.source.list(&dir_key).ok()?;
        for entry in entries {
            if entry.kind != EntryKind::File || entry.name == name {
                continue;
            }
            for codec in [script, other] {
                let Ok(text) = codec.decode(&entry.name, &entry.name) else {
                    continue;
                };
                if strip_image_ext(&text).eq_ignore_ascii_case(name_stem) {
                    return Some(format!(
                        ";同名条目存在但编码不同: 脚本 {} `{}` ↔ 数据源 {} `{}`",
                        script.as_str(),
                        key_text,
                        codec.as_str(),
                        text
                    ));
                }
            }
        }
        None
    }

    /// 读包内字节（不做图像解码）。返回 `(命中候选, 原始字节)`；失败时
    /// 返回可直接入缓存的 `Cached`（与 `load_inner` 同诊断）。
    fn read_source(&mut self, key: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Cached> {
        let candidates = self.candidates(key);
        for (candidate, format) in candidates {
            let mut lookup = match LookupKey::new(&candidate, self.codec) {
                Ok(lookup) => lookup,
                Err(diag) => {
                    return Err(Cached {
                        resolved: None,
                        result: Err(unimplemented(
                            code::MISSING,
                            key.to_vec(),
                            format!(
                                "候选路径无法归一: {} ({})",
                                display_bytes(&candidate),
                                diag.detail
                            ),
                        )),
                    });
                }
            };
            let mut resolved = candidate.clone();
            if !self.source.exists(&lookup) {
                if !self.name_fallback {
                    continue;
                }
                let Some(actual) = self.name_encoding_fallback(&candidate) else {
                    continue;
                };
                let Ok(actual_key) = LookupKey::new(&actual, self.codec) else {
                    continue;
                };
                if !self.source.exists(&actual_key) {
                    continue;
                }
                log::info!(
                    target: "yu-render",
                    "[asset] name-encoding fallback: {} -> {}",
                    display_bytes(&candidate),
                    display_bytes(&actual)
                );
                self.name_fallback_hits = self.name_fallback_hits.saturating_add(1);
                resolved = actual;
                lookup = actual_key;
            }
            if let Some(format) = format {
                if format == ImageFormat::Psb || matches!(format, ImageFormat::Other(_)) {
                    return Err(Cached {
                        resolved: Some(resolved.clone()),
                        result: Err(unimplemented(
                            code::UNSUPPORTED,
                            key.to_vec(),
                            format!(
                                "命中 {}:引擎格式码 {} 没有解码器",
                                display_bytes(&resolved),
                                format.as_str()
                            ),
                        )),
                    });
                }
            }
            let bytes = match self.source.open(&lookup) {
                Ok(bytes) => bytes,
                Err(diag) => {
                    return Err(Cached {
                        resolved: Some(resolved.clone()),
                        result: Err(Diag::recoverable(code::MISSING, Boundary::Data)
                            .at_data(resolved.clone(), 0)
                            .with_detail(format!(
                                "命中 {} 但读取失败: {}",
                                display_bytes(&resolved),
                                diag.detail
                            ))),
                    });
                }
            };
            return Ok((resolved, bytes));
        }
        let tried = self.candidates(key).len();
        let hint = self.encoding_mismatch_hint(key).unwrap_or_default();
        Err(Cached {
            resolved: None,
            result: Err(Diag::recoverable(code::MISSING, Boundary::Data)
                .at_data(key.to_vec(), 0)
                .with_detail(format!(
                    "资产 {} 不存在;已尝试 {tried} 个候选{hint}",
                    display_bytes(key)
                ))),
        })
    }

    fn load_inner(&mut self, key: &[u8]) -> Cached {
        let (candidate, bytes) = match self.read_source(key) {
            Ok(value) => value,
            Err(cached) => return cached,
        };
        let decoded = image::load_from_memory(&bytes);
        match decoded {
            Ok(img) => {
                self.decoded += 1;
                Cached {
                    resolved: Some(candidate),
                    result: Ok(std::rc::Rc::new(Texture::from_rgba8(img.into_rgba8()))),
                }
            }
            Err(err) => Cached {
                resolved: Some(candidate.clone()),
                result: Err(Diag::recoverable(code::DECODE, Boundary::Data)
                    .at_data(candidate.clone(), 0)
                    .with_detail(format!(
                        "命中 {} 但解码失败: {err}",
                        display_bytes(&candidate)
                    ))),
            },
        }
    }

    /// 解码 `YMV` 影片的一帧(失败也计时/记字节,口径与 [`Self::load`] 一致)。
    fn load_movie(&mut self, asset: &[u8], frame: u32) -> Cached {
        let started = std::time::Instant::now();
        let cached = self.load_movie_inner(asset, frame);
        let elapsed_us = started.elapsed().as_micros() as u64;
        self.decode_us = self.decode_us.saturating_add(elapsed_us);
        if let Ok(texture) = &cached.result {
            self.decode_bytes = self.decode_bytes.saturating_add(texture.rgba.len() as u64);
            if std::env::var_os("YURIS_ASSET_TIMING").is_some() {
                log::warn!(
                    target: "yu-render",
                    "[movie] asset={} frame={} size={}x{} bytes={} decode={}us",
                    display_bytes(asset),
                    frame,
                    texture.width,
                    texture.height,
                    texture.rgba.len(),
                    elapsed_us
                );
            }
        }
        cached
    }

    /// 读影片资产 → 解析容器 → 解该帧两段 JPEG → 合成 RGBA8。
    ///
    /// 候选序:`YMV[FILE]` 原字节 → `原字节 + ".ymv"`(语料两种写法都有;
    /// 见 `yu-rt` 的 `resolve_movie_meta`,两处必须同口径,否则会出现
    /// 「VM 量到元数据、渲染端却找不到资产」)。
    ///
    /// **容器字节按资产缓存**(见 [`Self::movie_bytes`]):官方 `0x459094`
    /// 在 `YMV[FILE]` 时把整份影片读进内存,之后换帧只在该缓冲上切段
    /// (`0x45934c`);若每帧回包重读,单是 6MB 读取+解压就能把 30fps 打穿
    /// (实测掉到 30fps、render 1.35ms→10ms)。
    fn load_movie_inner(&mut self, asset: &[u8], frame: u32) -> Cached {
        let container = match self.movie_container(asset) {
            MovieSlot::Missing => {
                return Cached {
                    resolved: None,
                    result: Err(unimplemented(
                        code::MISSING,
                        asset.to_vec(),
                        format!("影片资产不存在: {}", display_bytes(asset)),
                    )),
                };
            }
            MovieSlot::Broken {
                path,
                code,
                detail,
            } => {
                return Cached {
                    resolved: Some(path),
                    result: Err(Diag::recoverable(code::MOVIE, Boundary::Data)
                        .at_data(asset.to_vec(), 0)
                        .with_detail(format!("{code}: {detail}"))),
                };
            }
            MovieSlot::Ready { path, container } => (path, container),
        };
        let (candidate, container) = container;
        let ymv = &container;
        let info = ymv.info();
        if frame >= info.frames {
            return Cached {
                resolved: Some(candidate),
                result: Err(Diag::recoverable(code::MOVIE, Boundary::Data)
                    .at_data(asset.to_vec(), 0)
                    .with_detail(format!(
                        "帧号 {frame} 越界(影片共 {} 帧;gfx 侧应已钳位)",
                        info.frames
                    ))),
            };
        }
        let Some((color_jpeg, alpha_jpeg)) = ymv.frame_planes(frame) else {
            return Cached {
                resolved: Some(candidate),
                result: Err(Diag::recoverable(code::MOVIE, Boundary::Data)
                    .at_data(asset.to_vec(), 0)
                    .with_detail(format!("第 {frame} 帧段区间不可读"))),
            };
        };
        let color = match image::load_from_memory(color_jpeg) {
            Ok(img) => img.to_rgb8(),
            Err(err) => {
                return Cached {
                    resolved: Some(candidate),
                    result: Err(Diag::recoverable(code::DECODE, Boundary::Data)
                        .at_data(asset.to_vec(), 0)
                        .with_detail(format!("颜色平面 JPEG 解码失败(帧 {frame}): {err}"))),
                };
            }
        };
        let (width, height) = (color.width(), color.height());
        if width == 0 || height == 0 {
            return Cached {
                resolved: Some(candidate),
                result: Err(Diag::recoverable(code::DECODE, Boundary::Data)
                    .at_data(asset.to_vec(), 0)
                    .with_detail(format!("颜色平面尺寸为 0(帧 {frame})"))),
            };
        }
        // alpha 平面:只有 `标志 == 1` 且段存在时才合成;尺寸不一致时按
        // 「无 alpha」处理(畸形容器不猜,记诊断)。
        let mut alpha_note = None;
        let alpha = alpha_jpeg.and_then(|jpeg| match image::load_from_memory(jpeg) {
            Ok(img) => {
                let luma = img.to_luma8();
                if luma.width() == width && luma.height() == height {
                    Some(luma.into_raw())
                } else {
                    alpha_note = Some(format!(
                        "alpha 平面 {}x{} 与颜色平面 {}x{} 不一致;按不透明处理",
                        luma.width(),
                        luma.height(),
                        width,
                        height
                    ));
                    None
                }
            }
            Err(err) => {
                alpha_note = Some(format!("alpha 平面 JPEG 解码失败: {err};按不透明处理"));
                None
            }
        });
        let mut rgba = Vec::with_capacity(color.as_raw().len() / 3 * 4);
        let rgb = color.as_raw();
        for (i, px) in rgb.chunks_exact(3).enumerate() {
            rgba.extend_from_slice(px);
            rgba.push(alpha.as_ref().map_or(255, |a| a[i]));
        }
        if let Some(note) = alpha_note {
            log::warn!(
                target: "yu-render",
                "[movie] {} 帧 {}:{}",
                display_bytes(asset),
                frame,
                note
            );
        }
        self.decoded += 1;
        Cached {
            resolved: Some(candidate),
            result: Ok(std::rc::Rc::new(Texture {
                width,
                height,
                rgba,
            })),
        }
    }

    /// 并行预热一批尚未缓存的资产：顺序读包 + 多线程图像解码，最后按输入
    /// 顺序写回缓存。互不依赖的资产解码可并行；帧边界/插入顺序不变。
    ///
    /// 官方 `CG[FILE]` 在命令处理体内同步装载；这里只缩小同一命令批次的
    /// 墙钟，不改变“命令所在帧完成”的语义。
    pub fn preload_parallel(&mut self, keys: &[Vec<u8>]) {
        let mut jobs = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for key in keys {
            if self.cache.contains_key(key) || !seen.insert(key.clone()) {
                continue;
            }
            match self.read_source(key) {
                Ok((candidate, bytes)) => jobs.push((key.clone(), candidate, bytes)),
                Err(cached) => {
                    self.insert_cached(key.clone(), cached);
                }
            }
        }
        if jobs.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        // 官方线程只有软件合成池 + 影片播放(REA 0.495 `0x463f50` / `0x422268`),
        // 资源解码是命令内单线程;这里的并行是登记分歧(DIV-GFX-067)。
        // 留一核给主线程/音频,上限 4;结果按输入序写回,像素与串行一致。
        let workers = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(3)
            .clamp(1, 4)
            .min(jobs.len());
        let chunk = jobs.len().div_ceil(workers);
        let decoded: Vec<(usize, Result<Texture, String>)> = std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for (base, part) in jobs.chunks(chunk).enumerate() {
                handles.push(scope.spawn(move || {
                    part.iter()
                        .enumerate()
                        .map(|(offset, (_, _, bytes))| {
                            let result = std::panic::catch_unwind(|| {
                                image::load_from_memory(bytes)
                                    .map(|img| Texture::from_rgba8(img.into_rgba8()))
                                    .map_err(|err| err.to_string())
                            })
                            .unwrap_or_else(|_| Err("PNG 解码线程 panic".to_string()));
                            (base * chunk + offset, result)
                        })
                        .collect::<Vec<_>>()
                }));
            }
            handles
                .into_iter()
                .flat_map(|handle| match handle.join() {
                    Ok(rows) => rows,
                    Err(_) => {
                        log::error!(
                            target: "yu-render",
                            "[asset] preload worker panic; affected assets fall back to lazy decode"
                        );
                        Vec::new()
                    }
                })
                .collect()
        });
        let elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.decode_us = self.decode_us.saturating_add(elapsed_us);
        for (index, result) in decoded {
            let (key, candidate, _) = &jobs[index];
            let cached = match result {
                Ok(texture) => {
                    self.decoded += 1;
                    self.decode_bytes = self.decode_bytes.saturating_add(texture.rgba.len() as u64);
                    Cached {
                        resolved: Some(candidate.clone()),
                        result: Ok(std::rc::Rc::new(texture)),
                    }
                }
                Err(err) => Cached {
                    resolved: Some(candidate.clone()),
                    result: Err(Diag::recoverable(code::DECODE, Boundary::Data)
                        .at_data(candidate.clone(), 0)
                        .with_detail(format!(
                            "命中 {} 但解码失败: {err}",
                            display_bytes(candidate)
                        ))),
                },
            };
            self.insert_cached(key.clone(), cached);
        }
    }

    fn candidates(&self, key: &[u8]) -> Vec<(Vec<u8>, Option<ImageFormat>)> {
        let mut out = Vec::new();
        if has_extension(key, self.codec) {
            out.push((key.to_vec(), known_extension_format(key)));
        }
        for format in &self.probe_order {
            let Some(ext) = format.extension() else {
                continue;
            };
            let mut candidate = key.to_vec();
            candidate.extend_from_slice(ext.as_bytes());
            out.push((candidate, Some(*format)));
        }
        if out.is_empty() {
            out.push((key.to_vec(), None));
        }
        out
    }
}

fn unimplemented(code: &'static str, path: Vec<u8>, detail: impl Into<String>) -> Diag {
    Diag::new(code, Severity::Unimplemented, Boundary::Data)
        .at_data(path, 0)
        .with_detail(detail)
}

/// 最后一个路径组件是否带扩展名(`.` 不在首位)。
fn has_extension(key: &[u8], codec: Codec) -> bool {
    let name = last_component(key, codec);
    match name.iter().rposition(|&b| b == b'.') {
        Some(0) | None => false,
        Some(_) => true,
    }
}

/// 最后一个路径组件，分隔符只在**单字节字符**位置上生效。
///
/// 不能对原始字节直接 `rsplit(b'\\')`：CP932 的尾字节区间(`0x40..=0x7E`)
/// 含 `\`(`0x5C`)，`ソ`=`83 5C`、`表`=`95 5C` 会被劈成两段，`cg/ソ.png`
/// 的“最后组件”会只剩 `.png`，随即被判成“无扩展名”，原 key 不参与探测，
/// 实际存在的资产报 `asset.missing`。
fn last_component(key: &[u8], codec: Codec) -> &[u8] {
    let mut start = 0;
    let mut i = 0;
    while i < key.len() {
        let step = if key[i] >= 0x80 && codec.is_lead(key[i]) && i + 1 < key.len() {
            2
        } else {
            1
        };
        if step == 1 && (key[i] == b'/' || key[i] == b'\\') {
            start = i + 1;
        }
        i += step;
    }
    &key[start..]
}

/// 按编解码器把路径劈成 `(父目录含尾部分隔符, 文件名)`。
fn split_dir_name(key: &[u8], codec: Codec) -> (&[u8], &[u8]) {
    let name = last_component(key, codec);
    let dir_len = key.len() - name.len();
    (&key[..dir_len], name)
}

/// 去掉已知图像扩展名(ASCII,大小写不敏感)。
fn strip_image_ext(text: &str) -> &str {
    const EXTS: [&str; 6] = [".png", ".bmp", ".jpg", ".jpeg", ".gif", ".psb"];
    let bytes = text.as_bytes();
    for ext in EXTS {
        if bytes.len() > ext.len()
            && bytes[bytes.len() - ext.len()..].eq_ignore_ascii_case(ext.as_bytes())
        {
            // 后缀首字节 `.` 是 ASCII ⇒ 切点在字符边界。
            return &text[..bytes.len() - ext.len()];
        }
    }
    text
}

/// 尾部 ASCII 后缀比较（大小写不敏感）。
///
/// 后缀首字节 `.`(`0x2E`)既不是 CP932 的 lead，也不在尾字节区间，因此后缀
/// 不可能切进双字节字符；逐字节比较即可，不需要先把整串折小写（整串折小写
/// 会改写双字节字符的尾字节）。
fn ends_with_ascii_ci(name: &[u8], suffix: &[u8]) -> bool {
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn known_extension_format(key: &[u8]) -> Option<ImageFormat> {
    if ends_with_ascii_ci(key, b".png") {
        Some(ImageFormat::Png)
    } else if ends_with_ascii_ci(key, b".bmp") {
        Some(ImageFormat::Bmp)
    } else if ends_with_ascii_ci(key, b".jpg") || ends_with_ascii_ci(key, b".jpeg") {
        Some(ImageFormat::Jpg)
    } else if ends_with_ascii_ci(key, b".gif") {
        Some(ImageFormat::Gif)
    } else if ends_with_ascii_ci(key, b".psb") {
        Some(ImageFormat::Psb)
    } else {
        None
    }
}

fn display_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if (0x20..0x7f).contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

/// 影片帧的合成缓存键。
///
/// 以 NUL 开头 ⇒ 不可能与真实资产 key 冲突(资产 key 是 CP932/ASCII 路径);
/// 同一 `(资产, 帧)` 解码一次,换帧才产生新条目。
fn movie_cache_key(asset: &[u8], frame: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(asset.len() + 8);
    key.extend_from_slice(b"\x00ymv\x00");
    key.extend_from_slice(asset);
    key.push(0);
    key.extend_from_slice(&frame.to_le_bytes());
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
    use yu_vfs::MemSource;

    /// 造一个 `frames` 帧的 `YSMV` 容器(与官方产物同布局、同 XOR):
    /// 颜色段 = 3×2 的纯色 RGB JPEG,alpha 段 = 同尺寸灰度 JPEG。
    fn ymv_fixture(frames: u32, color: [u8; 3], alpha: u8) -> Vec<u8> {
        use yu_format::ymv::{decrypt_in_place, MAGIC};
        let mut color_jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut color_jpeg, 90)
            .encode(
                &image::RgbImage::from_pixel(3, 2, image::Rgb(color)).into_raw(),
                3,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .expect("encode color jpeg");
        let mut alpha_jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut alpha_jpeg, 90)
            .encode(
                &image::GrayImage::from_pixel(3, 2, image::Luma([alpha])).into_raw(),
                3,
                2,
                image::ExtendedColorType::L8,
            )
            .expect("encode alpha jpeg");
        let n = frames as usize;
        let tables_end = 0x20 + n * 12;
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&frames.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // alpha 标志
        out.extend_from_slice(&3u32.to_le_bytes()); // 宽
        out.extend_from_slice(&2u32.to_le_bytes()); // 高
        out.extend_from_slice(&30u32.to_le_bytes()); // fps
        out.extend_from_slice(&0u32.to_le_bytes());
        let mut off = tables_end;
        for _ in 0..n {
            out.extend_from_slice(&(off as u32).to_le_bytes());
            off += color_jpeg.len() + alpha_jpeg.len();
        }
        for _ in 0..n {
            out.extend_from_slice(&(color_jpeg.len() as u32).to_le_bytes());
        }
        for _ in 0..n {
            out.extend_from_slice(&(alpha_jpeg.len() as u32).to_le_bytes());
        }
        for _ in 0..n {
            let mut c = color_jpeg.clone();
            let mut a = alpha_jpeg.clone();
            decrypt_in_place(&mut c);
            decrypt_in_place(&mut a);
            out.extend_from_slice(&c);
            out.extend_from_slice(&a);
        }
        out
    }

    /// `YMV[FRAME]`:颜色 JPEG + alpha 灰度 JPEG → RGBA8(官方 `0x45934c`)。
    #[test]
    fn movie_frame_composes_color_with_alpha_plane() {
        let mut store = store(
            &[(b"pac/sample2.ymv", ymv_fixture(3, [10, 200, 30], 64))],
            vec![ImageFormat::Png],
        );
        // 原 key 不带扩展名 ⇒ 走 `+.ymv` 探测(与 VM 侧同口径)。
        let tex = store.resolve_movie(b"pac/sample2", 0).expect("frame 0");
        assert_eq!((tex.width, tex.height), (3, 2));
        let px = tex.pixel(1, 1).expect("pixel");
        assert!((px[0] as i32 - 10).abs() <= 2, "R={}", px[0]);
        assert!((px[1] as i32 - 200).abs() <= 3, "G={}", px[1]);
        assert!((px[2] as i32 - 30).abs() <= 3, "B={}", px[2]);
        assert!((px[3] as i32 - 64).abs() <= 2, "A={}", px[3]);
        assert_eq!(
            store.resolved_path(b"\x00ymv\x00pac/sample2\x00\x00\x00\x00\x00"),
            Some(b"pac/sample2.ymv".as_slice())
        );
        // 同帧第二次命中缓存。
        let _ = store.resolve_movie(b"pac/sample2", 0).expect("cached");
        assert_eq!(store.decoded_count(), 1);
        // 换帧:新帧解码,旧帧从缓存摘掉(每资产只留最近一帧,防长片无界增长)。
        let _ = store.resolve_movie(b"pac/sample2", 2).expect("frame 2");
        assert_eq!(store.decoded_count(), 2);
        assert_eq!(store.cache_len(), 1, "影片每资产只保留最近一帧");
        // 回到旧帧:已不在缓存,重新解码(结果一致)。
        let back = store.resolve_movie(b"pac/sample2", 0).expect("frame 0 again");
        assert_eq!((back.width, back.height), (3, 2));
        assert_eq!(store.cache_len(), 1);
        // 越界帧不猜。
        let err = store
            .resolve_movie(b"pac/sample2", 3)
            .expect_err("frame out of range");
        assert_eq!(err.code, code::MOVIE);
    }

    #[test]
    fn movie_missing_asset_reports_missing() {
        let mut store = store(&[], vec![ImageFormat::Png]);
        let err = store
            .resolve_movie(b"pac/none", 0)
            .expect_err("missing movie");
        assert_eq!(err.code, code::MISSING);
        let issues = store.issues();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, b"pac/none".to_vec());
    }

    fn png_2x1() -> Vec<u8> {
        let img = RgbaImage::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0, 255, 0, 128])
            }
        });
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(img.as_raw(), 2, 1, ExtendedColorType::Rgba8)
            .expect("encode fixture png");
        out
    }

    fn png_solid(rgba: [u8; 4]) -> Vec<u8> {
        let img = RgbaImage::from_pixel(2, 1, Rgba(rgba));
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(img.as_raw(), 2, 1, ExtendedColorType::Rgba8)
            .expect("encode fixture png");
        out
    }

    fn store(files: &[(&[u8], Vec<u8>)], order: Vec<ImageFormat>) -> TextureStore {
        let mut source = MemSource::new(Codec::Cp932);
        for (path, bytes) in files {
            source.insert(path, bytes.clone()).expect("insert");
        }
        TextureStore::new(Box::new(source), Codec::Cp932, order)
    }

    #[test]
    fn probes_in_frame_order_and_decodes_rgba8() {
        let mut store = store(
            &[(b"img/foo.png", png_2x1())],
            vec![ImageFormat::Bmp, ImageFormat::Png, ImageFormat::Jpg],
        );
        let tex = store.resolve(b"img/foo").expect("resolve");
        assert_eq!((tex.width, tex.height), (2, 1));
        assert_eq!(tex.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(tex.pixel(1, 0), Some([0, 255, 0, 128]));
        assert_eq!(
            store.resolved_path(b"img/foo"),
            Some(b"img/foo.png".as_slice())
        );
        // 第二次直接命中文档缓存,不再解码。
        let _ = store.resolve(b"img/foo").expect("cached resolve");
        assert_eq!(store.decoded_count(), 1);
        assert_eq!(store.cache_len(), 1);
    }

    #[test]
    fn existing_raw_key_is_tried_first() {
        let mut store = store(&[(b"img/bar.png", png_2x1())], vec![ImageFormat::Png]);
        let tex = store.resolve(b"img/bar.png").expect("raw key");
        assert_eq!(tex.width, 2);
    }

    /// `ソ`=`83 5C`：尾字节是 `\`。它不能被当成分隔符，否则 `cg/ソ.png`
    /// 的“最后组件”只剩 `.png`，会被判成无扩展名，原 key 不参与探测。
    #[test]
    fn cp932_trail_backslash_is_not_a_path_separator() {
        let key = b"cg/\x83\x5c.png";
        assert!(has_extension(key, Codec::Cp932));
        assert_eq!(last_component(key, Codec::Cp932), b"\x83\x5c.png");
        assert_eq!(known_extension_format(key), Some(ImageFormat::Png));
        assert!(!has_extension(b"cg/\x83\x5c", Codec::Cp932));
        assert_eq!(last_component(b"cg/\x83\x5c", Codec::Cp932), b"\x83\x5c");
    }

    #[test]
    fn cp932_name_with_extension_resolves_exact_key() {
        let mut store = store(&[(b"cg/\x83\x5c.png", png_2x1())], vec![ImageFormat::Png]);
        let tex = store.resolve(b"cg/\x83\x5c.png").expect("cp932 name");
        assert_eq!(tex.width, 2);
        assert_eq!(
            store.resolved_path(b"cg/\x83\x5c.png"),
            Some(b"cg/\x83\x5c.png".as_slice())
        );
    }

    /// 回归护栏：所有 CP932 lead 与尾字节 `0x5C`(`\`) 的组合都不得被当成
    /// 路径分隔符；带 `.png` 后缀时必须判为“有扩展名”。
    #[test]
    fn last_component_never_splits_trail_backslash() {
        for lead in (0x81u8..=0x9f).chain(0xe0..=0xfc) {
            let mut key = b"cg/".to_vec();
            key.push(lead);
            key.push(0x5c);
            assert_eq!(
                last_component(&key, Codec::Cp932),
                &[lead, 0x5c],
                "lead={lead:#04x} 被切开"
            );
            assert!(!has_extension(&key, Codec::Cp932));
            key.extend_from_slice(b".png");
            assert!(has_extension(&key, Codec::Cp932), "lead={lead:#04x}");
        }
    }

    #[test]
    fn parallel_preload_matches_sequential_and_caches() {
        let files = [
            (b"a/img.png".as_slice(), png_solid([1, 2, 3, 255])),
            (b"b/img.png".as_slice(), png_solid([4, 5, 6, 255])),
            (b"c/img.png".as_slice(), png_solid([7, 8, 9, 255])),
        ];
        let keys = vec![
            b"a/img".to_vec(),
            b"a/img".to_vec(),
            b"b/img".to_vec(),
            b"c/img".to_vec(),
            b"missing".to_vec(),
        ];
        let unique = [b"a/img".as_slice(), b"b/img", b"c/img"];

        // 基准:逐个 `resolve`(串行、按 key 独立解码)。
        let mut sequential = store(&files, vec![ImageFormat::Png]);
        for key in unique {
            sequential.resolve(key).expect("sequential");
        }
        let missing = sequential.resolve(b"missing").expect_err("missing").code;

        let mut parallel = store(&files, vec![ImageFormat::Png]);
        parallel.preload_parallel(&keys);
        assert_eq!(parallel.decoded_count(), 3);

        for key in unique {
            let want = sequential.resolve(key).expect("sequential");
            let got = parallel.resolve(key).expect("preloaded");
            assert_eq!(
                (got.width, got.height, got.rgba.as_slice()),
                (want.width, want.height, want.rgba.as_slice()),
                "并行结果必须与串行逐字节一致: {}",
                String::from_utf8_lossy(key)
            );
            assert_eq!(parallel.resolved_path(key), sequential.resolved_path(key));
        }
        assert_eq!(
            parallel.resolve(b"missing").expect_err("missing").code,
            missing
        );

        // 重复预热不得重解码(已在缓存中的 key 直接跳过)。
        parallel.preload_parallel(&keys);
        assert_eq!(parallel.decoded_count(), 3);
    }

    #[test]
    fn cache_budget_evicts_only_unreferenced() {
        let mut store = store(
            &[
                (b"a/img.png", png_solid([1, 2, 3, 255])),
                (b"b/img.png", png_solid([4, 5, 6, 255])),
            ],
            vec![ImageFormat::Png],
        );
        // 每张 2×1 RGBA = 8B;预算 = 一张。
        store.set_budget_bytes(8);
        let held = store.resolve_shared(b"a/img").expect("a");
        store.resolve(b"b/img").expect("b");
        assert_eq!(store.resident_bytes(), 16, "插入阶段不回收(允许本帧暂时超额)");
        assert_eq!(
            store.resolved_path(b"a/img"),
            Some(b"a/img.png".as_slice()),
            "图层/快照仍持有的纹理不得回收"
        );

        drop(held);
        store.set_budget_bytes(8);
        assert!(
            store.resolved_path(b"a/img").is_none(),
            "无外部引用后应被预算回收"
        );
        assert_eq!(store.resolved_path(b"b/img"), Some(b"b/img.png".as_slice()));
        assert_eq!(store.evicted_count(), 1);
        assert_eq!(store.resident_bytes(), 8);
    }

    #[test]
    fn preload_batch_survives_budget_until_next_frame() {
        let mut store = store(
            &[
                (b"a/img.png", png_solid([1, 2, 3, 255])),
                (b"b/img.png", png_solid([4, 5, 6, 255])),
            ],
            vec![ImageFormat::Png],
        );
        store.set_budget_bytes(8);
        store.preload_parallel(&[b"a/img".to_vec(), b"b/img".to_vec()]);
        assert_eq!(store.resident_bytes(), 16, "预热允许本帧暂时超额");

        // 预热发生在渲染前:本帧 begin_frame 必须豁免刚插入的条目,
        // 否则渲染首用会重新解码。
        store.begin_frame();
        assert!(store.resolved_path(b"a/img").is_some());
        assert!(store.resolved_path(b"b/img").is_some());
        assert_eq!(store.resident_bytes(), 16);

        // 下一帧仍无引用 ⇒ 按 LRU 回收旧条目。
        store.begin_frame();
        assert!(store.resolved_path(b"a/img").is_none());
        assert_eq!(store.resolved_path(b"b/img"), Some(b"b/img.png".as_slice()));
        assert_eq!(store.resident_bytes(), 8);
    }

    #[test]
    fn missing_asset_has_diag_and_issue() {
        let mut store = store(&[], vec![ImageFormat::Png, ImageFormat::Bmp]);
        let err = store.resolve(b"nope").expect_err("must fail");
        assert_eq!(err.code, code::MISSING);
        let issues = store.issues();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, b"nope");
        assert_eq!(issues[0].code, code::MISSING);
        assert!(issues[0].detail.contains("已尝试 2 个候选"));
    }

    /// 包内名字按主机 ACP(GBK)落盘、脚本字面量是 CP932 时,原始字节
    /// miss 后按“同名 Unicode 唯一”回退到真实条目。
    #[test]
    fn resolves_same_name_in_other_encoding() {
        // 包内: cgsys/x/tip_s_／.png(GBK: A3 AF)
        // 脚本:cgsys/x/tip_s_／(CP932: 81 5E)
        let mut store = store(
            &[(b"cgsys/x/tip_s_\xa3\xaf.png", png_2x1())],
            vec![ImageFormat::Png],
        );
        let tex = store.resolve(b"cgsys/x/tip_s_\x81\x5e").expect("回退命中");
        assert_eq!(tex.width, 2);
        assert_eq!(
            store.resolved_path(b"cgsys/x/tip_s_\x81\x5e"),
            Some(b"cgsys/x/tip_s_\xa3\xaf.png".as_slice())
        );
        assert_eq!(store.name_fallback_hits(), 1);
    }

    /// 关掉兼容回退(或出现歧义)时,`asset.missing` 必须把“同名异码”
    /// 的条目指出来,便于当场识别坏包。
    #[test]
    fn missing_asset_reports_same_name_in_other_encoding_when_fallback_off() {
        let mut store = store(
            &[(b"cgsys/x/tip_s_\xa3\xaf.png", png_2x1())],
            vec![ImageFormat::Png],
        );
        store.set_name_fallback(false);
        let err = store
            .resolve(b"cgsys/x/tip_s_\x81\x5e")
            .expect_err("回退关闭时必须 miss");
        assert_eq!(err.code, code::MISSING);
        let detail = &store.issues()[0].detail;
        assert!(
            detail.contains("编码不同") && detail.contains("gbk"),
            "detail 必须指出 GBK 同名条目: {detail}"
        );
    }

    /// 原始字节命中优先:同名的 CP932/GBK 两条并存时,不得走回退。
    #[test]
    fn exact_bytes_win_over_encoding_fallback() {
        let mut store = store(
            &[
                (b"cgsys/x/tip_s_\xa3\xaf.png", png_solid([1, 2, 3, 255])),
                (b"cgsys/x/tip_s_\x81\x5e.png", png_solid([4, 5, 6, 255])),
            ],
            vec![ImageFormat::Png],
        );
        let tex = store.resolve(b"cgsys/x/tip_s_\x81\x5e").expect("exact");
        assert_eq!(tex.pixel(0, 0), Some([4, 5, 6, 255]));
        assert_eq!(store.name_fallback_hits(), 0);
    }

    #[test]
    fn decode_failure_is_terminal_for_first_hit() {
        let mut store = store(
            &[
                (b"broken.png", b"not a png".to_vec()),
                (b"broken.bmp", png_2x1()),
            ],
            vec![ImageFormat::Png, ImageFormat::Bmp],
        );
        let err = store.resolve(b"broken").expect_err("decode must fail");
        assert_eq!(err.code, code::DECODE);
        assert_eq!(store.decoded_count(), 0);
    }

    #[test]
    fn psb_is_reported_as_unsupported() {
        let mut store = store(&[(b"cg/x.psb", vec![0u8; 4])], vec![ImageFormat::Psb]);
        let err = store.resolve(b"cg/x").expect_err("psb unsupported");
        assert_eq!(err.code, code::UNSUPPORTED);
        assert_eq!(store.issues()[0].code, code::UNSUPPORTED);
    }

    #[test]
    fn solid_black_and_other_refs() {
        let mut store = store(&[], vec![ImageFormat::Png]);
        let black = store
            .resolve_ref(&TextureRef::SolidBlack1x1)
            .expect("solid");
        assert_eq!((black.width, black.height), (1, 1));
        assert_eq!(black.pixel(0, 0), Some([0, 0, 0, 255]));

        for texture in [
            TextureRef::None,
            TextureRef::Snapshot(yu_gfx::CopyId(1)),
            TextureRef::Offscreen(yu_gfx::LayerId::new(b":/W0/X".to_vec())),
        ] {
            let err = store.resolve_ref(&texture).expect_err("unsupported");
            assert_eq!(err.code, code::UNSUPPORTED);
        }
        assert!(store.issues().is_empty(), "非资产 ref 不进入缺失清单");
    }
}
