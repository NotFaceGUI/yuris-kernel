//! `yscfg` 图像扩展码 → 扩展名(单一真值表)。
//!
//! 两代码表不同:
//!
//! - 0.495 及更早(`FUN_0040c9b4`):
//!   `1=.png 2=.bmp 3=.jpg 4=.gif 5=.psb`;
//! - 0.500(`FUN_0046fb18`,`<v500-relirium>` 反编译 + `yscfg` 实测):
//!   `1=.bmp 2=.png 3=.jpg 4=.gif 5=.ydg 6=.webp 7=.jxl 8=.qoi 9=.avif`。
//!
//! 渲染侧(`yu-gfx::ImageFormat::from_code*`)与 VM 侧尺寸探测
//! (`resolve_asset_size`)必须用同一张表:一边认 `.ydg` 一边不认,会出现
//! 「渲染器画得出、`CGINFO[SX/SY]` 却为 0」——按钮命中矩形退化成点,
//! 点击永远不生效。

/// 版本带:0.500 换代表。
#[must_use]
pub const fn extension(code: u8, v500: bool) -> Option<&'static str> {
    if v500 {
        match code {
            1 => Some(".bmp"),
            2 => Some(".png"),
            3 => Some(".jpg"),
            4 => Some(".gif"),
            5 => Some(".ydg"),
            6 => Some(".webp"),
            7 => Some(".jxl"),
            8 => Some(".qoi"),
            9 => Some(".avif"),
            _ => None,
        }
    } else {
        match code {
            1 => Some(".png"),
            2 => Some(".bmp"),
            3 => Some(".jpg"),
            4 => Some(".gif"),
            5 => Some(".psb"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v500_code_table_swaps_first_two_and_adds_ydg() {
        assert_eq!(extension(1, true), Some(".bmp"));
        assert_eq!(extension(2, true), Some(".png"));
        assert_eq!(extension(5, true), Some(".ydg"));
        assert_eq!(extension(6, true), Some(".webp"));
        assert_eq!(extension(5, false), Some(".psb"));
        assert_eq!(extension(6, false), None);
    }
}
