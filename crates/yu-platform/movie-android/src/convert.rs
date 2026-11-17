//! 纯数据转换:MediaCodec ByteBuffer 的 YUV420 → RGBA,以及多声道 PCM →
//! 立体声。
//!
//! 单独放这里是为了能在**非 Android** 上跑单测(Android 真机不可依赖)。
//! 调用方(`android` 模块)只负责从解码输出缓冲取出字节、stride/slice/
//! crop,然后调这些安全函数;所有越界访问都退回中性值,不会 panic。

/// 把 YUV420 平面转成 RGBA8(行优先)。
///
/// 支持 I420(planar)与 NV12/NV21(semi-planar):两者只差 U/V 的
/// `pixel_stride`(1 vs 2)与起始偏移。系数用 ITU-R BT.601 limited range
/// (Y 16..235,Cb/Cr 16..240)展开为 full range,与内置 pl_mpeg 后端同一
/// 口径(官方 MCI 由系统解码器做等价转换)。
///
/// - `*_offset`:裁剪原点在对应平面里的**字节偏移**(crop 左/上);
/// - `*_stride`:行 stride(ByteBuffer 布局由输出格式给出);
/// - `u/v_pixel_stride`:U/V 平面像素步长(I420 = 1,NV12/NV21 = 2)。
#[allow(clippy::too_many_arguments)]
pub fn yuv420_to_rgba(
    width: usize,
    height: usize,
    y: &[u8],
    y_stride: usize,
    y_offset: usize,
    u: &[u8],
    u_stride: usize,
    u_pixel_stride: usize,
    u_offset: usize,
    v: &[u8],
    v_stride: usize,
    v_pixel_stride: usize,
    v_offset: usize,
    out: &mut Vec<u8>,
) {
    let pixels = width.saturating_mul(height);
    out.clear();
    out.resize(pixels.saturating_mul(4), 0);
    if width == 0 || height == 0 {
        return;
    }
    for row in 0..height {
        for col in 0..width {
            let y_index = y_offset
                .saturating_add(row.saturating_mul(y_stride))
                .saturating_add(col);
            let u_index = u_offset
                .saturating_add((row / 2).saturating_mul(u_stride))
                .saturating_add((col / 2).saturating_mul(u_pixel_stride));
            let v_index = v_offset
                .saturating_add((row / 2).saturating_mul(v_stride))
                .saturating_add((col / 2).saturating_mul(v_pixel_stride));
            // 越界(被裁剪的最后一列/行)按中性值处理:比 panic 安全。
            let luma = i32::from(y.get(y_index).copied().unwrap_or(16));
            let cb = i32::from(u.get(u_index).copied().unwrap_or(128)) - 128;
            let cr = i32::from(v.get(v_index).copied().unwrap_or(128)) - 128;
            let c = (luma - 16).max(0);
            let r = ((298 * c + 409 * cr + 128) >> 8).clamp(0, 255) as u8;
            let g = ((298 * c - 100 * cb - 208 * cr + 128) >> 8).clamp(0, 255) as u8;
            let b = ((298 * c + 516 * cb + 128) >> 8).clamp(0, 255) as u8;
            let o = (row * width + col) * 4;
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = 255;
        }
    }
}

/// MediaCodec ByteBuffer 视频输出的常见 YUV420 布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferYuvLayout {
    /// I420:Y 平面 + 独立的 U/V 平面。
    Planar,
    /// NV12:Y 平面 + 交错 UV 平面。
    SemiPlanar,
}

/// Android `COLOR_FormatYUV420Planar`。
pub const COLOR_FORMAT_YUV420_PLANAR: i32 = 19;
/// Android `COLOR_FormatYUV420SemiPlanar`(NV12)。
pub const COLOR_FORMAT_YUV420_SEMI_PLANAR: i32 = 21;
/// Android `COLOR_FormatYUV420Flexible`。
pub const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 0x7F42_0888;

/// 把 MediaCodec ByteBuffer 视频输出转成 RGBA8。
///
/// 输入是**解码器输出缓冲**(已按 `info.offset` 切好),再给出行 stride/
/// slice-height 与 crop 矩形。支持 I420 与 NV12;`flexible`/未知厂商格式
/// 先按 NV12 尝试,长度不够再按 I420。返回实际使用的布局与显示尺寸。
///
/// 这条路径不经过 `AImage`,因此绕开高通 UBWC 等 Surface 私有格式。
#[allow(clippy::too_many_arguments)]
pub fn yuv420_buffer_to_rgba(
    buffer: &[u8],
    width: usize,
    height: usize,
    color_format: i32,
    stride: usize,
    slice_height: usize,
    crop_left: usize,
    crop_top: usize,
    crop_width: usize,
    crop_height: usize,
    out: &mut Vec<u8>,
) -> Option<(usize, usize, BufferYuvLayout)> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = stride.max(width);
    let slice_height = slice_height.max(height);
    let crop_left = crop_left.min(width.saturating_sub(1));
    let crop_top = crop_top.min(height.saturating_sub(1));
    let crop_width = crop_width.min(width - crop_left).max(1);
    let crop_height = crop_height.min(height - crop_top).max(1);
    let y_size = stride.checked_mul(slice_height)?;

    let layouts: &[BufferYuvLayout] = match color_format {
        COLOR_FORMAT_YUV420_PLANAR => &[BufferYuvLayout::Planar],
        COLOR_FORMAT_YUV420_SEMI_PLANAR => &[BufferYuvLayout::SemiPlanar],
        // flexible / 厂商格式:先 NV12(最常见),长度不够再 I420。
        _ => &[BufferYuvLayout::SemiPlanar, BufferYuvLayout::Planar],
    };
    for &layout in layouts {
        match layout {
            BufferYuvLayout::Planar => {
                let chroma_stride = (stride + 1) / 2;
                let chroma_rows = (slice_height + 1) / 2;
                let chroma_size = chroma_stride.checked_mul(chroma_rows)?;
                let total = y_size.checked_add(chroma_size.checked_mul(2)?)?;
                if buffer.len() < total {
                    continue;
                }
                let y = &buffer[..y_size];
                let u = &buffer[y_size..y_size + chroma_size];
                let v = &buffer[y_size + chroma_size..total];
                let y_offset = crop_top * stride + crop_left;
                let chroma_offset = (crop_top / 2) * chroma_stride + crop_left / 2;
                yuv420_to_rgba(
                    crop_width,
                    crop_height,
                    y,
                    stride,
                    y_offset,
                    u,
                    chroma_stride,
                    1,
                    chroma_offset,
                    v,
                    chroma_stride,
                    1,
                    chroma_offset,
                    out,
                );
                return Some((crop_width, crop_height, layout));
            }
            BufferYuvLayout::SemiPlanar => {
                let uv_rows = (slice_height + 1) / 2;
                let uv_size = stride.checked_mul(uv_rows)?;
                let total = y_size.checked_add(uv_size)?;
                if buffer.len() < total {
                    continue;
                }
                let y = &buffer[..y_size];
                let uv = &buffer[y_size..total];
                let y_offset = crop_top * stride + crop_left;
                let u_offset = (crop_top / 2) * stride + (crop_left / 2) * 2;
                // NV12:U 在前、V 在后,交错;NV21 会把两者交换,颜色会偏,
                // 但 Codec2 的 flexible/NV12 路径通常是 U 先。
                yuv420_to_rgba(
                    crop_width,
                    crop_height,
                    y,
                    stride,
                    y_offset,
                    uv,
                    stride,
                    2,
                    u_offset,
                    uv,
                    stride,
                    2,
                    u_offset + 1,
                    out,
                );
                return Some((crop_width, crop_height, layout));
            }
        }
    }
    None
}

/// 把交错 PCM 降混为立体声(声音域 `MovieAudioInfo.channels` 恒为 2)。
///
/// - 单声道:复制到左右;
/// - 双声道:原样;
/// - 多声道(5.1/7.1 等):取前两声道(FL/FR),不引入额外相移。
///
/// 不足一帧的尾巴丢弃(解码器输出按整帧交付,正常不会出现)。
pub fn downmix_to_stereo(samples: &[i16], channels: usize, out: &mut Vec<i16>) {
    if channels == 0 {
        return;
    }
    let frames = samples.len() / channels;
    out.reserve(frames.saturating_mul(2));
    match channels {
        1 => {
            for &sample in &samples[..frames] {
                out.push(sample);
                out.push(sample);
            }
        }
        2 => out.extend_from_slice(&samples[..frames * 2]),
        _ => {
            for frame in samples.chunks_exact(channels).take(frames) {
                out.push(frame[0]);
                out.push(frame[1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba_at(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
        let o = (y * width + x) * 4;
        [rgba[o], rgba[o + 1], rgba[o + 2], rgba[o + 3]]
    }

    #[test]
    fn neutral_gray_maps_to_expected_bt601_values() {
        let y = [128u8; 4];
        let u = [128u8; 1];
        let v = [128u8; 1];
        let mut out = Vec::new();
        yuv420_to_rgba(2, 2, &y, 2, 0, &u, 1, 1, 0, &v, 1, 1, 0, &mut out);
        assert_eq!(out.len(), 2 * 2 * 4);
        for pixel in out.chunks_exact(4) {
            assert_eq!(pixel, [130, 130, 130, 255]);
        }
    }

    #[test]
    fn black_and_white_endpoints_clamp() {
        let mut out = Vec::new();
        let y = [16u8, 235u8, 16u8, 235u8];
        let u = [128u8; 1];
        let v = [128u8; 1];
        yuv420_to_rgba(2, 2, &y, 2, 0, &u, 1, 1, 0, &v, 1, 1, 0, &mut out);
        assert_eq!(rgba_at(&out, 2, 0, 0), [0, 0, 0, 255]);
        assert_eq!(rgba_at(&out, 2, 1, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn i420_and_nv12_are_equivalent() {
        // 4×4,UV 各一个 2×2 常量块;突出 stride 与 pixel_stride 的差异。
        let width = 4usize;
        let height = 4usize;
        let y: Vec<u8> = (0..width * height).map(|i| (32 + i * 7) as u8).collect();
        let u_plane = [96u8, 96, 96, 96];
        let v_plane = [160u8, 160, 160, 160];

        let mut i420 = Vec::new();
        yuv420_to_rgba(
            width, height, &y, width, 0, &u_plane, 2, 1, 0, &v_plane, 2, 1, 0, &mut i420,
        );

        // NV12:U/V 交错在同一个平面里(U 在前),pixel_stride = 2。
        let uv = [96u8, 160, 96, 160, 96, 160, 96, 160];
        let mut nv12 = Vec::new();
        yuv420_to_rgba(
            width, height, &y, width, 0, &uv, 4, 2, 0, &uv, 4, 2, 1, &mut nv12,
        );
        assert_eq!(i420, nv12);
    }

    #[test]
    fn nv21_swaps_u_v_without_changing_result_when_equal() {
        let y = [128u8; 4];
        let uv = [120u8, 136, 120, 136, 120, 136, 120, 136];
        let mut nv12 = Vec::new();
        yuv420_to_rgba(2, 2, &y, 2, 0, &uv, 2, 2, 0, &uv, 2, 2, 1, &mut nv12);
        let mut nv21 = Vec::new();
        yuv420_to_rgba(2, 2, &y, 2, 0, &uv, 2, 2, 1, &uv, 2, 2, 0, &mut nv21);
        // 交换 U/V 会改变色相:两条路径必须产出不同像素,但 alpha 都是 255。
        assert_eq!(nv12.len(), nv21.len());
        assert_ne!(nv12, nv21);
        assert!(nv12.chunks_exact(4).all(|p| p[3] == 255));
        assert!(nv21.chunks_exact(4).all(|p| p[3] == 255));
    }

    #[test]
    fn crop_offsets_use_the_requested_origin() {
        // 4×4,但只裁出右下 2×2;stride 仍按整行。
        let width = 2usize;
        let height = 2usize;
        let y: Vec<u8> = (0..16).map(|i| if i == 10 { 235 } else { 16 }).collect();
        let u = [128u8; 1];
        let v = [128u8; 1];
        let mut out = Vec::new();
        yuv420_to_rgba(
            width,
            height,
            &y,
            4,
            2 * 4 + 2,
            &u,
            1,
            1,
            0,
            &v,
            1,
            1,
            0,
            &mut out,
        );
        // 裁剪原点 (2,2) 的 Y 索引 = 10 → 白。
        assert_eq!(rgba_at(&out, 2, 0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn mono_is_duplicated_to_stereo() {
        let mut out = Vec::new();
        downmix_to_stereo(&[1, 2, 3], 1, &mut out);
        assert_eq!(out, vec![1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn stereo_passes_through() {
        let mut out = Vec::new();
        downmix_to_stereo(&[1, 2, 3, 4], 2, &mut out);
        assert_eq!(out, vec![1, 2, 3, 4]);
    }

    #[test]
    fn five_one_takes_front_left_and_right() {
        let mut out = Vec::new();
        downmix_to_stereo(&[10, 20, 30, 40, 50, 60], 6, &mut out);
        assert_eq!(out, vec![10, 20]);
    }

    #[test]
    fn zero_channels_is_a_no_op() {
        let mut out = Vec::new();
        downmix_to_stereo(&[1, 2, 3], 0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn bytebuffer_planar_matches_direct_conversion() {
        let width = 4usize;
        let height = 4usize;
        let y: Vec<u8> = (0..16).map(|i| (32 + i * 7) as u8).collect();
        let u = [96u8; 4];
        let v = [160u8; 4];
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&y);
        buffer.extend_from_slice(&u);
        buffer.extend_from_slice(&v);

        let mut direct = Vec::new();
        yuv420_to_rgba(
            width,
            height,
            &y,
            width,
            0,
            &u,
            2,
            1,
            0,
            &v,
            2,
            1,
            0,
            &mut direct,
        );
        let mut from_buffer = Vec::new();
        let (w, h, layout) = yuv420_buffer_to_rgba(
            &buffer,
            width,
            height,
            COLOR_FORMAT_YUV420_PLANAR,
            width,
            height,
            0,
            0,
            width,
            height,
            &mut from_buffer,
        )
        .expect("I420 buffer");
        assert_eq!((w, h, layout), (4, 4, BufferYuvLayout::Planar));
        assert_eq!(from_buffer, direct);
    }

    #[test]
    fn bytebuffer_semi_planar_matches_nv12() {
        let width = 4usize;
        let height = 4usize;
        let y: Vec<u8> = (0..16).map(|i| (32 + i * 7) as u8).collect();
        let uv = [96u8, 160, 96, 160, 96, 160, 96, 160];
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&y);
        buffer.extend_from_slice(&uv);

        let mut direct = Vec::new();
        yuv420_to_rgba(
            width,
            height,
            &y,
            width,
            0,
            &uv,
            width,
            2,
            0,
            &uv,
            width,
            2,
            1,
            &mut direct,
        );
        let mut from_buffer = Vec::new();
        let (_, _, layout) = yuv420_buffer_to_rgba(
            &buffer,
            width,
            height,
            COLOR_FORMAT_YUV420_SEMI_PLANAR,
            width,
            height,
            0,
            0,
            width,
            height,
            &mut from_buffer,
        )
        .expect("NV12 buffer");
        assert_eq!(layout, BufferYuvLayout::SemiPlanar);
        assert_eq!(from_buffer, direct);
    }

    #[test]
    fn bytebuffer_flexible_prefers_semi_planar() {
        let width = 4usize;
        let height = 4usize;
        let y = vec![128u8; 16];
        let uv = vec![128u8; 8];
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&y);
        buffer.extend_from_slice(&uv);
        let mut out = Vec::new();
        let (_, _, layout) = yuv420_buffer_to_rgba(
            &buffer,
            width,
            height,
            COLOR_FORMAT_YUV420_FLEXIBLE,
            width,
            height,
            0,
            0,
            width,
            height,
            &mut out,
        )
        .expect("flexible buffer");
        assert_eq!(layout, BufferYuvLayout::SemiPlanar);
    }

    #[test]
    fn bytebuffer_crop_uses_output_format_rect() {
        // 6×4 分配(stride=6),显示 4×4;裁出右下 2×2。
        let stride = 6usize;
        let height = 4usize;
        let mut y = vec![16u8; stride * height];
        // 裁出区域的第一个像素 (2,2) = 白。
        y[2 * stride + 2] = 235;
        let uv = vec![128u8; stride * ((height + 1) / 2)];
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&y);
        buffer.extend_from_slice(&uv);
        let mut out = Vec::new();
        let (w, h, _) = yuv420_buffer_to_rgba(
            &buffer,
            4,
            4,
            COLOR_FORMAT_YUV420_SEMI_PLANAR,
            stride,
            height,
            2,
            2,
            2,
            2,
            &mut out,
        )
        .expect("cropped buffer");
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba_at(&out, 2, 0, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn bytebuffer_short_buffer_returns_none() {
        let mut out = Vec::new();
        assert!(yuv420_buffer_to_rgba(
            &[0u8; 8],
            4,
            4,
            COLOR_FORMAT_YUV420_SEMI_PLANAR,
            4,
            4,
            0,
            0,
            4,
            4,
            &mut out,
        )
        .is_none());
    }
}
