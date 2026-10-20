//! GPU 纹理 → 读回缓冲 → RGBA8 [`Surface`]。
//!
//! `copy_texture_to_buffer` 的 `bytes_per_row` 必须是
//! [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`](256)的倍数,而输出表面是紧凑行;
//! 本模块负责去掉行尾填充。读回缓冲按尺寸缓存,同尺寸连续帧不重建。
//!
//! 这是**显示用**的同步读回(一次一帧),不是渲染热路径里的隐式同步:
//! 每帧只在最后读屏幕目标一次。

use std::sync::mpsc;

use yu_backend_soft::Surface;
use yu_log::{Boundary, Diag};

use crate::device::{code, GpuContext};

/// 读回缓冲缓存。
pub struct ReadbackCache {
    buffer: Option<wgpu::Buffer>,
    size: u64,
}

impl ReadbackCache {
    pub fn new() -> Self {
        Self {
            buffer: None,
            size: 0,
        }
    }

    /// 把 `texture`(Rgba8Unorm,`width × height`)读成紧凑 RGBA8。
    pub fn read(
        &mut self,
        ctx: &GpuContext,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        mut encoder: wgpu::CommandEncoder,
    ) -> Result<Surface, Diag> {
        if width == 0 || height == 0 {
            return Err(Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("读回尺寸为 {width}x{height}")));
        }
        let unpadded_row = u64::from(width) * 4;
        let alignment = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let padded_row = unpadded_row.div_ceil(alignment) * alignment;
        let size = padded_row.checked_mul(u64::from(height)).ok_or_else(|| {
            Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("读回缓冲 {width}x{height} 尺寸溢出"))
        })?;

        if self.size != size || self.buffer.is_none() {
            self.buffer = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-backend-wgpu readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.size = size;
        }
        let buffer = self.buffer.as_ref().expect("readback buffer created above");

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(u32::try_from(padded_row).map_err(|_| {
                        Diag::fatal(code::READBACK, Boundary::Platform)
                            .with_detail("读回行跨度超过 u32")
                    })?),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let slice = buffer.slice(..);
        let (sender, receiver) = mpsc::channel();

        ctx.queue.submit(Some(encoder.finish()));
        // 先把 copy 提交,再挂 map_async;反过来在部分后端上会立即映射,
        // 导致 submit 时缓冲仍处于 mapped 状态(验证错误)。
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|err| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("等待 GPU 读回失败: {err}"))
            })?;

        receiver
            .recv()
            .map_err(|_| {
                Diag::fatal(code::READBACK, Boundary::Platform).with_detail("读回映射回调丢失")
            })?
            .map_err(|err| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("读回缓冲映射失败: {err}"))
            })?;

        let mapped = slice.get_mapped_range().map_err(|err| {
            Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("读回缓冲范围无效: {err}"))
        })?;
        let mut rgba = Vec::with_capacity((unpadded_row * u64::from(height)) as usize);
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            let end = start + unpadded_row as usize;
            rgba.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        buffer.unmap();

        Ok(Surface {
            width,
            height,
            rgba,
        })
    }
}

impl Default for ReadbackCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn row_alignment_math() {
        for (width, expected_padded) in
            [(1u32, 256u64), (3, 256), (64, 256), (65, 512), (200, 1024)]
        {
            let unpadded = u64::from(width) * 4;
            let padded = unpadded.div_ceil(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
                * u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            assert_eq!(padded, expected_padded, "width {width}");
        }
    }
}
