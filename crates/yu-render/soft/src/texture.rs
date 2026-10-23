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
use yu_vfs::{Codec, DataSource, LookupKey};

/// `asset.*` 诊断码。
pub mod code {
    /// 所有探测候选都不存在。
    pub const MISSING: &str = "asset.missing";
    /// 找到文件但解码失败。
    pub const DECODE: &str = "asset.decode";
    /// 格式已识别但没有解码器(PSB/未知码)。
    pub const UNSUPPORTED: &str = "asset.unsupported";
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

/// 缓存条目。
struct Cached {
    /// 实际命中的探测路径(缺失时 `None`)。
    resolved: Option<Vec<u8>>,
    result: Result<Texture, Diag>,
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
}

impl TextureStore {
    pub fn new(source: Box<dyn DataSource>, codec: Codec, probe_order: Vec<ImageFormat>) -> Self {
        Self {
            source,
            codec,
            probe_order,
            cache: BTreeMap::new(),
            issues: BTreeMap::new(),
            solid: Texture::solid_black_1x1(),
            decoded: 0,
        }
    }

    /// 开始新一帧:清空本帧问题清单(缓存保留)。
    pub fn begin_frame(&mut self) {
        self.issues.clear();
    }

    /// 解析一个资产 key。成功返回缓存中的纹理;失败返回 `asset.*` [`Diag`]。
    pub fn resolve(&mut self, key: &[u8]) -> Result<&Texture, Diag> {
        if !self.cache.contains_key(key) {
            let cached = self.load(key);
            self.cache.insert(key.to_vec(), cached);
        }
        let entry = self.cache.get(key).expect("cache entry inserted above");
        match &entry.result {
            Ok(_) => Ok(match &entry.result {
                Ok(tex) => tex,
                Err(_) => unreachable!("matched Ok above"),
            }),
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

    /// 解析 [`TextureRef`]。`Offscreen/Snapshot/None` 不归本模块物化。
    pub fn resolve_ref(&mut self, texture: &TextureRef) -> Result<&Texture, Diag> {
        match texture {
            TextureRef::Asset(key) => self.resolve(key),
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

    fn load(&mut self, key: &[u8]) -> Cached {
        let candidates = self.candidates(key);
        for (candidate, format) in candidates {
            let lookup = match LookupKey::new(&candidate, self.codec) {
                Ok(lookup) => lookup,
                Err(diag) => {
                    return Cached {
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
                    };
                }
            };
            if !self.source.exists(&lookup) {
                continue;
            }
            if let Some(format) = format {
                if format == ImageFormat::Psb || matches!(format, ImageFormat::Other(_)) {
                    return Cached {
                        resolved: Some(candidate.clone()),
                        result: Err(unimplemented(
                            code::UNSUPPORTED,
                            key.to_vec(),
                            format!(
                                "命中 {}:引擎格式码 {} 没有解码器",
                                display_bytes(&candidate),
                                format.as_str()
                            ),
                        )),
                    };
                }
            }
            let bytes = match self.source.open(&lookup) {
                Ok(bytes) => bytes,
                Err(diag) => {
                    return Cached {
                        resolved: Some(candidate.clone()),
                        result: Err(Diag::recoverable(code::MISSING, Boundary::Data)
                            .at_data(candidate.clone(), 0)
                            .with_detail(format!(
                                "命中 {} 但读取失败: {}",
                                display_bytes(&candidate),
                                diag.detail
                            ))),
                    };
                }
            };
            return match image::load_from_memory(&bytes) {
                Ok(img) => {
                    self.decoded += 1;
                    Cached {
                        resolved: Some(candidate),
                        result: Ok(Texture::from_rgba8(img.to_rgba8())),
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
            };
        }
        // 探测顺序本身可能为空(工程 yscfg 全 0):最后尝试原 key。
        let tried = self.candidates(key).len();
        Cached {
            resolved: None,
            result: Err(Diag::recoverable(code::MISSING, Boundary::Data)
                .at_data(key.to_vec(), 0)
                .with_detail(format!(
                    "资产 {} 不存在;已尝试 {tried} 个候选",
                    display_bytes(key)
                ))),
        }
    }

    fn candidates(&self, key: &[u8]) -> Vec<(Vec<u8>, Option<ImageFormat>)> {
        let mut out = Vec::new();
        if has_extension(key) {
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
fn has_extension(key: &[u8]) -> bool {
    let name = key
        .rsplit(|&b| b == b'/' || b == b'\\')
        .next()
        .unwrap_or(key);
    match name.iter().rposition(|&b| b == b'.') {
        Some(0) | None => false,
        Some(_) => true,
    }
}

fn known_extension_format(key: &[u8]) -> Option<ImageFormat> {
    let lower: Vec<u8> = key.iter().map(u8::to_ascii_lowercase).collect();
    if lower.ends_with(b".png") {
        Some(ImageFormat::Png)
    } else if lower.ends_with(b".bmp") {
        Some(ImageFormat::Bmp)
    } else if lower.ends_with(b".jpg") || lower.ends_with(b".jpeg") {
        Some(ImageFormat::Jpg)
    } else if lower.ends_with(b".gif") {
        Some(ImageFormat::Gif)
    } else if lower.ends_with(b".psb") {
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
    use yu_vfs::MemSource;

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
