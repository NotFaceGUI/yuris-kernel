//! 图层 ID 路径规范化。
//!
//! 手册 `basic/layer.html`:
//! - ID 是路径语义:`/`、`.`、`:`(根)、`..` 可用;
//! - 默认根图层组是 `W0`,当前路径初始在其中;
//! - `ID` 末尾 `/` = 图层组(文件夹)。
//!
//! 引擎证据(会话 F 重新锚定):`FUN_004141e4` 把“基准路径 + ID”规范化
//! (`:` 根、`/` 层级、`.`/`..`、空串),`FUN_00414559` 用 MurmurHash 查层表;
//! `FUN_00413dc8` 对 `TID`/`MID` 走同一套解析。`IDNO` 的拼接方式已重新
//! 锚定:**`<ID>.<十进制>`**(见 [`normalize`] 的锚点;DIV-GFX-027 勘误)。
//!
//! 规范化结果以 `:/` 开头、无尾部 `/`;组/层的区别由调用方单独记录。

use crate::layer::LayerId;

/// 默认根图层组(手册 `basic/layer.html`)。
pub const DEFAULT_BASE: &[u8] = b"W0";

/// 解析结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedPath {
    /// 规范化路径(`:/W0/ABC`)。
    pub path: LayerId,
    /// ID 末尾 `/`(图层组)。
    pub is_dir: bool,
}

/// 规范化一个图层 ID。
///
/// `idno` 存在时拼成 `<ID>.<十进制>`——**点分隔**(DIV-GFX-027 勘误,
/// 2026-11-03)。证据(0.495 官方处理体,capstone 线性反汇编):
///
/// - `YMV`(`0x458f51..0x458f96`)、`YMVEND`(`0x459ad5..0x459b14`)、
///   `YMVINFO`(`0x459c06..0x459c41`)三处同型:先把 ID 拷进缓冲,再从
///   常量 `0x808354`(字节 `2e 00 00 00`,即 `"."`)追加一个点,最后
///   以缓冲 NUL 位置为目标调用 `0x46a7c0`(`__ultoa`,基数 10)追加十进制;
/// - `CGACT` 的同型块 `0x42c533..0x42c576`(点常量同址)与
///   `0x42c735..0x42c77c` 亦同。
///
/// 早前把 `0x42c542..0x42c54c` 那段**点追加循环**误读成 itoa 追加,因而
/// 去掉了点(旧口径「多了一个点」不成立;语料 0 次使用 IDNO,差异未暴露)。
pub fn normalize(id: &[u8], idno: Option<i64>, trailing_slash: bool) -> ParsedPath {
    let mut bytes = id.to_vec();
    if let Some(n) = idno {
        bytes.push(b'.');
        bytes.extend_from_slice(n.to_string().as_bytes());
    }
    normalize_bytes(&bytes, trailing_slash)
}

/// 规范化已拼接 `IDNO` 的字节串。
pub fn normalize_bytes(bytes: &[u8], trailing_slash: bool) -> ParsedPath {
    let mut is_dir = trailing_slash;
    let mut rest = bytes;
    // 尾部分隔符 = 目录标记(`ABC/`、`:/W3/`)。
    while rest.last() == Some(&b'/') {
        is_dir = true;
        rest = &rest[..rest.len() - 1];
    }
    let absolute = rest.first() == Some(&b':');
    if absolute {
        rest = &rest[1..];
        while rest.first() == Some(&b'/') {
            rest = &rest[1..];
        }
    }

    let mut segs: Vec<&[u8]> = Vec::new();
    if !absolute {
        segs.push(DEFAULT_BASE);
    }
    for seg in rest.split(|&b| b == b'/') {
        if seg.is_empty() || seg == b"." {
            continue;
        }
        if seg == b".." {
            segs.pop();
            continue;
        }
        segs.push(seg);
    }

    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.extend_from_slice(b":/");
    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(seg);
    }
    ParsedPath {
        path: LayerId(out),
        is_dir,
    }
}

/// 父路径;根(`:/`)返回 `None`。
pub fn parent(path: &LayerId) -> Option<LayerId> {
    let b = path.as_bytes();
    if b == b":/" {
        return None;
    }
    let pos = b.iter().rposition(|&x| x == b'/')?;
    if pos <= 1 {
        Some(LayerId(b":/".to_vec()))
    } else {
        Some(LayerId(b[..pos].to_vec()))
    }
}

/// 路径深度(根 = 0;`:/W0` = 1)。
pub fn depth(path: &LayerId) -> usize {
    let b = path.as_bytes();
    if b == b":/" {
        return 0;
    }
    b.iter().filter(|&&x| x == b'/').count()
}

/// 路径的最后一段(显示用)。
pub fn leaf(path: &LayerId) -> &[u8] {
    let b = path.as_bytes();
    match b.iter().rposition(|&x| x == b'/') {
        Some(i) => &b[i + 1..],
        None => b,
    }
}

/// 子路径。
pub fn child(parent: &LayerId, name: &[u8]) -> LayerId {
    let mut b = parent.as_bytes().to_vec();
    if !b.ends_with(b"/") {
        b.push(b'/');
    }
    b.extend_from_slice(name);
    LayerId(b)
}

/// 是否为根。
pub fn is_root(path: &LayerId) -> bool {
    path.as_bytes() == b":/"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(id: &str) -> String {
        normalize(id.as_bytes(), None, false).path.display()
    }

    #[test]
    fn relative_ids_land_in_w0() {
        assert_eq!(n("ABC"), ":/W0/ABC");
        assert_eq!(n("./ABC"), ":/W0/ABC");
        assert_eq!(n("ABC/CG1"), ":/W0/ABC/CG1");
    }

    #[test]
    fn empty_id_normalizes_to_w0_path() {
        // 这是 TID="" 的陷阱:路径规范化把空串变成 :/W0。
        // 引擎对空 TID 直接跳过配对查询,因此调用方必须显式清 tid。
        assert_eq!(n(""), ":/W0");
    }

    #[test]
    fn absolute_ids_ignore_current_base() {
        assert_eq!(n(":/TEMP"), ":/TEMP");
        assert_eq!(n(":/W3/ABC"), ":/W3/ABC");
    }

    #[test]
    fn dotdot_clamps_at_root() {
        assert_eq!(n("../ABC"), ":/ABC");
        assert_eq!(n("../../ABC"), ":/ABC");
        assert_eq!(n("A/../B"), ":/W0/B");
    }

    #[test]
    fn trailing_slash_marks_dir() {
        let p = normalize(b"ABC/", None, false);
        assert!(p.is_dir);
        assert_eq!(p.path.display(), ":/W0/ABC");
        let p = normalize(b":/W3/", None, false);
        assert!(p.is_dir);
        assert_eq!(p.path.display(), ":/W3");
    }

    #[test]
    fn idno_is_dot_decimal_suffix() {
        // 官方 `0x808354` 的 `"."` + `__ultoa(10)`(DIV-GFX-027 勘误)。
        let p = normalize(b"bg/title", Some(2), false);
        assert_eq!(p.path.display(), ":/W0/bg/title.2");
        assert!(!p.is_dir);
        // 负数沿用 `__ultoa` 的十进制写法,不做特殊处理。
        let p = normalize(b"id", Some(-1), false);
        assert_eq!(p.path.display(), ":/W0/id.-1");
    }

    #[test]
    fn parent_depth_leaf() {
        let p = normalize(b":/W0/ABC", None, false).path;
        assert_eq!(parent(&p).unwrap().display(), ":/W0");
        assert_eq!(depth(&p), 2);
        assert_eq!(leaf(&p), b"ABC");
        assert!(is_root(&LayerId(b":/".to_vec())));
    }
}
