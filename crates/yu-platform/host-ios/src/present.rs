//! iOS 上屏端口(占位)。
//!
//! # 为什么现在不实现
//!
//! 1. `host-android` 的 `attach_surface` 直接返回具体类型
//!    `present::NativePresenter`,会话里的 `presenter` 字段也是具体类型;
//!    iOS 的 `CAMetalLayer` 呈现器无法塞进去。
//!    需要先抽一个平台无关的 `Presenter` 端口(attach / resize / present),
//!    这属于宿主核心重构,不在本次骨架范围。
//! 2. Metal surface 只能在 macOS/iOS SDK 下编译,Windows 上无法验证。
//!    项目纪律:无证据不猜,未验证的平台代码不伪装成可用。
//!
//! # 接线步骤(在 Mac 上)
//!
//! 1. `host-android`:加 `pub trait Presenter`(`size` / `present`),把
//!    `Session.presenter` 改为 `Option<Box<dyn Presenter>>`,
//!    `attach_surface` 泛化为 `attach_surface<P: Presenter + 'static>`。
//!    Android 的 `NativePresenter` 实现该 trait,行为零变化。
//! 2. 本模块实现 `CaPresenter::new(layer: *mut c_void, gpu: &GpuContext)`:
//!
//!    ```ignore
//!    let surface = unsafe {
//!        gpu.instance.create_surface_unsafe(
//!            wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer),
//!        )
//!    }?;
//!    ```
//!
//!    wgpu 30 的该变体在 `src/api/surface.rs:430`,由默认 `metal` feature
//!    提供;`layer` 必须是 `CAMetalLayer*`(Swift 侧 `override class var
//!    layerClass: AnyClass { CAMetalLayer.self }`,再用
//!    `Unmanaged.passUnretained(layer).toOpaque()` 交出)。
//! 3. blit 管线与 `present.wgsl` 同源;建议把 Android `present.rs` 里的
//!    管线/采样器/绑定组抽成共享的 `present-blit`,避免两平台各抄一份。
//! 4. 生命周期:`CAMetalLayer` 的引用由 Swift 壳持有,surface 必须先于
//!    layer 释放;present 回调在渲染线程,UIKit 操作回主线程。
//! 5. 验证口径:同一工程首 N 帧与 `soft` 后端逐字节 hash 对齐,再开
//!    `Immediate`/`AutoNoVsync` 的帧率对照。

/// `yu_ios_attach_layer` 的返回码:未接线。
pub const UNSUPPORTED: &str = "iOS Metal 上屏尚未接线: 需要先抽 host 的 Presenter 端口,\
再以 wgpu SurfaceTargetUnsafe::CoreAnimationLayer 建 surface;当前请用 soft 后端 + \
yu_ios_copy_frame 拉 RGBA。接线步骤见 crates/yu-platform/host-ios/src/present.rs";
