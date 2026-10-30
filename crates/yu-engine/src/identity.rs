//! 游戏身份:真实游戏名 / 窗口图标 / 版本变量(所有宿主共用)。
//!
//! 这些值是**宿主层变量**,不是 VM 语义;放在 `yu-engine` 是为了让
//! 桌面窗口、serve 页面、将来的 wasm / Android 宿主拿到同一份事实,
//! 而不是各自猜一遍。
//!
//! 全部来自工程自身的数据或官方产物,不写死:
//!
//! | 变量 | 来源 | 证据 |
//! |---|---|---|
//! | 游戏名 | `ysbin/yscfg.ybn`(YSCF)标题字段,CP932 | `docs/formats/ybn.md`;`yu_format::yscf` |
//! | YU-RIS 版本 | `ysbin/ysc.ybn`(YSCM)容器头版本,495 ⇒ `0.495` | `yu-rt/examples/version_probe.rs`(七表交叉一致) |
//! | 窗口图标 | 工程根 `*.exe` 的 `RT_GROUP_ICON` / `RT_ICON` 资源 | PE 资源目录;官方 `YSIcon.exe` 改写该资源 |
//! | 内核版本 | 本 crate 包版本 | `env!("CARGO_PKG_VERSION")` |
//!
//! 图标解码只覆盖 DIB(`BI_RGB`,32/24/8bpp)。PNG 压缩图标(Vista 起
//! 常见于 256×256 项)暂不解码,遇到就按尺寸降序回退到 DIB 项;都没有
//! 时 `icon = None`(窗口用系统缺省图标),不伪造。

use std::path::{Path, PathBuf};

use yu_format::Codec as TextCodec;

use crate::project::{json_escape, Project};

/// 内核名(窗口标题/状态 JSON 用)。
pub const KERNEL_NAME: &str = "yuris-kernel";

/// 内核版本(工作区包版本)。
pub const KERNEL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 已解码的窗口图标(RGBA8,逐行从上到下)。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconImage {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` 字节。
    pub rgba: Vec<u8>,
}

/// 游戏身份(宿主可读变量)。
#[derive(Clone, Debug)]
pub struct Identity {
    /// 工程根目录名(游戏名缺失时的回退)。
    pub project_name: String,
    /// 工程 `yscfg.ybn` 标题 = 游戏真名;缺失/空串 = `None`。
    pub game_title: Option<String>,
    /// YU-RIS 引擎版本(`ysc.ybn` 头版本;495 ⇒ 0.495)。
    pub engine_version: u32,
    /// 内核版本。
    pub kernel_version: &'static str,
    /// 从官方 exe 资源抽出的窗口图标;缺失 = `None`。
    pub icon: Option<IconImage>,
}

impl Identity {
    /// 启动期构造(引擎装配层调用一次)。
    ///
    /// `title_bytes` 是 `yscfg.ybn` 的标题原始字节;`with_icon = false`
    /// 时跳过 exe 扫描(headless/用例不需要图标,省一次文件读取)。
    #[must_use]
    pub fn from_boot(
        project: &Project,
        title_bytes: &[u8],
        engine_version: u32,
        with_icon: bool,
    ) -> Self {
        let project_name = project
            .root()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let game_title = decode_title(title_bytes);
        let icon = if with_icon {
            load_icon(project.root())
        } else {
            None
        };
        Self {
            project_name,
            game_title,
            engine_version,
            kernel_version: KERNEL_VERSION,
            icon,
        }
    }

    /// 展示用游戏名:`yscfg` 标题优先,否则工程目录名。
    #[must_use]
    pub fn display_title(&self) -> &str {
        match &self.game_title {
            Some(title) if !title.is_empty() => title,
            _ => &self.project_name,
        }
    }

    /// `495 → "0.495"`、`1234 → "1.234"`。
    #[must_use]
    pub fn engine_version_text(&self) -> String {
        format!(
            "{}.{:03}",
            self.engine_version / 1000,
            self.engine_version % 1000
        )
    }

    /// 窗口标题:游戏真名 · YU-RIS 版本 · 内核版本。
    #[must_use]
    pub fn window_title(&self) -> String {
        format!(
            "{} · YU-RIS {} · {} {}",
            self.display_title(),
            self.engine_version_text(),
            KERNEL_NAME,
            self.kernel_version
        )
    }

    /// 单行摘要(日志用)。
    #[must_use]
    pub fn summary(&self) -> String {
        let icon = match &self.icon {
            Some(icon) => format!("{}x{}", icon.width, icon.height),
            None => "none".to_string(),
        };
        format!(
            "game=\"{}\" yuris={} kernel={} {} icon={icon}",
            self.display_title(),
            self.engine_version_text(),
            KERNEL_NAME,
            self.kernel_version
        )
    }

    /// `/api/status` 的 `identity` 对象(最小 JSON;宿主自拼时可用)。
    #[must_use]
    pub fn to_json(&self) -> String {
        let icon = match &self.icon {
            Some(icon) => format!("\"icon\":\"{}x{}\"", icon.width, icon.height),
            None => "\"icon\":null".to_string(),
        };
        format!(
            "{{\"game\":{},\"project\":{},\"engine_version\":{},\
             \"engine_version_text\":\"{}\",\"kernel\":\"{}\",\
             \"kernel_version\":\"{}\",{icon}}}",
            match &self.game_title {
                Some(title) => format!("\"{}\"", json_escape(title)),
                None => "null".to_string(),
            },
            format!("\"{}\"", json_escape(&self.project_name)),
            self.engine_version,
            self.engine_version_text(),
            KERNEL_NAME,
            self.kernel_version,
        )
    }
}

/// CP932 标题解码;去掉尾部 NUL/空白,空串视为缺失。
fn decode_title(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let text = TextCodec::Cp932
        .decode(bytes, b"ysbin/yscfg.ybn")
        .ok()?;
    let trimmed = text
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ---------------------------------------------------------------- 图标提取

/// 单个 exe 的读取上限(防御异常文件;正常游戏 exe 远小于此)。
const MAX_EXE_BYTES: u64 = 64 * 1024 * 1024;

/// 从工程根的 exe 资源里取窗口图标。
///
/// 多个 exe 时按文件大小降序尝试(游戏本体通常远大于 `エンジン設定.exe`
/// 这类附带工具);第一个能解出图标的胜出。
#[must_use]
pub fn load_icon(root: &Path) -> Option<IconImage> {
    let mut exes: Vec<(u64, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && has_exe_extension(path))
        .filter_map(|path| {
            let len = path.metadata().ok()?.len();
            Some((len, path))
        })
        .collect();
    exes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (len, path) in exes {
        if len == 0 || len > MAX_EXE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if let Some(icon) = icon_from_pe(&bytes) {
            log::debug!(target: "yu-engine::identity", "window icon: {}", path.display());
            return Some(icon);
        }
    }
    None
}

fn has_exe_extension(path: &Path) -> bool {
    path.extension()
        .map(|ext| ext.to_string_lossy().eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
}

/// PE 只读视图(节表 + 资源目录 RVA)。
struct Pe<'a> {
    data: &'a [u8],
    /// `(虚拟地址, 映射长度, 文件偏移, 文件长度)`。
    sections: Vec<(u32, u32, u32, u32)>,
    resource_rva: u32,
}

impl<'a> Pe<'a> {
    fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 0x40 || &data[..2] != b"MZ" {
            return None;
        }
        let pe = read_u32(data, 0x3C)? as usize;
        if pe + 24 > data.len() || &data[pe..pe + 4] != b"PE\0\0" {
            return None;
        }
        let sections = read_u16(data, pe + 6)? as usize;
        let optional_size = read_u16(data, pe + 20)? as usize;
        let optional = pe + 24;
        if optional + optional_size > data.len() {
            return None;
        }
        let magic = read_u16(data, optional)?;
        let data_dirs = match magic {
            0x10B => optional + 96,
            0x20B => optional + 112,
            _ => return None,
        };
        if data_dirs + 24 > data.len() {
            return None;
        }
        let resource_rva = read_u32(data, data_dirs + 16)?;
        let mut sections_out = Vec::with_capacity(sections);
        let table = optional + optional_size;
        for index in 0..sections {
            let at = table + index * 40;
            if at + 40 > data.len() {
                return None;
            }
            let virtual_size = read_u32(data, at + 8)?;
            let virtual_address = read_u32(data, at + 12)?;
            let raw_size = read_u32(data, at + 16)?;
            let raw_pointer = read_u32(data, at + 20)?;
            sections_out.push((
                virtual_address,
                virtual_size.max(raw_size),
                raw_pointer,
                raw_size,
            ));
        }
        Some(Self {
            data,
            sections: sections_out,
            resource_rva,
        })
    }

    fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        for (address, span, raw_pointer, raw_size) in &self.sections {
            if rva >= *address && rva < address.checked_add(*span)? {
                let delta = rva - address;
                if delta >= *raw_size {
                    return None;
                }
                return Some(*raw_pointer as usize + delta as usize);
            }
        }
        None
    }
}

/// 资源目录条目:`(名称/ID, 偏移)`;偏移高位置 1 表示子目录。
fn dir_entries(data: &[u8], base: usize, offset: usize) -> Option<Vec<(u32, u32)>> {
    let at = base.checked_add(offset)?;
    if at + 16 > data.len() {
        return None;
    }
    let named = read_u16(data, at + 12)? as usize;
    let ids = read_u16(data, at + 14)? as usize;
    let count = named + ids;
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let entry = at + 16 + index * 8;
        if entry + 8 > data.len() {
            return None;
        }
        out.push((read_u32(data, entry)?, read_u32(data, entry + 4)?));
    }
    Some(out)
}

fn sub_dir(offset: u32) -> Option<usize> {
    if offset & 0x8000_0000 != 0 {
        Some((offset & 0x7FFF_FFFF) as usize)
    } else {
        None
    }
}

/// 资源叶子(某一资源类型下所有 `ID → 字节`)。
fn resource_leaves<'a>(pe: &Pe<'a>, base: usize, want_type: u32) -> Vec<(u32, &'a [u8])> {
    let data = pe.data;
    let mut out = Vec::new();
    let Some(types) = dir_entries(data, base, 0) else {
        return out;
    };
    for (type_id, type_offset) in types {
        if type_id != want_type {
            continue;
        }
        let Some(type_dir) = sub_dir(type_offset) else {
            continue;
        };
        let Some(names) = dir_entries(data, base, type_dir) else {
            continue;
        };
        for (id, name_offset) in names {
            let Some(name_dir) = sub_dir(name_offset) else {
                continue;
            };
            let Some(languages) = dir_entries(data, base, name_dir) else {
                continue;
            };
            for (_, language_offset) in languages {
                if let Some(bytes) = resource_data(pe, base, language_offset as usize) {
                    out.push((id, bytes));
                    break;
                }
            }
        }
    }
    out
}

fn resource_data<'a>(pe: &Pe<'a>, base: usize, offset: usize) -> Option<&'a [u8]> {
    let data = pe.data;
    let at = base.checked_add(offset)?;
    if at + 16 > data.len() {
        return None;
    }
    let rva = read_u32(data, at)?;
    let size = read_u32(data, at + 4)? as usize;
    let start = pe.rva_to_offset(rva)?;
    data.get(start..start.checked_add(size)?)
}

/// `GRPICONDIR` → `(宽, bpp, RT_ICON id)`;宽 0 表示 256。
fn parse_icon_group(bytes: &[u8]) -> Vec<(u32, u16, u16)> {
    let Some(count) = read_u16(bytes, 4) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for index in 0..usize::from(count) {
        let at = 6 + index * 14;
        if at + 14 > bytes.len() {
            break;
        }
        let width = if bytes[at] == 0 {
            256
        } else {
            u32::from(bytes[at])
        };
        let Some(bpp) = read_u16(bytes, at + 6) else {
            break;
        };
        let Some(id) = read_u16(bytes, at + 12) else {
            break;
        };
        out.push((width, bpp, id));
    }
    out
}

/// 候选图标的优先序键(小者优先)。
///
/// 任务栏/标题栏按 32px 取图,所以 16..=64 这一档里离 32 最近的排最前;
/// 只有大图(128/256)的作品退化为取最大尺寸(缩放后仍比缺省图标好)。
fn icon_rank(entry: (u32, u16, u16)) -> (u8, u32, std::cmp::Reverse<u16>) {
    let (width, bpp, _) = entry;
    if (16..=64).contains(&width) {
        (0, width.abs_diff(32), std::cmp::Reverse(bpp))
    } else {
        (1, u32::MAX - width, std::cmp::Reverse(bpp))
    }
}

/// 从一份 PE 字节里抽图标;按尺寸优先序,第一个可解码的胜出。
fn icon_from_pe(data: &[u8]) -> Option<IconImage> {
    let pe = Pe::parse(data)?;
    if pe.resource_rva == 0 {
        return None;
    }
    let base = pe.rva_to_offset(pe.resource_rva)?;
    let groups = resource_leaves(&pe, base, 14);
    if groups.is_empty() {
        return None;
    }
    let icons = resource_leaves(&pe, base, 3);
    if icons.is_empty() {
        return None;
    }
    let mut candidates: Vec<(u32, u16, u16)> = groups
        .iter()
        .flat_map(|(_, bytes)| parse_icon_group(bytes))
        .collect();
    candidates.sort_by(|a, b| icon_rank(*a).cmp(&icon_rank(*b)));
    for (_, _, id) in candidates {
        let Some((_, bytes)) = icons.iter().find(|(icon_id, _)| *icon_id == u32::from(id)) else {
            continue;
        };
        if let Some(icon) = decode_dib_icon(bytes) {
            return Some(icon);
        }
    }
    None
}

/// DIB 形式的 `RT_ICON` 解码(`BITMAPINFOHEADER` + XOR 位图 + AND 掩码)。
///
/// 只接受 `BI_RGB` 的 32/24/8bpp;PNG 压缩项与其它位深返回 `None`,
/// 由调用方回退到下一尺寸。
fn decode_dib_icon(bytes: &[u8]) -> Option<IconImage> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG_SIGNATURE) {
        return None;
    }
    let header_size = read_u32(bytes, 0)? as usize;
    let width = read_i32(bytes, 4)?;
    let height_raw = read_i32(bytes, 8)?;
    let bpp = read_u16(bytes, 14)?;
    let compression = read_u32(bytes, 16)?;
    if header_size < 40 || compression != 0 || width <= 0 || height_raw == 0 {
        return None;
    }
    let width = width as usize;
    let full_height = height_raw.unsigned_abs() as usize;
    if full_height % 2 != 0 {
        return None;
    }
    let height = full_height / 2;
    if width == 0 || height == 0 || width > 512 || height > 512 {
        return None;
    }
    if !matches!(bpp, 8 | 24 | 32) {
        return None;
    }
    let top_down = height_raw < 0;

    let mut pixel_offset = header_size;
    let palette: Vec<[u8; 4]> = if bpp == 8 {
        let used = read_u32(bytes, 32)? as usize;
        let count = if used == 0 { 256 } else { used.min(256) };
        let end = pixel_offset.checked_add(count.checked_mul(4)?)?;
        if end > bytes.len() {
            return None;
        }
        let mut out = Vec::with_capacity(count);
        for index in 0..count {
            let at = pixel_offset + index * 4;
            out.push([bytes[at + 2], bytes[at + 1], bytes[at], 255]);
        }
        pixel_offset = end;
        out
    } else {
        Vec::new()
    };

    let stride = ((width * usize::from(bpp) + 31) / 32) * 4;
    let pixel_end = pixel_offset.checked_add(stride.checked_mul(height)?)?;
    if pixel_end > bytes.len() {
        return None;
    }

    let mut rgba = vec![0u8; width * height * 4];
    let mut any_alpha = false;
    for y in 0..height {
        let source_y = if top_down { y } else { height - 1 - y };
        let row = pixel_offset + source_y * stride;
        for x in 0..width {
            let (r, g, b, a) = match bpp {
                32 => {
                    let at = row + x * 4;
                    (bytes[at + 2], bytes[at + 1], bytes[at], bytes[at + 3])
                }
                24 => {
                    let at = row + x * 3;
                    (bytes[at + 2], bytes[at + 1], bytes[at], 255)
                }
                _ => {
                    let at = row + x;
                    let entry = *palette.get(usize::from(bytes[at]))?;
                    (entry[0], entry[1], entry[2], entry[3])
                }
            };
            if a != 0 {
                any_alpha = true;
            }
            let at = (y * width + x) * 4;
            rgba[at] = r;
            rgba[at + 1] = g;
            rgba[at + 2] = b;
            rgba[at + 3] = a;
        }
    }
    // 旧式 32bpp 图标 alpha 恒 0,透明度由 AND 掩码表达。
    if bpp == 32 && !any_alpha {
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
    }

    let needs_mask = bpp != 32 || !any_alpha;
    if needs_mask {
        let mask_offset = pixel_end;
        let mask_stride = ((width + 31) / 32) * 4;
        if let Some(mask_end) = mask_offset.checked_add(mask_stride.checked_mul(height)?) {
            if mask_end <= bytes.len() {
                for y in 0..height {
                    let source_y = if top_down { y } else { height - 1 - y };
                    let row = mask_offset + source_y * mask_stride;
                    for x in 0..width {
                        if bytes[row + x / 8] & (0x80 >> (x % 8)) != 0 {
                            rgba[(y * width + x) * 4 + 3] = 0;
                        }
                    }
                }
            }
        }
    }

    Some(IconImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}

fn read_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn read_i32(data: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}
