//! GPU 纹理 → 读回缓冲 → RGBA8 [`Surface`]。
//!
//! `copy_texture_to_buffer` 的 `bytes_per_row` 必须是
//! [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`](256)的倍数,而输出表面是紧凑行;
//! 本模块负责去掉行尾填充。读回缓冲按尺寸缓存,同尺寸连续帧不重建。
//!
//! # 两种读法
//!
//! - [`ReadbackCache::read`](主帧路径):**异步一帧滞后**。本帧只提交拷贝,
//!   取回的是**上一帧**已完成的拷贝 —— GPU 在上一帧 submit 之后有整整一帧
//!   (宿主呈现 + vsync 空闲)去完成它,`poll` 立即返回,不再把 CPU/GPU
//!   流水线拍死同步等待(旧实现每帧 `poll(wait_indefinitely)`,负载大时
//!   帧内所有上传/绘制的队列尾部都被读回串行化)。滞后一帧与 `COPY :/W0/`
//!   的「上一帧呈现内容」语义**完全一致**(旧实现的后缓冲本来就是上一帧
//!   帧末读回的那份)。
//! - [`ReadbackCache::read_now`](精确读):旧同步路径,当帧出像素。
//!   只在 `--shot` 末帧 / 对账等低频场景使用。
//!
//! 这是**显示用**的读回(一次一帧),不是渲染热路径里的隐式同步:
//! 每帧只在最后读屏幕目标一次。

use std::sync::mpsc;

use yu_render_core::Surface;
use yu_log::{Boundary, Diag};

use crate::device::{code, GpuContext};

/// 读回缓冲缓存(单缓冲:collect 发生在下一帧 copy 提交之前,可交替复用)。
pub struct ReadbackCache {
    buffer: Option<wgpu::Buffer>,
    size: u64,
    /// 上次拷贝的行列参数(collect 去填充时用)。
    layout: Option<(u32, u32, u64)>,
}

impl ReadbackCache {
    pub fn new() -> Self {
        Self {
            buffer: None,
            size: 0,
            layout: None,
        }
    }

    fn row_layout(width: u32, height: u32) -> Result<(u64, u64), Diag> {
        let unpadded_row = u64::from(width) * 4;
        let alignment = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let padded_row = unpadded_row.div_ceil(alignment) * alignment;
        let size = padded_row.checked_mul(u64::from(height)).ok_or_else(|| {
            Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("读回缓冲 {width}x{height} 尺寸溢出"))
        })?;
        Ok((padded_row, size))
    }

    fn ensure_buffer(&mut self, ctx: &GpuContext, size: u64) -> Result<&wgpu::Buffer, Diag> {
        if self.size != size || self.buffer.is_none() {
            self.buffer = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-backend-wgpu readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.size = size;
        }
        Ok(self.buffer.as_ref().expect("buffer created above"))
    }

    fn record_copy(
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        buffer: &wgpu::Buffer,
        width: u32,
        height: u32,
        padded_row: u64,
    ) -> Result<(), Diag> {
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
        Ok(())
    }

    /// 主帧路径:提交本帧拷贝,取回**上一帧**结果(异步一帧滞后;
    /// 首帧无上一帧时退回同步读本帧,仅发生一次)。
    ///
    /// `encoder` 含本帧全部绘制命令,在这里统一 submit。
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
        // 1) 取上一帧已完成拷贝:自上次 submit 以来 GPU 已跑过一整帧,
        //    map+poll 立即返回,不构成流水线同步点。
        let previous = self.collect(ctx)?;
        // 2) 本帧拷贝入队(不等待)。
        let (padded_row, size) = Self::row_layout(width, height)?;
        {
            let buffer = self.ensure_buffer(ctx, size)?;
            Self::record_copy(&mut encoder, texture, buffer, width, height, padded_row)?;
        }
        self.layout = Some((width, height, padded_row));
        ctx.queue.submit(Some(encoder.finish()));
        // 3) 有上一帧 = 异步成功;首帧退回同步收本帧。
        match previous {
            Some(surface) => Ok(surface),
            None => self.collect(ctx)?.ok_or_else(|| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail("首帧读回缺缓冲布局")
            }),
        }
    }

    /// 精确读:同步等待**本帧**拷贝结果(`--shot` 末帧/低频对账)。
    pub fn read_now(
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
        let (padded_row, size) = Self::row_layout(width, height)?;
        let buffer = self.ensure_buffer(ctx, size)?;
        Self::record_copy(&mut encoder, texture, buffer, width, height, padded_row)?;
        self.layout = Some((width, height, padded_row));
        ctx.queue.submit(Some(encoder.finish()));
        self.collect(ctx)?.ok_or_else(|| {
            Diag::fatal(code::READBACK, Boundary::Platform).with_detail("精确读回缺缓冲布局")
        })
    }

    /// 收走当前挂着的拷贝(map → 等 → 去行填充 → unmap)。
    fn collect(&mut self, ctx: &GpuContext) -> Result<Option<Surface>, Diag> {
        let Some(buffer) = self.buffer.as_ref() else {
            return Ok(None);
        };
        let Some((width, height, padded_row)) = self.layout else {
            return Ok(None);
        };
        let slice = buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        // 先挂 map_async 再 poll:反过来在部分后端上会立即映射,
        // 导致 submit 时缓冲仍处于 mapped 状态(验证错误)。
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.poll_wait(ctx)?;
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
            Diag::fatal(code::READBACK, Boundary::Platform).with_detail(format!("读回缓冲范围无效: {err}"))
        })?;
        let unpadded_row = u64::from(width) * 4;
        let mut rgba = Vec::with_capacity((unpadded_row * u64::from(height)) as usize);
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            let end = start + unpadded_row as usize;
            rgba.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(Some(Surface {
            width,
            height,
            rgba,
        }))
    }

    fn poll_wait(&self, ctx: &GpuContext) -> Result<(), Diag> {
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|err| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("等待 GPU 读回失败: {err}"))
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
            assert_eq!(padded, expected_padded);
        }
    }
}

/// 取证用:把一张 GPU 纹理读成 RGBA8(同步,`YU_TRANS_DUMP` 专用)。
///
/// 只在环境变量开启时由转场规划调用;不做缓存、不管行尾填充之外的优化。
pub fn dump_texture(
    ctx: &GpuContext,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Diag> {
    let (padded_row, size) = {
        let unpadded = u64::from(width) * 4;
        let align = u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let padded = unpadded.div_ceil(align) * align;
        (padded, padded * u64::from(height))
    };
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("yu-backend-wgpu dump"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("dump") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u32::try_from(padded_row).unwrap_or(u32::MAX)),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|err| {
            Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("dump 等待失败: {err}"))
        })?;
    receiver
        .recv()
        .map_err(|_| {
            Diag::fatal(code::READBACK, Boundary::Platform).with_detail("dump 映射回调丢失")
        })?
        .map_err(|err| {
            Diag::fatal(code::READBACK, Boundary::Platform).with_detail(format!("dump 映射失败: {err}"))
        })?;
    let mapped = slice.get_mapped_range().map_err(|err| {
        Diag::fatal(code::READBACK, Boundary::Platform).with_detail(format!("dump 范围无效: {err}"))
    })?;
    let unpadded = (u64::from(width) * 4) as usize;
    let mut rgba = Vec::with_capacity(unpadded * height as usize);
    for row in 0..height as usize {
        let start = row * padded_row as usize;
        rgba.extend_from_slice(&mapped[start..start + unpadded]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(rgba)
}
