//! PNG 解码:官方 libpng(C 库)。
//!
//! # 为什么用 libpng 而不是纯 Rust 实现
//!
//! 差分图(RGBA8, bpp=4)的反滤波是整个解码里最贵的一段,而 `Paeth` /
//! `Average` 有**逐字节的数据依赖**(每个字节依赖左邻居的已还原值),
//! 纯 Rust 循环在 arm64 上无法向量化 —— LLVM 只对 x86_64 的这类模式
//! 做自动向量化。
//!
//! libpng 自带手写 NEON 实现(`vendor/arm/filter_neon_intrinsics.c`):
//! `png_read_filter_row_{up,sub3,sub4,avg3,avg4,paeth3,paeth4}_neon`,
//! 运行期由 `png_init_filter_functions_neon` 按 CPU 能力挂到函数指针上。
//! `libpng-sys` 的 `build_static` 在 `CARGO_CFG_TARGET_ARCH` 为
//! `arm`/`aarch64` 时自动编译这些文件。
//!
//! # 覆盖面
//!
//! 用 `png_read_png` + 一组 transform 让 libpng 统一吐出 **8 位 RGBA**:
//! 全部位深(1/2/4/8/16)、全部颜色类型(灰度/RGB/调色板/灰度+α/RGBA)、
//! Adam7 隔行、`tRNS` 透明、`PLTE` 调色板都由库内部处理。
//!
//! # 回退
//!
//! 任何失败都返回 `Err`,调用方回落 `image` crate。成功路径与
//! `image::load_from_memory(..).into_rgba8()` **逐字节相同**
//! (有全量语料对拍用例)。

use std::ffi::{c_char, c_int, CStr};

use crate::texture::Texture;
use libpng_sys::ffi;

/// 错误信息收集器(`png_set_error_fn` 的 error_ptr)。
///
/// libpng 的错误路径是 `longjmp`,不是返回码;把消息写到这块缓冲后由调用方
/// 在 `setjmp` 之外读取。
struct ErrBuf {
    text: [u8; 256],
    len: usize,
}

impl ErrBuf {
    fn new() -> Self {
        Self {
            text: [0; 256],
            len: 0,
        }
    }

    fn as_str(&self) -> String {
        if self.len == 0 {
            return "未知错误".to_string();
        }
        String::from_utf8_lossy(&self.text[..self.len]).into_owned()
    }
}

/// libpng 错误回调:写入 `ErrBuf` 后不返回(longjmp 由 `setjmp` 接管)。
unsafe extern "C" fn on_error(_png: *mut ffi::png_struct_def, msg: *const c_char) {
    let ctx = unsafe { ffi::png_get_error_ptr(&*_png) }.cast::<ErrBuf>();
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &mut *ctx };
    if msg.is_null() {
        return;
    }
    let text = unsafe { CStr::from_ptr(msg) }.to_bytes();
    let n = text.len().min(ctx.text.len());
    ctx.text[..n].copy_from_slice(&text[..n]);
    ctx.len = n;
}

unsafe extern "C" fn on_warning(_png: *mut ffi::png_struct_def, _msg: *const c_char) {}

/// 从内存读数据的回调状态。
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

unsafe extern "C" fn read_data(png: *mut ffi::png_struct_def, out: *mut u8, len: usize) {
    let ctx = unsafe { ffi::png_get_io_ptr(&*png) }.cast::<Reader<'_>>();
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &mut *ctx };
    if ctx.pos + len > ctx.bytes.len() {
        // 数据不足:交给 libpng 的 error 路径(longjmp)。
        unsafe {
            ffi::png_error(&mut *png, c"读取越界".as_ptr());
        }
        return;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(ctx.bytes.as_ptr().add(ctx.pos), out, len);
    }
    ctx.pos += len;
}

/// 解一张 PNG 到 RGBA8。
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut err = ErrBuf::new();
    let mut reader = Reader { bytes, pos: 0 };

    // SAFETY: 全部指针在本次调用内有效;错误路径由 libpng 的 longjmp 处理,
    // 我们在每个可能跳转的调用之后检查返回值。`png_read_png` 内部完成
    // 分配与读取,返回后 `info_ptr->row_pointers` 持有像素。
    unsafe {
        let png_ptr = ffi::png_create_read_struct(
            ffi::PNG_LIBPNG_VER_STRING.cast(),
            (&mut err as *mut ErrBuf).cast(),
            Some(on_error),
            Some(on_warning),
        );
        if png_ptr.is_null() {
            return Err("png_create_read_struct 失败".to_string());
        }
        let info_ptr = ffi::png_create_info_struct(&*png_ptr);
        if info_ptr.is_null() {
            ffi::png_destroy_read_struct(&mut (png_ptr as *mut _), std::ptr::null_mut(), std::ptr::null_mut());
            return Err("png_create_info_struct 失败".to_string());
        }

        ffi::png_set_read_fn(&mut *png_ptr, (&mut reader as *mut Reader<'_>).cast(), Some(read_data));

        // 目标格式统一为 **8 位 RGBA**:
        // - `EXPAND`:调色板→RGB、低位深灰度→8 位、`tRNS`→alpha;
        // - `STRIP_16`:16 位降到 8 位(取高字节,与 image 同口径);
        // - `GRAY_TO_RGB`:灰度补成三通道;
        // - `PACKING`:1/2/4 位解包;
        // - `EXPAND` 已含 `tRNS`→alpha,灰度/真彩若原本无 alpha 由下面补 255。
        let transforms = (ffi::PNG_TRANSFORM_EXPAND
            | ffi::PNG_TRANSFORM_STRIP_16
            | ffi::PNG_TRANSFORM_GRAY_TO_RGB
            | ffi::PNG_TRANSFORM_PACKING) as c_int;
        ffi::png_read_png(&mut *png_ptr, &mut *info_ptr, transforms, std::ptr::null_mut());

        let width = ffi::png_get_image_width(&*png_ptr, &*info_ptr);
        let height = ffi::png_get_image_height(&*png_ptr, &*info_ptr);
        let channels = ffi::png_get_channels(&*png_ptr, &*info_ptr);
        let rowbytes = ffi::png_get_rowbytes(&*png_ptr, &*info_ptr);

        let mut result: Result<(u32, u32, Vec<u8>), String> = Err("未初始化".to_string());
        if width == 0 || height == 0 {
            result = Err("尺寸为 0".to_string());
        } else if rowbytes == 0 || rowbytes < width as usize * channels as usize {
            result = Err(format!("行宽异常: rowbytes={rowbytes}"));
        } else {
            let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
            let rows = ffi::png_get_rows(&*png_ptr, &mut *info_ptr);
            if rows.is_null() {
                result = Err("png_get_rows 返回空".to_string());
            } else {
                for y in 0..height as usize {
                    let row = *rows.add(y);
                    if row.is_null() {
                        result = Err(format!("第 {y} 行为空"));
                        break;
                    }
                    let src = std::slice::from_raw_parts(row, rowbytes);
                    match channels {
                        4 => rgba.extend_from_slice(src),
                        3 => {
                            for px in src.chunks_exact(3) {
                                rgba.extend_from_slice(px);
                                rgba.push(255);
                            }
                        }
                        1 => {
                            for &g in src {
                                rgba.extend_from_slice(&[g, g, g, 255]);
                            }
                        }
                        2 => {
                            for px in src.chunks_exact(2) {
                                rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
                            }
                        }
                        n => {
                            result = Err(format!("不支持 {n} 通道"));
                            break;
                        }
                    }
                }
                if matches!(result, Err(ref e) if e == "未初始化") {
                    result = Ok((width, height, rgba));
                }
            }
        }

        ffi::png_destroy_read_struct(
            &mut (png_ptr as *mut _),
            &mut (info_ptr as *mut _),
            std::ptr::null_mut(),
        );
        result
    }
}

/// 解成渲染器用的 [`Texture`]。
pub fn decode_texture(bytes: &[u8]) -> Result<Texture, String> {
    let (width, height, rgba) = decode(bytes)?;
    Ok(Texture {
        width,
        height,
        rgba,
    })
}
