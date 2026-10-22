//! `yu-host-wasm` · 浏览器**呈现端**(架构 B)。
//!
//! # 当前定位:只是调试期胶水,不是最终 wasm 包
//!
//! 现在这个 wasm 只服务于 dev 热重启调试:页面刷新即重新加载,它只把
//! native 引擎(`yu-cli serve`)推来的 RGBA 帧画上 canvas,全部读取与
//! 渲染语义都跑在 native。**后期的 wasm 包才是真正的编译产物**——把
//! `yu-rt` + `yu-gfx` + `yu-backend-soft` 整个编进 wasm32,浏览器脱离
//! 本地服务自跑(PROGRESS 路线表"wasm 宿主");届时本 crate 的
//! `present()` 就是帧生产端从 WS 换成页面内的接入缝。
//!
//! 读取与执行都在 native 本地服务(`yu-cli serve`);本 crate 只接收 RGBA 帧
//! 并把它呈现到 canvas:
//!
//! ```text
//! WebSocket 二进制帧(YUF1)
//!   → www/app.js 解析头
//!   → Presenter::present(rgba, w, h)
//!        ├─ WebGPU:页面安装的 window.__YU_PRESENT_GPU__(ptr,len,w,h)
//!        │           (wasm 内存 → GPU 纹理 → 全屏 blit)
//!        └─ Canvas2D:ImageData + putImageData
//! ```
//!
//! # 为什么 WebGPU 的 JS 调用在页面里
//!
//! `web-sys` 的 WebGPU 绑定需要 `--cfg web_sys_unstable_apis`(整棵依赖树
//! 都要带这个 cfg)。把它变成构建命令的隐藏前提会让 `cargo check
//! -p yu-host-wasm --target wasm32-unknown-unknown` 在别的机器上语义漂移。
//! 因此 GPU 对象创建/绘制由页面里一个**受控 shim**完成;wasm 决定优先级、
//! 提供帧缓冲、并在 shim 失败时回退 2D。合成语义仍在 native,不在浏览器。
//!
//! 本 crate **不依赖** `yu-rt`/`yu-gfx`/`yu-backend-*`/`yu-vfs`:不读游戏数据,
//! 不解释渲染语义。

#![forbid(unsafe_code)]
#![allow(clippy::result_large_err)]

use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// 呈现后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentBackend {
    /// 页面 shim:RGBA → WebGPU 纹理 → 全屏 blit。
    WebGpu,
    /// `ImageData` + `putImageData`。
    Canvas2d,
}

impl PresentBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            PresentBackend::WebGpu => "webgpu",
            PresentBackend::Canvas2d => "2d",
        }
    }
}

/// 浏览器呈现器(wasm-bindgen 类)。
#[wasm_bindgen]
pub struct Presenter {
    canvas: HtmlCanvasElement,
    backend: PresentBackend,
    context_2d: Option<CanvasRenderingContext2d>,
    width: u32,
    height: u32,
}

#[wasm_bindgen]
impl Presenter {
    /// 创建呈现器。页面若已成功初始化 WebGPU shim(并设置
    /// `globalThis.__YU_PRESENT_GPU__`),优先走 WebGPU;否则 Canvas2D。
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Result<Presenter, JsValue> {
        let backend = if gpu_shim().is_some() {
            PresentBackend::WebGpu
        } else {
            PresentBackend::Canvas2d
        };
        let mut presenter = Presenter {
            canvas,
            backend,
            context_2d: None,
            width: 0,
            height: 0,
        };
        if backend == PresentBackend::Canvas2d {
            presenter.context_2d = Some(presenter.create_2d_context()?);
        }
        Ok(presenter)
    }

    /// 当前呈现后端。
    #[wasm_bindgen(getter)]
    pub fn backend(&self) -> String {
        self.backend.as_str().to_string()
    }

    /// 呈现一帧 RGBA8(`width*height*4 == rgba.len()`)。
    ///
    /// 返回实际使用的后端(`"webgpu"` / `"2d"`)。尺寸非法显式报错。
    pub fn present(&mut self, rgba: Vec<u8>, width: u32, height: u32) -> Result<String, JsValue> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| JsValue::from_str("present: size overflow"))?;
        if rgba.len() != expected {
            return Err(JsValue::from_str(&format!(
                "present: expected {expected} bytes for {width}x{height}, got {}",
                rgba.len()
            )));
        }
        if width == 0 || height == 0 {
            return Err(JsValue::from_str("present: zero-sized frame"));
        }

        if self.backend == PresentBackend::WebGpu {
            if let Some(shim) = gpu_shim() {
                let ptr = (rgba.as_ptr() as usize).min(u32::MAX as usize) as u32;
                let len = rgba.len().min(u32::MAX as usize) as u32;
                let ok = shim
                    .call4(
                        &JsValue::NULL,
                        &JsValue::from(ptr),
                        &JsValue::from(len),
                        &JsValue::from(width),
                        &JsValue::from(height),
                    )
                    .ok()
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                if ok {
                    return Ok(PresentBackend::WebGpu.as_str().to_string());
                }
                return Err(JsValue::from_str(
                    "present: WebGPU shim failed (see console); reload after fixing GPU init",
                ));
            }
            // shim 在初始化后被移除:降级到 2D。
            self.backend = PresentBackend::Canvas2d;
            self.context_2d = Some(self.create_2d_context()?);
        }

        if self.width != width || self.height != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
            self.width = width;
            self.height = height;
        }
        let context = match self.context_2d.as_ref() {
            Some(context) => context.clone(),
            None => {
                let context = self.create_2d_context()?;
                self.context_2d = Some(context.clone());
                context
            }
        };
        let image = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), width, height)?;
        context.put_image_data(&image, 0.0, 0.0)?;
        Ok(PresentBackend::Canvas2d.as_str().to_string())
    }

    /// 最近一次呈现的尺寸(诊断用)。
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    fn create_2d_context(&self) -> Result<CanvasRenderingContext2d, JsValue> {
        let context = self
            .canvas
            .get_context("2d")?
            .ok_or_else(|| JsValue::from_str("canvas: 2d context unavailable"))?;
        context
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("canvas: context is not CanvasRenderingContext2d"))
    }
}

fn gpu_shim() -> Option<js_sys::Function> {
    let global = js_sys::global();
    let value = js_sys::Reflect::get(&global, &JsValue::from_str("__YU_PRESENT_GPU__")).ok()?;
    if value.is_undefined() || value.is_null() {
        return None;
    }
    value.dyn_into::<js_sys::Function>().ok()
}
