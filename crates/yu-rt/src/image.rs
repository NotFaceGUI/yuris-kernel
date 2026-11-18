//! 资产图像头解析(只读尺寸,不解码像素)。
//!
//! 引擎 `CGINFO[SX/SY]` 读的是已装载纹理的尺寸;本内核的 `yu-gfx` 不读文件,
//! 由持有数据源的 VM 在查询回退路径解析图像头并回填宿主(DIV-GFX-024 /
//! EVID-J3-03)。工程 `yscfg` 探测序里的四种格式:
//! `1=.png / 2=.bmp / 3=.jpg / 4=.gif`(见 `ImageFormat` 注释中的引擎证据)。
//!
//! **4.7x/4.8x 的 `.png` 条目实际是 YCG 容器**(见 `yu_format::ycg` 与
//! DIV-GFX-091):扩展名仍是 `.png`,字节以 `YCG\0` 开头。这里必须识别它的
//! 头部,否则 `CGINFO[SX/SY]` 对整代资产返回“尺寸未知”→ 精灵/按钮矩形
//! 退化成 1×1:logo 被按 1×1 放样且停在屏幕外,按钮命中区也只剩 1×1,
//! 表现为“有画面但点击无反应”。尺寸字段与官方 0.479 `FUN_004254e4`
//! 一致:`+0x04=width`、`+0x08=height`(LE)。

/// 从图像字节头返回 `(width, height)`;无法识别/截断返回 `None`。
pub(crate) fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 12 && bytes.starts_with(b"YCG\0") {
        let w = le32(&bytes[4..8]);
        let h = le32(&bytes[8..12]);
        // 与官方装载体同口径:超过 0x2000 的文件在 `CG[FILE]` 阶段就被拒,
        // 这里也不能回填一个“看似可用”的尺寸。
        if w > 0
            && h > 0
            && w <= yu_format::ycg::MAX_DIMENSION
            && h <= yu_format::ycg::MAX_DIMENSION
        {
            return Some((w, h));
        }
    }
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        return Some((be32(&bytes[16..20]), be32(&bytes[20..24])));
    }
    if bytes.len() >= 26 && bytes.starts_with(b"BM") {
        let w = le32(&bytes[18..22]) as i32;
        let h = le32(&bytes[22..26]) as i32;
        if w > 0 && h != 0 {
            return Some((w as u32, h.unsigned_abs()));
        }
    }
    if bytes.len() >= 10 && bytes.starts_with(b"GIF8") {
        return Some((le16(&bytes[6..8]), le16(&bytes[8..10])));
    }
    if bytes.len() >= 4 && bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg_size(bytes);
    }
    None
}

fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 3 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // 填充字节与独立标记无长度字段。
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // SOS 之后是熵编码数据,不再有尺寸段。
        if marker == 0xDA {
            return None;
        }
        if i + 4 > bytes.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            if i + 9 > bytes.len() {
                return None;
            }
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
            return Some((w as u32, h as u32));
        }
        if seg_len < 2 {
            return None;
        }
        i += 2 + seg_len;
    }
    None
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn le16(b: &[u8]) -> u32 {
    u32::from(u16::from_le_bytes([b[0], b[1]]))
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

#[cfg(test)]
mod tests {
    use super::pixel_size;

    #[test]
    fn png_ihdr() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend_from_slice(&800u32.to_be_bytes());
        b.extend_from_slice(&600u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(pixel_size(&b), Some((800, 600)));
    }

    #[test]
    fn bmp_header() {
        let mut b = vec![0u8; 54];
        b[0] = b'B';
        b[1] = b'M';
        b[18..22].copy_from_slice(&800i32.to_le_bytes());
        b[22..26].copy_from_slice(&(-600i32).to_le_bytes());
        assert_eq!(pixel_size(&b), Some((800, 600)));
    }

    #[test]
    fn gif_header() {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&800u16.to_le_bytes());
        b.extend_from_slice(&600u16.to_le_bytes());
        assert_eq!(pixel_size(&b), Some((800, 600)));
    }

    #[test]
    fn ycg_header() {
        // 4.7x/4.8x:扩展名 `.png`,字节是 `YCG\0` + width@0x04 + height@0x08
        // (官方 0.479 `FUN_004254e4`;真实样本 `cg/bg/logo.png` 为 1280×960)。
        let mut b = vec![0u8; 0x38];
        b[..4].copy_from_slice(b"YCG\0");
        b[0x04..0x08].copy_from_slice(&1280u32.to_le_bytes());
        b[0x08..0x0c].copy_from_slice(&960u32.to_le_bytes());
        assert_eq!(pixel_size(&b), Some((1280, 960)));
        // 头不足 12 字节 / 尺寸为 0 / 超过官方 0x2000 时不得误判。
        assert_eq!(pixel_size(b"YCG\0"), None);
        b[0x04..0x08].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(pixel_size(&b), None);
        b[0x04..0x08].copy_from_slice(&0x2001u32.to_le_bytes());
        assert_eq!(pixel_size(&b), None);
    }

    #[test]
    fn jpeg_sof0() {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        b.extend_from_slice(&600u16.to_be_bytes());
        b.extend_from_slice(&800u16.to_be_bytes());
        b.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        assert_eq!(pixel_size(&b), Some((800, 600)));
    }
}
