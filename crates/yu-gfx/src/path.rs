//! 图层 ID 路径规范化。
//!
//! 手册 `basic/layer.html`:
//! - ID 是路径语义:`/`、`.`、`:`(根)、`..` 可用;
//! - 默认根图层组是 `W0`,当前路径初始在其中;
//! - `ID` 末尾 `/` = 图层组(文件夹)。
//!
//! 引擎证据(旁线 `docs/status.md` §3.23):`IDNO` 的实现是
//! `ID + "." + 十进制(IDNO)` 后进同一套路径解析(`CGEND 0x43d334`,
//! `CGINFO 0x43d4a4`;`CG` 共用同一 `(ID, IDNO)` 定位)。
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
/// `idno` 存在时按引擎约定拼成 `ID + "." + 十进制`。
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
    fn idno_is_decimal_suffix() {
        let p = normalize(b"bg/title", Some(2), false);
        assert_eq!(p.path.display(), ":/W0/bg/title.2");
        assert!(!p.is_dir);
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
