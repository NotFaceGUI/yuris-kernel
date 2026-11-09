# DIV-GFX-083 · 手机画质选项：`SIZE` 改双线性采样

> 状态：**宿主可选，内核默认关闭**。默认路径仍与官方逐像素对齐；
> Android 壳默认开启，用于缓解高 DPI 屏幕上的 CG/差分缩放锯齿。

## 事实

- 官方 `CGACT[SIZE]` 是**最近邻**放大/缩小；`SIZE2` 才是双线性
  （`crates/yu-render/core/src/cache.rs`、`yu-render/soft/src/imageop.rs`）。
- 手机屏幕通常是 1200×2800 级，游戏视口 1100×825；CG/差分层经 `SIZE`
  最近邻缩放后，边缘锯齿在物理像素上被放大，肉眼比桌面明显。
- 过渡/差分动画（`CGACT[TRANSITION]` + `SIZE`）会把这种锯齿保留到合成结果，
  所以只有 CG 模式显眼，普通立绘（常用 `SIZE2`）正常。

## 实现

- `EngineOptions.bilinear_scale: Option<bool>`（`None`/`false` = 官方语义）；
- `WgpuRenderer::set_force_bilinear_scale` → `plan` 的 `SIZE` 分支：
  `bilinear = action.is_bilinear_scale() || force_bilinear_scale`；
- Android `RunConfig.bilinear_scale`，壳「SIZE 双线性」开关**默认开**；
- 关闭开关 = 完全回到官方最近邻，像素与改动前一致。

## 口径与残留

- 这是**宿主画质选项**，不是官方行为；开启后 CG 边缘与 soft/官方不再逐像素
  相同（软后端不实现该开关，保持参考解）。
- 采样在屏幕合成阶段按 `Content.bilinear` 选择 `FilterKind::Linear`，
  只影响被 `SIZE` 标记的层；不改变 `SIZE2`、不改变缩放尺寸、不改变过渡公式。
- 带宽影响：屏幕合成每像素最多 4 texel 读取，量级为视口 1100×825，
  远小于全屏 17.8MB 的表面写；比“再做一次全尺寸双线性重采样”便宜。
