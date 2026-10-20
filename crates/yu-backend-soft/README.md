# yu-backend-soft

`yu_gfx::CompositorFrame` → RGBA8 → PNG 的确定性软件后端。只消费 `yu-gfx`
公开契约,不修改语义层。

## 用法

```bash
cargo test -p yu-backend-soft
cargo clippy -p yu-backend-soft --all-targets

export YURIS_SAMPLE_ERIS_YSBIN=<project-eris>/save/data/ysbin
export YURIS_SAMPLE_ERIS_YPF=<project-eris>/data.ypf   # 可选
cargo run -p yu-backend-soft --example first_frame
```

`first_frame` 用 **`GfxHost` 同步事件路径** boot ERIS,每 tick 后用
`Vm::take_events()` 只做事件计数/drain,**不**再把同一批事件喂给 `GfxState`。
输出 `target/tmp/first_frame.png` 与 `target/tmp/first_frame.display.json`。

## 执行契约

| 输入 | 行为 |
|---|---|
| `PassTarget::Texture` | copy/image 全部当前 `Unimplemented`:不执行,按 `(kind, reason, target)` 计数 |
| `PassTarget::Offscreen` | 组尺寸 = `SX/SY`,未给时用视口;clear = pass.clear(透明黑) |
| `PassTarget::Screen` | 输出尺寸 = `frame.viewport`;clear = pass.clear / frame.clear |
| `Op::Layer` | `Asset` / `SolidBlack1x1`;`Snapshot` / `None` 跳过 |
| `Op::Group` | 取已渲染的 offscreen 表面,组的 `A/MODE` 只在此时作用一次 |
| `Op::Transition/Mask/Copy/Image` | 跳过并计数 |
| `TextRun` | 不栅格化,只计入 `skipped_text` |

- Alpha:固定管线 `min(max(A,0),255)/255`;纹理 alpha 与层 alpha 在混合
  方程里相乘,不预乘。
- `MODE 0`: `src·a + dst·(1−a)`;`MODE 1`: `src·a + dst`;
  `MODE 2`: `dst − src·a`。其余模式跳过。
- 颜色空间:存储值直接算术(`gamma_encoded_srgb8`),不做线性化。
- 过滤:`Point` 最近邻;`Linear/Anisotropic` 双线性。缩放采样用整数
  16.16 定点。
- PNG:固定 `Fast` + `NoFilter`,同一表面重复编码逐字节一致。

## 证据与已知 Likely 决策

- **MODE 0/1/2 状态**:官方 0.495 `yu-ris.exe`(sha256
  `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`)
  `FUN_00414ba4`(`0x414ba4`,REA/Ghidra 12.1.4):
  - `A` 先 clamp 到 `0..255`;
  - MODE 0: `SRCBLEND=5(SRCALPHA)` / `DESTBLEND=6(INVSRCALPHA)` /
    `BLENDOP=1(ADD)`;
  - MODE 1: `5` / `2(ONE)` / `1`;
  - MODE 2: `5` / `2` / `3(REVSUBTRACT)`;
  - 只设 `D3DRS_ALPHABLENDENABLE`(0xAB),未设
    `D3DRS_SEPARATEALPHABLENDENABLE` ⇒ alpha 通道与 RGB 用同一组因子
    (MODE 2 的 alpha 会被减到 `dst_a − a`)。
- **组尺寸回退视口**:未给 `SX/SY` 的组尺寸未取证(DIV-GFX-008),与
  `docs/render-parity.md` 一致,标 Likely。
- **MODE 2 的 diffuse**:契约可见公式按 `dst − src·a`(diffuse=白);
  引擎 stage0 是 `MOD(D,T)`,没有逐层 diffuse 值进入本契约,这里按白处理。
- **过滤设置**:`frame.filter.source=placeholder` 表示宿主未注入引擎设置;
  后端执行 point/linear,但**不声称**已与官方采样器对拍。
- **资产探测**:若 key 自带扩展名先试原 key;否则按
  `frame.image_probe_order` 追加扩展名。第一个存在的候选胜出;该候选解码
  失败即终态,不回退后续扩展名。`.psb`/未知码无解码器 ⇒ `asset.unsupported`。
- **资源图 + `SX/SY`**:跳过(`gfx.image.size.unsupported`);只有
  `FILE=""` 的 1×1 黑图按 `SX/SY` 拉伸。
- **未实现项**:COPY/图像加工/掩码/过渡/文本栅格化/PSB 解码均不近似。

## 确定性

渲染无墙钟、无线程、无散列迭代序;资产缓存键是原始 key;同一
`CompositorFrame` 与同一 `TextureStore` 连续渲染两次,PNG 字节一致
(`tests/render.rs::asset_layer_composites_to_screen` 与 `first_frame` 示例
都会自检)。
