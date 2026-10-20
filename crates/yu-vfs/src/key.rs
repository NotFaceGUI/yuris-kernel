//! 查找键归一。
//!
//! 只做三件事(architecture §1): 分隔符 `\`/`/` 统一、ASCII A–Z 折叠、
//! 多字节字符按编解码器整对跳过。`.`/`..` 组件被拒绝——这是数据源的安全
//! 不变量(禁止逃出挂载根),不是对官方路径语义的猜测。

use crate::codec::Codec;
use yu_log::{code, Boundary, Diag};

/// 归一后的字节查找键。空键表示挂载根。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LookupKey(Vec<u8>);

impl LookupKey {
    /// 根键(空路径)。
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// 从原始路径字节构造。空路径、纯分隔符都归一为根。
    pub fn new(raw: &[u8], codec: Codec) -> Result<Self, Diag> {
        let components = split_components(raw, codec)?;
        let mut bytes = Vec::new();
        for (i, component) in components.iter().enumerate() {
            if i > 0 {
                bytes.push(b'/');
            }
            bytes.extend_from_slice(&normalize_component(component, codec));
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// 归一后的组件切片(根返回空表)。
    pub fn components(&self) -> Vec<&[u8]> {
        if self.0.is_empty() {
            Vec::new()
        } else {
            self.0.split(|&b| b == b'/').collect()
        }
    }

    /// 父路径。根没有父(返回 `None`);顶层组件的父是根。
    pub fn parent(&self) -> Option<Self> {
        if self.0.is_empty() {
            return None;
        }
        match self.0.iter().rposition(|&b| b == b'/') {
            Some(i) => Some(Self(self.0[..i].to_vec())),
            None => Some(Self::root()),
        }
    }

    /// 最后一个组件(根返回 `None`)。
    pub fn file_name(&self) -> Option<&[u8]> {
        if self.0.is_empty() {
            return None;
        }
        match self.0.iter().rposition(|&b| b == b'/') {
            Some(i) => Some(&self.0[i + 1..]),
            None => Some(&self.0[..]),
        }
    }

    /// 诊断/测试用的可读形式(非 UTF-8 字节按替换符显示)。
    pub fn display(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

/// 按编解码器把原始路径切成组件。空组件(前导/重复/尾随分隔符)被折叠。
pub fn split_components(raw: &[u8], codec: Codec) -> Result<Vec<Vec<u8>>, Diag> {
    let mut out = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b == 0 {
            return Err(path_error(raw, "NUL byte in lookup key"));
        }
        if b >= 0x80 && codec.is_lead(b) && i + 1 < raw.len() {
            let trail = raw[i + 1];
            if trail == 0 {
                return Err(path_error(
                    raw,
                    "NUL byte inside a multibyte character",
                ));
            }
            current.push(b);
            current.push(trail);
            i += 2;
            continue;
        }
        if b == b'\\' || b == b'/' {
            flush_component(&mut out, &mut current, raw)?;
            i += 1;
            continue;
        }
        current.push(b);
        i += 1;
    }
    flush_component(&mut out, &mut current, raw)?;
    Ok(out)
}

fn flush_component(
    out: &mut Vec<Vec<u8>>,
    current: &mut Vec<u8>,
    raw: &[u8],
) -> Result<(), Diag> {
    if current.is_empty() {
        return Ok(());
    }
    let component = std::mem::take(current);
    if component == b"." || component == b".." {
        return Err(path_error(
            raw,
            "`.`/`..` components are rejected by the VFS safety invariant",
        ));
    }
    out.push(component);
    Ok(())
}

/// 单个组件的归一: 多字节整对跳过,其余 ASCII A–Z 折叠。
pub fn normalize_component(raw: &[u8], codec: Codec) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b >= 0x80 && codec.is_lead(b) && i + 1 < raw.len() {
            out.push(b);
            out.push(raw[i + 1]);
            i += 2;
        } else {
            out.push(b.to_ascii_lowercase());
            i += 1;
        }
    }
    out
}

fn path_error(raw: &[u8], detail: &str) -> Diag {
    Diag::recoverable(code::vfs::PATH, Boundary::Data)
        .at_data(raw.to_vec(), 0)
        .with_detail(detail.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_separators_and_ascii_case() {
        let key = LookupKey::new(br"CG\Ev/A.PNG", Codec::Cp932).unwrap();
        assert_eq!(key.as_bytes(), b"cg/ev/a.png");
        assert_eq!(key.components().len(), 3);
    }

    #[test]
    fn collapses_repeated_and_edge_separators() {
        let key = LookupKey::new(b"/a//b/", Codec::Cp932).unwrap();
        assert_eq!(key.as_bytes(), b"a/b");
    }

    #[test]
    fn empty_path_is_root() {
        assert!(LookupKey::new(b"", Codec::Cp932).unwrap().is_root());
        assert!(LookupKey::new(b"///", Codec::Cp932).unwrap().is_root());
    }

    #[test]
    fn cp932_trail_backslash_is_not_a_separator() {
        // 0x83 0x5C 是一个 CP932 双字节字符,trail 恰好是 '\'。
        let key = LookupKey::new(&[0x83, 0x5c, 0x41], Codec::Cp932).unwrap();
        assert_eq!(key.as_bytes(), &[0x83, 0x5c, 0x61]);
        assert_eq!(key.components().len(), 1);
    }

    #[test]
    fn gbk_trail_backslash_is_not_a_separator() {
        let key = LookupKey::new(&[0x81, 0x5c, 0x42], Codec::Gbk).unwrap();
        assert_eq!(key.as_bytes(), &[0x81, 0x5c, 0x62]);
        assert_eq!(key.components().len(), 1);
    }

    #[test]
    fn cp932_halfwidth_kana_does_not_swallow_next_byte() {
        let key = LookupKey::new(&[0xb1, 0x41], Codec::Cp932).unwrap();
        assert_eq!(key.as_bytes(), &[0xb1, 0x61]);
    }

    #[test]
    fn rejects_dot_components() {
        assert!(LookupKey::new(b"a/../b", Codec::Cp932).is_err());
        assert!(LookupKey::new(b"a/./b", Codec::Cp932).is_err());
    }

    #[test]
    fn rejects_nul() {
        assert!(LookupKey::new(b"a\0b", Codec::Cp932).is_err());
    }

    #[test]
    fn parent_and_file_name() {
        let key = LookupKey::new(b"a/b/c.bin", Codec::Cp932).unwrap();
        assert_eq!(key.file_name(), Some(b"c.bin".as_slice()));
        assert_eq!(key.parent().unwrap().as_bytes(), b"a/b");
        assert_eq!(key.parent().unwrap().parent().unwrap().as_bytes(), b"a");
        assert!(key
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .is_root());
        assert!(LookupKey::root().parent().is_none());
    }
}
