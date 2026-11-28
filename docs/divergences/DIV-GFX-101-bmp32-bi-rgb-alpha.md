# DIV-GFX-101 · 32bpp BI_RGB BMP 的第 4 字节是 alpha

- 状态: **fixed**(2026-11-28)
- 影响面: `yu-render-core` 资产解码(所有 32bpp BI_RGB BMP)
- 关联: DIV-GFX-005(alpha 合成)、DIV-GFX-063(滤镜)

## 事实

- 官方 0.292 手册 `cg/cg.html` 明写:
  「BMPファイルは32bitBMP(24bit画像＋α画像)に対応」。
- `<0.263 商业>` 的 `cg/main/name_window.bmp` 是 **174×43、32bpp、
  compression=0(BI_RGB)**;第 4 字节 alpha 实测 **5,348 个 0 / 1,962 个 255**
  (其余中间值),是真实 alpha。
- 旧路径 `image` crate 对 BI_RGB 32bpp 走 `RGB32`:第 4 字节不参与解码,
  输出 alpha 全 255 ⇒ 名字窗白底实心,表现为消息窗口上的一块白矩形。
- 同目录 `name_window_a.bmp` 也是 32bpp BI_RGB,alpha 5,662/1,734 混合,
  属同一族。

## 决策

- 在 `decode_asset` 的通用 `image` 解码前增加 **32bpp BI_RGB BGRA**
  专用路径:
  - `compression == 0 && bpp == 32`,自下而上/自上而下都按 stride=w*4 读;
  - 第 4 字节写回 alpha;
  - 全部 alpha 为 0 的旧式资产回退不透明(避免整层消失);
  - `BI_BITFIELDS` 等其它 32bpp 形态仍交给 `image`。
- 不做版本/游戏判断,所有 32bpp BI_RGB BMP 统一走该规则。

## 验证

- 单测 `texture::ydg_real_sample_tests::bmp32_bi_rgb_uses_alpha_channel`
  (2×2 自下而上、alpha 0/128/255、全 0 回退)。
- 真样本 `YURIS_SAMPLE_BMP32=<name_window.bmp>`:
  `real_bmp32_sample_preserves_alpha` 断言同时存在 alpha=0 与 alpha=255。
- `<0.263 商业>` desktop 重跑:名字窗白底消失,消息窗恢复透明。
