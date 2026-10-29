//! 声音层 ID 的路径规范化与树形键。
//!
//! 官方 0.495(`sha256 4f5eefa3…`)声音层是一棵**名字树**,不是扁平表:
//! `SOUND[ID=]` 的 ID 先经 `FUN_004141e4` 规范化,再用 `FUN_00414559`
//! 按 `/` 逐段下行查找(`0x44e235` / `0x44f500` / `0x44f346` 三处调用,
//! 前缀指针 `[0x801718]`)。
//!
//! 证据锚点:
//! - 前缀 `"S0"` = `.data:0x80bf54`,启动时复制进 `[0x801718]`
//!   (`0x462592..0x4625a6`),随后用该前缀建出根层 `S0`(`0x4625be`)。
//! - 字符分类表 `0x689b80` 运行时只有三项非零:`'.'`(0x2e)、`'/'`(0x2f)、
//!   `':'`(0x3a)(运行时 ReadProcessMemory 实测)。ID 不含这三者走
//!   快速路径(`前缀 + "/" + ID`);含任一者走慢路径(规范化)。
//! - 慢路径:绝对 ID 以 `:` 开头(`:/` 或 `:`)时不加前缀;折叠连续 `/`、
//!   去掉 `./`、`../` 回退一级、去掉末尾 `/`;段内出现空格返回 -1
//!   (调用方整条命令不应用)。
//! - 两条路径在**原 ID 以 `/` 结尾**时都返回 1;`SOUNDEND` 用这个返回值
//!   区分「停止单个声音」与「递归停止子树」(`0x44f369..0x44f378`)。
//!
//! 这些特殊字符都在 0x40 以下,不可能是 CP932 双字节字符的尾字节,
//! 因此按字节扫描与官方按字符宽度跳步等价。

/// 声音层树的根层名(官方 `[0x801718]` 前缀)。
pub const SOUND_ROOT: &[u8] = b"S0";

/// 规范化结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedId {
    /// 层表键(树路径,`/` 分隔,无末尾 `/`)。
    pub key: Vec<u8>,
    /// 原 ID 是否以 `/` 结尾(官方 `FUN_004141e4` 返回 1 的条件)。
    pub trailing_slash: bool,
}

/// 规范化一个 `SOUND`/`SOUNDINFO`/`SOUNDEND` 的层 ID。
///
/// 返回 `None` 表示官方慢路径的空格检查失败(`FUN_004141e4` 返回 -1),
/// 调用方应整条命令不应用并记 `snd.layer.invalid_id`。
pub fn normalize(id: &[u8]) -> Option<NormalizedId> {
    let trailing_slash = id.last() == Some(&b'/');
    let special = id.iter().any(|&b| b == b'.' || b == b'/' || b == b':');
    if !special {
        // 快速路径:前缀 + "/" + ID。空 ID 的输出是 "S0/"(官方同样把
        // 前缀与分隔符写进输出缓冲,末尾 `/` 只从输入串里剥掉)。
        let mut key = SOUND_ROOT.to_vec();
        key.push(b'/');
        key.extend_from_slice(id);
        return Some(NormalizedId {
            key,
            trailing_slash,
        });
    }
    // 慢路径:先拼出临时串(绝对 ID 去掉 `:` / `:/`)。
    let mut tmp: Vec<u8> = Vec::new();
    let rest = if id.first() == Some(&b':') {
        if id.get(1) == Some(&b'/') {
            &id[2..]
        } else {
            &id[1..]
        }
    } else {
        tmp.extend_from_slice(SOUND_ROOT);
        if id.first() != Some(&b'/') {
            tmp.push(b'/');
        }
        tmp.extend_from_slice(id);
        &[][..]
    };
    if !rest.is_empty() {
        tmp.extend_from_slice(rest);
    }
    if tmp.contains(&b' ') {
        return None;
    }
    // 逐段规范化:空段(连续 `/`)丢弃;非末段的 `.` 丢弃;非末段的 `..`
    // 回退一级;其余原样保留。末尾 `/` 自然被空段规则吃掉。
    let mut segs: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    for i in 0..=tmp.len() {
        if i == tmp.len() || tmp[i] == b'/' {
            segs.push(&tmp[start..i]);
            start = i + 1;
        }
    }
    let mut out: Vec<&[u8]> = Vec::new();
    let last = segs.len().saturating_sub(1);
    for (k, seg) in segs.iter().enumerate() {
        if seg.is_empty() {
            continue;
        }
        if *seg == b"." && k != last {
            continue;
        }
        if *seg == b".." && k != last {
            out.pop();
            continue;
        }
        out.push(seg);
    }
    let mut key = Vec::new();
    for (i, seg) in out.iter().enumerate() {
        if i != 0 {
            key.push(b'/');
        }
        key.extend_from_slice(seg);
    }
    Some(NormalizedId {
        key,
        trailing_slash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &[u8]) -> Vec<u8> {
        normalize(id).expect("id accepted").key
    }

    #[test]
    fn relative_ids_get_sound_root_prefix() {
        assert_eq!(key(b"BGM/A1"), b"S0/BGM/A1");
        assert_eq!(key(b"BGM"), b"S0/BGM");
        assert_eq!(key(b"BGM/"), b"S0/BGM");
        assert_eq!(key(b"/BGM"), b"S0/BGM");
        assert_eq!(key(b"BGM//A1"), b"S0/BGM/A1");
        assert_eq!(key(b"BGM/./A1"), b"S0/BGM/A1");
        assert_eq!(key(b"BGM/X/../A1"), b"S0/BGM/A1");
    }

    #[test]
    fn absolute_ids_skip_prefix() {
        // 脚本的 `:/S0/` 是声音树根层的绝对写法。
        assert_eq!(key(b":/S0/"), b"S0");
        assert_eq!(key(b":/S0/BGM/"), b"S0/BGM");
        assert_eq!(key(b":S0"), b"S0");
    }

    #[test]
    fn trailing_slash_is_reported() {
        assert!(normalize(b"BGM/").unwrap().trailing_slash);
        assert!(normalize(b":/S0/").unwrap().trailing_slash);
        assert!(!normalize(b"BGM/A1").unwrap().trailing_slash);
        assert!(!normalize(b"BGM").unwrap().trailing_slash);
    }

    #[test]
    fn empty_id_maps_to_root_with_slash() {
        let n = normalize(b"").unwrap();
        assert_eq!(n.key, b"S0/");
        assert!(!n.trailing_slash);
    }

    #[test]
    fn space_is_rejected_on_slow_path_only() {
        assert!(normalize(b"BGM/A 1").is_none());
        // 不含 `.` `/` `:` 的 ID 走快速路径,空格不触发检查(官方同型)。
        assert_eq!(key(b"BGM A"), b"S0/BGM A");
    }

    #[test]
    fn double_dot_at_tail_is_kept() {
        assert_eq!(key(b"BGM/.."), b"S0/BGM/..");
        assert_eq!(key(b"BGM/../A1"), b"S0/A1");
    }
}
