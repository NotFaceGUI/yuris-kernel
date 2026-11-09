//! GPU 纹理 → 读回缓冲 → RGBA8 [`Surface`]。
//!
//! `copy_texture_to_buffer` 的 `bytes_per_row` 必须是
//! [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`](256)的倍数,而输出表面是紧凑行;
//! 本模块负责去掉行尾填充。读回缓冲按尺寸缓存,同尺寸连续帧不重建。
//!
//! # 两种读法
//!
//! - [`ReadbackCache::read`](主帧路径):**非阻塞**。本帧只提交拷贝并挂
//!   `map_async`,**绝不等 GPU**;能收上一帧结果就更新缓存,收不到就沿用
//!   上一次成功的结果(表面允许滞后一两帧)。上一帧的映射还没完成时
//!   **本帧干脆不提交读回**(缓冲仍 mapped,不能往里写),只提交绘制命令。
//!
//!   旧实现在每帧开头 `poll(wait_indefinitely)`:只要 GPU 队列里还有活
//!   (立绘换差分帧:71MB 纹理拷贝 + 数十 MB 上传),CPU 就被按在帧里等
//!   整条队列跑完 —— 实测 <game-b> 34fps→52fps、p95 60ms→18ms、
//!   render max 182ms→64ms。桌面呈现本来就靠 `screen_texture`,CPU 面
//!   只服务存档抓帧,迟一两帧完全不影响观感。
//! - [`ReadbackCache::read_now`](精确读):同步等待当帧结果。只在
//!   `--shot` 末帧 / 用例 golden / 存档抓帧这类低频路径使用。
//!
//! 这是**显示用**的读回(一次一帧),不是渲染热路径里的隐式同步。

use std::sync::mpsc;

use yu_log::{Boundary, Diag};
use yu_render_core::Surface;

use crate::device::{code, GpuContext};

/// 读回缓冲缓存(单缓冲 + 非阻塞;忙时跳过该帧)。
pub struct ReadbackCache {
    buffer: Option<wgpu::Buffer>,
    size: u64,
    /// 上次拷贝的行列参数(collect 去填充时用)。
    layout: Option<(u32, u32, u64)>,
    /// 已挂 `map_async`、等回调的结果通道。
    receiver: Option<mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    /// 最近一次成功收走的表面(非阻塞路径的兜底返回)。
    last: Option<Surface>,
}

impl ReadbackCache {
    pub fn new() -> Self {
        Self {
            buffer: None,
            size: 0,
            layout: None,
            receiver: None,
            last: None,
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
            let buffer = ctx.checked_oom(&format!("readback buffer {size}B"), || {
                ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("yu-backend-wgpu readback"),
                    size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })?;
            self.buffer = Some(buffer);
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
        Self::record_copy_region(encoder, texture, buffer, 0, 0, width, height, padded_row)
    }

    /// 只拷贝纹理的一个子矩形(`origin_x/y` 起,`width × height` 大)。
    ///
    /// `padded_row` 对应**子矩形**的行跨度(调用方已按 256 对齐)。
    /// 用于 `TRIM` 这类只需要边缘 alpha 的场景:整图 1100×825 读回在
    /// 手机 Vulkan 上要 100~150ms,而 `trim_box` 从外向内扫,通常几圈就停。
    #[allow(clippy::too_many_arguments)]
    fn record_copy_region(
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        buffer: &wgpu::Buffer,
        origin_x: u32,
        origin_y: u32,
        width: u32,
        height: u32,
        padded_row: u64,
    ) -> Result<(), Diag> {
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin_x,
                    y: origin_y,
                    z: 0,
                },
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

    /// 同步读回纹理的**子矩形**(紧凑 RGBA8,行已去填充)。
    ///
    /// 与 [`Self::read_now`] 同语义(阻塞到本帧命令跑完),只是量更小。
    /// `TRIM` 的包围盒只需要外圈 alpha;整图读回在移动端 TBDR 上是
    /// 「把全部 tile 刷回主存」,代价与整屏尺寸成正比 —— 读 4 条边
    /// 只要几十 KB。
    pub fn read_region_now(
        &mut self,
        ctx: &GpuContext,
        texture: &wgpu::Texture,
        origin: (u32, u32),
        width: u32,
        height: u32,
    ) -> Result<Surface, Diag> {
        if width == 0 || height == 0 {
            return Err(Diag::fatal(code::READBACK, Boundary::Platform)
                .with_detail(format!("区域读回尺寸为 {width}x{height}")));
        }
        let (padded_row, size) = Self::row_layout(width, height)?;
        // 独立缓冲:区域读回的尺寸与主路径不同,共用会反复重建。
        let buffer = ctx.checked_oom(&format!("region readback {size}B"), || {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("yu-backend-wgpu region readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        })?;
        let mut encoder =
            ctx.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("yu-backend-wgpu region readback"),
                });
        Self::record_copy_region(
            &mut encoder,
            texture,
            &buffer,
            origin.0,
            origin.1,
            width,
            height,
            padded_row,
        )?;
        ctx.queue.submit(std::iter::once(encoder.finish()));
        {
            let slice = buffer.slice(..);
            let (sender, receiver) = mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
            ctx.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|err| {
                    Diag::fatal(code::READBACK, Boundary::Platform)
                        .with_detail(format!("等待区域读回失败: {err}"))
                })?;
            receiver
                .recv()
                .map_err(|_| {
                    Diag::fatal(code::READBACK, Boundary::Platform)
                        .with_detail("区域读回映射回调丢失")
                })?
                .map_err(|err| {
                    Diag::fatal(code::READBACK, Boundary::Platform)
                        .with_detail(format!("区域读回缓冲映射失败: {err}"))
                })?;
        }
        let mut rgba = Vec::with_capacity((u64::from(width) * 4 * u64::from(height)) as usize);
        {
            let slice = buffer.slice(..);
            let mapped = slice.get_mapped_range().map_err(|err| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("区域读回缓冲范围无效: {err}"))
            })?;
            let unpadded_row = u64::from(width) * 4;
            for row in 0..height as usize {
                let start = row * padded_row as usize;
                let end = start + unpadded_row as usize;
                rgba.extend_from_slice(&mapped[start..end]);
            }
            drop(mapped);
        }
        buffer.unmap();
        Ok(Surface {
            width,
            height,
            rgba,
        })
    }

    /// 主帧路径:**不等待 GPU**。
    ///
    /// 1. 先非阻塞收上一帧(能收就更新 `last`);
    /// 2. 上一帧还没收回(缓冲仍 mapped)⇒ 本帧不提交读回,只提交绘制命令,
    ///    返回上一次成功结果;
    /// 3. 否则提交本帧拷贝 + 挂 `map_async`,同样不等待。
    ///
    /// `encoder` 含本帧全部绘制命令,两条分支都必须 submit。
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
        if self.receiver.is_some() {
            self.try_collect(ctx)?;
        }
        if self.receiver.is_some() {
            // 缓冲仍处于映射流程:本帧跳过读回(绝不往 mapped 缓冲写)。
            ctx.queue.submit(Some(encoder.finish()));
            return Ok(self.fallback(width, height));
        }
        let (padded_row, size) = Self::row_layout(width, height)?;
        {
            let buffer = self.ensure_buffer(ctx, size)?;
            Self::record_copy(&mut encoder, texture, buffer, width, height, padded_row)?;
        }
        self.layout = Some((width, height, padded_row));
        ctx.queue.submit(Some(encoder.finish()));
        if let Some(buffer) = self.buffer.as_ref() {
            let slice = buffer.slice(..);
            let (sender, receiver) = mpsc::channel();
            // 先挂 map_async 再 poll:反过来在部分后端上会立即映射,
            // 导致 submit 时缓冲仍处于 mapped 状态(验证错误)。
            slice.map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
            self.receiver = Some(receiver);
        }
        Ok(self.fallback(width, height))
    }

    /// 精确读:同步等待**本帧**拷贝结果(`--shot` 末帧/存档抓帧)。
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
        // 先把挂着的上一帧收掉(否则缓冲仍 mapped,不能写入)。
        if self.receiver.is_some() {
            self.collect_blocking(ctx)?;
        }
        let (padded_row, size) = Self::row_layout(width, height)?;
        let buffer = self.ensure_buffer(ctx, size)?;
        Self::record_copy(&mut encoder, texture, buffer, width, height, padded_row)?;
        self.layout = Some((width, height, padded_row));
        ctx.queue.submit(Some(encoder.finish()));
        self.collect_blocking(ctx)?.ok_or_else(|| {
            Diag::fatal(code::READBACK, Boundary::Platform).with_detail("精确读回缺缓冲布局")
        })
    }

    fn fallback(&self, width: u32, height: u32) -> Surface {
        self.last
            .clone()
            .unwrap_or_else(|| Surface::new(width, height, [0, 0, 0, 255]))
    }

    /// 非阻塞收:`poll(Poll)` 一次,完成则去填充后放进 `last`。
    fn try_collect(&mut self, ctx: &GpuContext) -> Result<(), Diag> {
        if ctx.device.poll(wgpu::PollType::Poll).is_err() {
            return Ok(());
        }
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(());
        };
        match receiver.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                self.receiver = None;
                return Err(Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("读回缓冲映射失败: {err}")));
            }
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                return Err(
                    Diag::fatal(code::READBACK, Boundary::Platform).with_detail("读回映射回调丢失")
                );
            }
        }
        self.receiver = None;
        if let Some(surface) = self.take_mapped()? {
            self.last = Some(surface);
        }
        Ok(())
    }

    /// 把已映射的缓冲取出来(去行填充 + unmap)。
    fn take_mapped(&mut self) -> Result<Option<Surface>, Diag> {
        let (Some(buffer), Some((width, height, padded_row))) = (self.buffer.as_ref(), self.layout)
        else {
            return Ok(None);
        };
        let surface = {
            let slice = buffer.slice(..);
            let mapped = slice.get_mapped_range().map_err(|err| {
                Diag::fatal(code::READBACK, Boundary::Platform)
                    .with_detail(format!("读回缓冲范围无效: {err}"))
            })?;
            let unpadded_row = u64::from(width) * 4;
            let mut rgba = Vec::with_capacity((unpadded_row * u64::from(height)) as usize);
            for row in 0..height as usize {
                let start = row * padded_row as usize;
                let end = start + unpadded_row as usize;
                rgba.extend_from_slice(&mapped[start..end]);
            }
            drop(mapped);
            Surface {
                width,
                height,
                rgba,
            }
        };
        buffer.unmap();
        Ok(Some(surface))
    }

    /// 阻塞收(仅 `read_now` / 精确路径用)。
    fn collect_blocking(&mut self, ctx: &GpuContext) -> Result<Option<Surface>, Diag> {
        if self.buffer.is_none() || self.layout.is_none() {
            return Ok(None);
        }
        // 没有挂过映射(精确路径)⇒ 现在挂,再等。
        if self.receiver.is_none() {
            if let Some(buffer) = self.buffer.as_ref() {
                let slice = buffer.slice(..);
                let (sender, receiver) = mpsc::channel();
                slice.map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
                self.receiver = Some(receiver);
            }
        }
        self.poll_wait(ctx)?;
        if let Some(receiver) = self.receiver.as_ref() {
            receiver
                .recv()
                .map_err(|_| {
                    Diag::fatal(code::READBACK, Boundary::Platform).with_detail("读回映射回调丢失")
                })?
                .map_err(|err| {
                    Diag::fatal(code::READBACK, Boundary::Platform)
                        .with_detail(format!("读回缓冲映射失败: {err}"))
                })?;
            self.receiver = None;
        }
        self.take_mapped()
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

/// 一次性同步读回纹理的一个子矩形。
///
/// 与 [`read_texture`] 同定位(低频路径),但只拷 `width × height` 的子矩形。
/// `TRIM` 的包围盒扫描用它:整图读回在移动端 TBDR 上是「全部 tile 刷回
/// 主存」,真机 1100×825 一次 100~151ms;而 `trim_box` 从外向内扫、
/// 通常几行/几列就停,每次读的窄带只有几十 KB。
pub fn read_region_now_cached(
    ctx: &GpuContext,
    texture: &wgpu::Texture,
    origin: (u32, u32),
    width: u32,
    height: u32,
) -> Result<Surface, Diag> {
    let mut cache = ReadbackCache::new();
    cache.read_region_now(ctx, texture, origin, width, height)
}

/// 一次性同步读回一张纹理(`TRIM`/文本基面/存档抓帧等低频路径)。
///
/// 独立缓冲 + 独立 encoder:不与流式读回共用(`collect` 会互踩)。调用方
/// 负责把这次读回计入 `RenderPhases::readback_bytes`(不允许静默)。
pub fn read_texture(
    ctx: &GpuContext,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Surface, Diag> {
    let mut cache = ReadbackCache::new();
    let encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yu-backend-wgpu read texture"),
        });
    cache.read_now(ctx, texture, width, height, encoder)
}

/// `TRIM` 的同步读回:只读四边窄带,不再整图回刷。
///
/// 真机 TBDR 上整图读回(1100×825)实测 100~291ms;`TRIM` 只需要外缘
/// 透明行/列,按 [`TRIM_BAND`] 逐带外扫即可。判据与 soft
/// `apply_trim` 完全一致(alpha 全 0 的行/列)。
pub fn trim_bounds(
    ctx: &GpuContext,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    mask: i64,
) -> Result<Option<(u32, u32, u32, u32)>, Diag> {
    trim_bounds_with(width, height, mask, |x, y, w, h| {
        read_region_now_cached(ctx, texture, (x, y), w, h)
    })
}

/// `TRIM` 扫描起始带宽(像素)。够小以少读,够大以避免逐行提交。
const TRIM_BAND: u32 = 64;
/// 连续空带时带宽翻倍到此上限:大留白图不会因为 64px 一带而产生几十次
/// `submit + map` 同步等待;靠近内容的那一带仍然只读窄条。
const TRIM_BAND_MAX: u32 = 512;

/// 与 [`trim_bounds`] 同逻辑;`read` 注入读回实现,CPU 单测用。
fn trim_bounds_with<F>(
    width: u32,
    height: u32,
    mask: i64,
    mut read: F,
) -> Result<Option<(u32, u32, u32, u32)>, Diag>
where
    F: FnMut(u32, u32, u32, u32) -> Result<Surface, Diag>,
{
    if width == 0 || height == 0 {
        return Ok(None);
    }
    if mask & 0xF == 0 {
        return Ok(Some((0, 0, width - 1, height - 1)));
    }
    let row_empty = |surface: &Surface, row: u32| -> bool {
        let stride = surface.width as usize;
        (0..surface.width).all(|x| surface.rgba[(row as usize * stride + x as usize) * 4 + 3] == 0)
    };
    let col_empty = |surface: &Surface, col: u32| -> bool {
        let stride = surface.width as usize;
        (0..surface.height).all(|y| {
            surface.rgba[((y as usize) * stride + col as usize) * 4 + 3] == 0
        })
    };

    let mut top = 0u32;
    if mask & 8 != 0 {
        let mut y = 0u32;
        let mut band = TRIM_BAND;
        'top: loop {
            let band_h = (height - y).min(band);
            let surface = read(0, y, width, band_h)?;
            for row in 0..band_h {
                if !row_empty(&surface, row) {
                    top = y + row;
                    break 'top;
                }
            }
            y += band_h;
            band = band.saturating_mul(2).min(TRIM_BAND_MAX);
            if y >= height {
                return Ok(None);
            }
        }
    }

    let mut bottom = height - 1;
    if mask & 4 != 0 {
        let mut end = height;
        let mut band = TRIM_BAND;
        'bottom: loop {
            let start = end.saturating_sub(band);
            let band_h = end - start;
            let surface = read(0, start, width, band_h)?;
            for row in (0..band_h).rev() {
                if !row_empty(&surface, row) {
                    bottom = start + row;
                    break 'bottom;
                }
            }
            end = start;
            band = band.saturating_mul(2).min(TRIM_BAND_MAX);
            if end == 0 {
                return Ok(None);
            }
        }
    }
    if top > bottom {
        return Ok(None);
    }

    let rows = bottom - top + 1;
    let mut left = 0u32;
    if mask & 2 != 0 {
        let mut x = 0u32;
        let mut band = TRIM_BAND;
        'left: loop {
            let band_w = (width - x).min(band);
            let surface = read(x, top, band_w, rows)?;
            for col in 0..band_w {
                if !col_empty(&surface, col) {
                    left = x + col;
                    break 'left;
                }
            }
            x += band_w;
            band = band.saturating_mul(2).min(TRIM_BAND_MAX);
            if x >= width {
                return Ok(None);
            }
        }
    }

    let mut right = width - 1;
    if mask & 1 != 0 {
        let mut end = width;
        let mut band = TRIM_BAND;
        'right: loop {
            let start = end.saturating_sub(band);
            let band_w = end - start;
            let surface = read(start, top, band_w, rows)?;
            for col in (0..band_w).rev() {
                if !col_empty(&surface, col) {
                    right = start + col;
                    break 'right;
                }
            }
            end = start;
            band = band.saturating_mul(2).min(TRIM_BAND_MAX);
            if end == 0 {
                return Ok(None);
            }
        }
    }
    if left > right {
        return Ok(None);
    }
    Ok(Some((left, top, right, bottom)))
}

/// 调试用:把一张纹理读成紧凑 RGBA8(低频人工取证)。
///
/// 低频诊断路径,允许同步等待。
pub fn dump_texture(
    ctx: &GpuContext,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Diag> {
    let mut cache = ReadbackCache::new();
    let encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("yu-backend-wgpu dump texture"),
        });
    let surface = cache.read_now(ctx, texture, width, height, encoder)?;
    Ok(surface.rgba)
}

impl Default for ReadbackCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_layout_pads_to_256() {
        let (padded, size) = ReadbackCache::row_layout(100, 10).expect("layout");
        assert_eq!(padded, 512);
        assert_eq!(size, 5120);
    }

    fn opaque_rect(width: u32, height: u32, rect: (u32, u32, u32, u32)) -> Vec<u8> {
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        let (x0, y0, x1, y1) = rect;
        for y in y0..=y1 {
            for x in x0..=x1 {
                rgba[((y as usize) * (width as usize) + x as usize) * 4 + 3] = 255;
            }
        }
        rgba
    }

    fn trim_of(
        width: u32,
        height: u32,
        rgba: &[u8],
        mask: i64,
    ) -> Option<(u32, u32, u32, u32)> {
        trim_bounds_with(width, height, mask, |x, y, w, h| {
            let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
            for row in y..y + h {
                let start = ((row as usize) * (width as usize) + x as usize) * 4;
                out.extend_from_slice(&rgba[start..start + (w as usize) * 4]);
            }
            Ok(Surface {
                width: w,
                height: h,
                rgba: out,
            })
        })
        .expect("trim")
    }

    #[test]
    fn banded_trim_matches_alpha_bounds() {
        // 单带内。
        let rgba = opaque_rect(10, 9, (3, 2, 6, 5));
        assert_eq!(trim_of(10, 9, &rgba, 15), Some((3, 2, 6, 5)));
        // 只裁上边:左右/下保持原尺寸。
        assert_eq!(trim_of(10, 9, &rgba, 8), Some((0, 2, 9, 8)));
        // 只裁右边:上/左/下保持原尺寸。
        assert_eq!(trim_of(10, 9, &rgba, 1), Some((0, 0, 6, 8)));
        // 全透明:保持原样(None 由调用方处理为不裁剪)。
        let empty = vec![0u8; 10 * 9 * 4];
        assert_eq!(trim_of(10, 9, &empty, 15), None);
        // 内容贴边:裁掉 0 像素。
        let full = opaque_rect(10, 9, (0, 0, 9, 8));
        assert_eq!(trim_of(10, 9, &full, 15), Some((0, 0, 9, 8)));
    }

    #[test]
    fn banded_trim_crosses_multiple_bands() {
        // 200×200,内容 60×60 居中偏右下:四个方向都要跨过 64px 带。
        let rgba = opaque_rect(200, 200, (70, 80, 129, 139));
        assert_eq!(trim_of(200, 200, &rgba, 15), Some((70, 80, 129, 139)));
    }
}
