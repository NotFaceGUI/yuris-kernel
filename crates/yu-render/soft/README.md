# yu-backend-soft

`yu_gfx::CompositorFrame` → RGBA8 → PNG 的确定性软件后端。只消费 `yu-gfx`
公开契约,不修改语义层。

## 用法

```bash
cargo test -p yu-backend-soft
cargo clippy -p yu-backend-soft --all-targets

export YURIS_SAMPLE_ERIS_ROOT=<project-eris>   # 数据入口:data.ypf 优先,散盘 data/ 为备
export YURIS_SAMPLE_ERIS_YPF=<project-eris>/data.ypf   # 可选
cargo run -p yu-backend-soft --example first_frame
cargo run -p yu-backend-soft --example frame_walk -- 600   # 逐帧基线 TSV
```

`first_frame` 用 **`GfxHost` 同步事件路径** boot ERIS,每 tick 后用
`Vm::take_events()` 只做事件计数/drain,**不**再把同一批事件喂给 `GfxState`。
输出 `target/tmp/first_frame.png` 与 `target/tmp/first_frame.display.json`。
`frame_walk` 输出每 tick 的 ops/drawn/skipped、transition/copy/image 计数、
原始 RGBA sha256 与渲染耗时,供三端逐帧对拍(J3)。

屏幕输出是 **opaque** 窗口:组 RT 的 alpha 只参与组合成到屏幕,最终
`render_frame` 返回前 alpha 归一 255。混合按 D3D9 浮点模型(先求和再
u8 取整),与 wgpu 固定管线/本后端共用同一舍入,淡入淡出区间 0 差异。

## 执行契约

| 输入 | 行为 |
|---|---|
| `PassTarget::Texture` | `Op::Copy`(COPY/COPY2)与已取证的 `Op::Image`(`RECTPAINT/2/3`、`REVERSE`、`HORIZONTAL`)执行;其余跳过并计数。纹理操作按 `seq` 命令交错序,不按类型分组 |
| `PassTarget::Offscreen` | 组尺寸 = `SX/SY`,未给时用视口;clear = pass.clear(透明黑) |
| `PassTarget::Screen` | 输出尺寸 = `frame.viewport`;clear = pass.clear / frame.clear |
| `Op::Layer` | `Asset` / `SolidBlack1x1` / `Snapshot`(按 `CopyId` 的冻结表面) |
| `Op::Group` | 取已渲染的 offscreen 表面,组的 `A/MODE` 只在此时作用一次 |
| `Op::Transition` | 双纹理 mask pass:RGB=本层纹理;alpha=纹理 α × 覆盖度 × 层 A;mask=配对纹理 G 通道(EVID-T-05) |
| `Op::Mask` | 跳过并计数 |
| `Op::Copy` | 源=命令执行时当前状态(组按需重绘);区域=源坐标含端点,裁剪块写目标 (0,0);快照按 `CopyId` 只捕获一次 |
| `Op::Image` | `RECTPAINT/2/3`、`REVERSE`(RGB 取反)、`HORIZONTAL`(水平镜像);作用于目标层纹理副本,绘制该层时优先使用副本 |
| `TextRun` | 不栅格化,只计入 `skipped_text` |

- Alpha:固定管线 `min(max(A,0),255)/255`;纹理 alpha 与层 alpha 在混合
  方程里相乘,不预乘。D3D9 未启用 `SEPARATEALPHABLENDENABLE` 时 alpha 与
  RGB 复用同一组因子,因此 alpha 源项是 `a²`(不是 `a`)。
- 固定管线模式全集:`0/1/2/3/5/6/7/9`。源色先按引擎两阶段状态归约
  (`T`=纹理,`A`=层 alpha,`a`=纹理α×层A):
  `0/1=T`、`2/5/6/9=T·A`、`3/7=T·A+(1−a)`;然后:
  `0`: `src·a+dst·(1−a)`;`1`: `src·a+dst`;`2`: `dst−src·a`;
  `3`: `dst·src`;`5`: `src+dst−src·dst`;`6`: `max`;`7`: `min`;
  `9`: `src+dst−2·src·dst`。alpha 与 RGB 同因子。
- MODE 3/5/9 在 wgpu 端用 `Src/OneMinusDst/OneMinusSrc` 因子实现同一公式;
  `fixed_pipeline_modes_3_5_6_7_9_match_soft` 逐像素验证两端一致。
- 证据:`EVID-J-01`(`docs/fidelity-ledger-j.md`);规则来自 Microsoft
  `d3d9types.h` 的 `D3DBLEND_SRCALPHA=5` 与
  `D3DRS_SEPARATEALPHABLENDENABLE=206`(默认 FALSE),并由引擎反编译
  `FUN_00414ba4` 的 `0x13=5 / 0x14=6|2 / 0xab=1|3` 状态序列与
  `FUN_00462e40` 的 `D3DRS_ALPHABLENDENABLE(0x1B)` 佐证。
- `RECTPAINT/RECTPAINT2/RECTPAINT3`(`src/imageop.rs`)按 `FUN_0043185c`
  模式 1/2/3 执行:RGB 填充并把 alpha 置 0xFF、只填 alpha、填整个
  `0xAARRGGBB`;区域含端点并裁剪。官方微场景逐像素对拍:EVID-J-02..05;
  区域内部逐通道差 0,1px 边界归 DIV-GFX-042。
- 颜色空间:存储值直接算术(`gamma_encoded_srgb8`),不做线性化。
- 过滤:`Point` 最近邻;`Linear/Anisotropic` 双线性。缩放采样用整数
  16.16 定点。
- PNG:固定 `Fast` + `NoFilter`,同一表面重复编码逐字节一致。

## 证据与已知 Likely 决策

- **MODE 0/1/2/3/5/6/7/9 状态**:官方 0.495 `yu-ris.exe`(sha256
  `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`)
  `FUN_00414ba4`(`0x414ba4`,REA/Ghidra 12.1.4):
  - `A` 先 clamp 到 `0..255`;
  - MODE 0: `SRCBLEND=5(SRCALPHA)` / `DESTBLEND=6(INVSRCALPHA)` /
    `BLENDOP=1(ADD)`;
  - MODE 1: `5` / `2(ONE)` / `1`;
  - MODE 2: `5` / `2` / `3(REVSUBTRACT)`;
  - 只设 `D3DRS_ALPHABLENDENABLE`(状态 `0x1B`),未设
    `D3DRS_SEPARATEALPHABLENDENABLE`(状态 `0xCE`)⇒ alpha 通道与 RGB 用同一
    组因子;`MODE 0` 的 alpha 输出是 `a² + dst_a·(1−a)`,`MODE 1` 是
    `a² + dst_a`,`MODE 2` 是 `dst_a − a²`。`0xAB` 是 `D3DRS_BLENDOP`,不是
    `ALPHABLENDENABLE`。
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
- **未实现项**:掩码/部分图像加工/PSB 解码不近似;文本已实现两条路径
  (`CGACT[TEXT]` 与 `_TEXT_FLAG` 自动层),缺少 `_TEXT_FLAG` 的
  `TextRun` 仍显式跳过并计数。

## 确定性

渲染无墙钟、无线程、无散列迭代序;资产缓存键是原始 key;同一
`CompositorFrame` 与同一 `TextureStore` 连续渲染两次,PNG 字节一致
(`tests/render.rs::asset_layer_composites_to_screen` 与 `first_frame` 示例
都会自检)。
