# yu-backend-wgpu

`yu_gfx::CompositorFrame` 的 wgpu 后端。语义唯一来源是 **`yu-gfx`**；
`yu-backend-soft` 是同一契约的参考执行器(浮点混合模型 + u8 量化)。本 crate 与软后端
**互为实现,不互为真值**:像素允许舍入差,但 pass 顺序、可执行/跳过计数、
跳过原因必须逐项对齐。

本 crate 只做 **离屏 RGBA8 渲染 + 读回**。窗口、surface 呈现、winit/wasm
事件循环不属于本会话;API 命名 (`TargetSet::screen`、`render_frame`) 为
宿主将来的薄壳留了位置。

## 用法

```toml
[dependencies]
yu-backend-wgpu = { path = "crates/yu-render/wgpu" }
yu-backend-soft = { path = "crates/yu-render/soft" }
yu-gfx = { path = "../yu-gfx" }
yu-vfs = { path = "../yu-vfs" }
```

```rust
use yu_backend_soft::TextureStore;
use yu_backend_wgpu::WgpuRenderer;

let store = TextureStore::new(Box::new(mount), Codec::Cp932, frame.image_probe_order.clone());
let mut backend = WgpuRenderer::new(store)?;          // 默认 DeviceOptions
let out = backend.render_frame(&frame)?;               // Surface + RenderReport + GpuStats
```

指定后端/适配器:

```rust
use yu_backend_wgpu::{DeviceOptions, WgpuRenderer};

let options = DeviceOptions {
    backends: Some(wgpu::Backends::DX12),
    adapter_name: Some("5070".into()),                 // ASCII 大小写不敏感子串
    ..DeviceOptions::default()
};
let mut backend = WgpuRenderer::new_with_options(store, &options)?;
```

### 环境变量覆盖(示例)

库内**不读环境变量**(任务书 §5);`examples/wgpu_first_frame.rs` 负责把环境变量
映射到 `DeviceOptions`:

| 环境变量 | 作用 |
|---|---|
| `YURIS_WGPU_ADAPTER` | 适配器名称子串;无匹配时返回 `platform.gpu.adapter_unavailable` |
| `YURIS_WGPU_BACKEND` | `dx12` / `vulkan` / `gl` / `primary` / `all` |

```bash
export YURIS_SAMPLE_ERIS_ROOT=<project-eris>   # 数据入口:官方挂载规则
cargo run -p yu-backend-wgpu --example wgpu_first_frame
# 输出 target/tmp/wgpu_first_frame.png
#      target/tmp/wgpu_first_frame.report.json
cargo run -p yu-backend-wgpu --example wgpu_frame_walk -- 600   # 逐帧基线 + GPU 上传计数
```

示例名与 soft 端**不同名**(`wgpu_first_frame`/`wgpu_frame_walk`),避免
Cargo 共享 `target/debug/examples/<name>.exe` 时互相覆盖(J1/J2 的实测坑)。

示例会:
- 与软后端 `first_frame` 相同的 `GfxHost` 同步事件路径 boot ERIS;
- 渲染首个可绘制帧;
- 硬校验输出尺寸 = `viewport`;全黑时警告并写 `all_black=true`
  (当前 ERIS boot 锚点尚未到 title,不作为 1:1 证据);
- 同帧渲染两次校验像素确定性;
- 若 `target/tmp/first_frame.display.json` 存在,逐项对账
  `passes / ops / executable / skipped / layers`(不一致则退出非零)。

## wgpu 版本

`wgpu = "30.0.1"`(2026-10-20 配置的 rsproxy 索引上的最新稳定版),默认
features,不开任何 experimental feature。`pollster` 只用于同步化
instance/device 创建与一次性读回。

**目标格式固定 `Rgba8Unorm`**:

- 引擎是 D3D9,反编译全树未设 `D3DRS_SRGBWRITEENABLE`(state 194),
  混合发生在 8-bit 存储值空间(`docs/render-parity.md` §10);
- `Rgba8Unorm` 采样返回 `value/255`、写入按 unorm 量化,不做 sRGB 解码/编码;
- 若换成 `Rgba8UnormSrgb`,GPU 会在采样时线性化、写入时再编码,与引擎的
  gamma 空间混合不一致。

## 模块

```text
src/device.rs     instance/adapter/device/queue;platform.gpu.* 诊断码
src/textures.rs   TextureStore RGBA8 → GPU 上传;资产/目标身份与缓存
src/pipelines.rs  blend 管线(0/1/2/6/7 + psC/psD + psA/psB);bind group/sampler/paint 缓存
src/shaders/      引擎 shader 的独立 WGSL + registry(quad/fixed/psC/psD/psA/psB/paint)
src/passes.rs     按 passes 顺序规划:跳过记账、纹理解析、顶点生成
src/readback.rs   texture → 256 字节行对齐 buffer → 紧凑 RGBA8
src/lib.rs        WgpuRenderer::new / render_frame / render_frame_scaled;GpuStats
examples/wgpu_first_frame.rs
```

## 显示缩放(effect Pass2/Pass3)

引擎把内部画面呈现到窗口时按 `DAT_00801334` 选 effect pass:
`2 → psB`(4-tap 锐利双线性)、否则 `3 → psA`(16-tap 双三次)。
后端提供**加法 API**,不改变 `render_frame` 的 1:1 行为:

```rust
use yu_backend_wgpu::ScaleFilter;

let out = backend.render_frame_scaled(
    &frame,
    Viewport::new(window_w, window_h),
    ScaleFilter::Bicubic,       // 或 ScaleFilter::Sharp
)?;
```

- 合成仍在 `frame.viewport`(引擎逻辑分辨率)完成,缩放只作用于呈现;
- `:/W0/` 后缓冲保留缩放前的画面,COPY 语义不变;
- 逐指令复刻:`SharpN = 1.25`(FXO 初值,引擎不写)、
  `YS_TexDiffU = (2/W, 0)`、D3D9 POINT 的 `floor(u*W)` tap 规则;
- 测试:`tests/wgpu.rs::display_scale_*`(1:1、2×、0.625× 与 CPU 参考 ≤1)。

模块边界与任务书建议略有调整:bind group 需要 bind group layout/sampler,
放在 `pipelines.rs`;`passes.rs` 产出有序计划,`lib.rs` 按该计划录制命令。
两者都是同一执行契约的实现细节,不改变行为。

## 执行契约

| 契约 | 本后端 |
|---|---|
| pass 顺序 | 严格 `frame.passes`:Texture(CPU 预执行/命令段)→ Offscreen(子先父后)→ Screen;COPY/快照填充在离屏之后、屏幕之前执行(可见结果不变) |
| Texture pass | `RECTPAINT/2/3`、`REVERSE`、`HORIZONTAL` 在 CPU 侧预执行并上传覆盖纹理(EVID-J-02..05、EVID-T-13);目标是快照的 RECTPAINT 走 GPU 快照填充管线 |
| Offscreen | 尺寸 = 组 `SX/SY`,未给用视口;`clear` = `pass.clear`,缺省透明黑;ops 按帧内顺序 |
| Screen | 尺寸 = `frame.viewport`;`clear` = `frame.clear`;只画 gfx 给出的主窗口组 ops |
| `Op::Layer` | `Asset`(无显式尺寸)/`SolidBlack1x1`/`Offscreen`/`Snapshot`;有 RECTPAINT 覆盖时优先覆盖纹理 |
| `Op::Group` | 取已渲染的离屏纹理;组的 `A/MODE/E` 只在画进父目标时作用一次 |
| `Op::Transition` | 独立双纹理 pipeline(配对 G 通道 mask + fade uniform);`T>=256` 覆盖度为满 |
| `Op::Mask` | 跳过并计数 |
| `Op::Copy` | GPU `copy_texture_to_texture` 到按 `CopyId` 复用的快照纹理;区域=源坐标含端点;同一 `CopyId` 只捕获一次 |
| `Op::Image` | `RECTPAINT/2/3`(快照目标走 GPU 填充)、`REVERSE`、`HORIZONTAL`;其余跳过并计数 |
| Alpha | 固定管线 `min(max(A,0),255)/255`;着色器输出 `texture.a × layer_alpha`,只乘一次;RGB 不预乘 |
| Blend | MODE 0/1/2/3/5/6/7/9;RGB 与 alpha 都按 D3D9 固定管线同因子。源色先按 stage 归约:`0/1=T`、`2/5/6/9=T·A`、`3/7=T·A+(1−a)`;颜色因子用 `Src/OneMinusDst/OneMinusSrc` 对应 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR` |
| Filter | `frame.filter` 全局;Point=nearest,Linear/Anisotropic=linear;引擎硬编码 MIP=POINT 且无 mip 链,故固定 LOD 0 等价 |
| 颜色空间 | 存储值直接算术,不线性化 |
| 文本 | 与 soft 同一 `TextComposer`:图层文本合成到基图副本后上传为 override;屏幕文本为全视口覆盖 quad;缺 `_TEXT_FLAG` 的 `TextRun` 仍 `skipped_text` |

`RenderReport` 直接复用 `yu_backend_soft::RenderReport`(同一结构、同一
`SkippedOp` 排序键),因此字段与跳过原因不会漂移。

## 性能与资源

- **资产纹理**按原始 key 上传一次;同一帧/跨帧重复引用零重传。
- **图像加工覆盖纹理**按 `(层, w, h, FNV-1a64(rgba))` 跨帧缓存;内容不变
  时复用 GPU 纹理。600 帧实测仅 4 帧上传(资产/覆盖内容首次出现)。
- **纹理身份**由进程级单调计数器分配,资产和目标共用,避免
  `(texture_id, sampler)` bind group 键跨缓存碰撞。
- **bind group** 按 `(texture_id, FilterKind)` 缓存;目标尺寸变化时只作废
  该目标的条目。
- **管线**按 blend 缓存;目标格式是全 crate 常量,键里的 format 维度因此
  是常量(见 `pipelines::TARGET_FORMAT` 注释)。
- **顶点缓冲**按 2 的幂扩容后复用;每帧一次 `write_buffer`;每 op 一次
  `draw`(起步策略,未做批处理)。
- **读回缓冲**按尺寸复用;每帧一次 `copy_texture_to_buffer` + 一次映射。
- **设备 limits** 默认请求 `wgpu::Limits::default()`(WebGPU 安全下限,
  `max_texture_dimension_2d=8192`);宿主需要更大纹理时通过
  `DeviceOptions.limits` 显式提高,超限返回 `platform.gpu.limit`。
- 不使用 `HashMap`;跳过记账/缓存都用 `BTreeMap`,迭代序稳定。
- 每帧仍会分配显示列表/顶点 `Vec`(计划产物),但 GPU 资源不逐帧重建。

## 差异账本(相对 soft 参考)

| 差异 | 类别 | 处理 |
|---|---|---|
| soft 混合模型 | 已消除 | J3 起 soft 也按 D3D9 浮点模型(先求和再 u8 取整),两边共用 u8 alpha 量化链;600 帧 0 差异 |
| 转场覆盖度 | 已对齐 | shader 与 `yu_gfx::transition_coverage` 同公式/同量化;title 与转场帧 0 差异 |
| 屏幕 alpha | 已对齐 | 最终屏幕按 opaque 窗口归一 alpha=255,与浏览器 `alphaMode=opaque` 读回一致 |
| sRGB 禁用方式 | 语义对齐 | `Rgba8Unorm` 直写;采样/清屏/混合均在存储值空间,等价于 D3D9 未开 sRGB 写 |
| `filter.source=placeholder` | 未取证 | 报告原样输出 `placeholder`;执行 point/linear,但不声称已与引擎采样器对齐 |
| 适配器/驱动差异 | 环境 | 报告记录 backend/adapter/device_type/driver;不做跨卡 golden |
| 双线性采样实现 | 舍入 | 软后端用 16.16 定点,GPU 用固定功能线性;预期 ≤1–2 LSB。当前可执行路径只有 1×1 纯色黑图,尚未在缩放资源图上验证 |
| 最近邻采样映射 | 舍入 | 四边形按像素边界映射、采样在像素中心;1:1 绘制实测一致 |
| alpha 通道的 soft 偏离 | **已修复(EVID-J-01)** | 两个后端都按 D3D9 同因子执行,alpha 源项 `a²`。soft 原先把源项写成 `a`;任务 J Phase A 改为 `mul255(a,a)` 并同步单测。修复前 ERIS 首帧有 9,451 个半透明像素不同(最大 RGB 差 61);修复后同帧逐通道差见 §首帧实测 |
| 组离屏 RT 的 alpha 存储 | 未取证 | 本后端与 soft 都用 RGBA8 直通 alpha;引擎 RT 是否为 A8/X8、是否写 alpha 位未取证(DIV-GFX-008 扩展)。若后续证据表明是 X8,组半透明合成语义需整体重做 |
| MODE 0 stage1 / MODE 2 diffuse | 未取证(gfx 契约) | gfx 没有携带逐层 diffuse/stage1 辅助纹理;与 soft 一样把 RGB 视为纹理直通、MODE 2 的 diffuse 按白处理。引擎像素未对拍 |
| 零尺寸组 pass 的 `ops_total` | 计数修正 | J2 起两端都计总数(soft 也已修正);`total=drawn+skipped` 不变量在两端成立 |
| `draw_calls` vs `ops_drawn` | 记账 | 目标尺寸为 0 时 op 算“已执行”但不产生 GPU draw;报告分别给出两个数 |
| i64 坐标 → f32 NDC | 舍入 | 极端坐标在 f32 下损失精度;这些四边形本来就在视口外,可见区域实测一致 |
| 组尺寸未给时回退视口 | 未取证 | 继承 gfx/soft 的 Likely 决策(DIV-GFX-008) |
| COPY/图像加工/掩码/过渡 | 未实现 | 与 soft 同一跳过原因与计数;不近似 |

**唯一硬性检查**(不是逐像素对拍):
- `passes / ops_total / ops_drawn / skipped_ops_total` 与
  `first_frame.display.json` 完全一致;
- 输出尺寸 = `viewport`;全黑时警告并记录 `all_black=true`(当前 boot 锚点未到
  title,不能当作 1:1 证据;`YURIS_FIRST_FRAME_TICK` 可固定非黑微场景帧)。

## 错误码

`yu-log` 当前只登记 `vfs.*`/`log.*`;gfx 的码以字符串直接使用。本 crate
新增 `platform.gpu.*` 并在库内声明常量,**不修改 `yu-log`**:

| 码 | 触发 | 理由 |
|---|---|---|
| `platform.gpu.adapter_unavailable` | 无适配器 / 名称过滤无匹配 / `request_adapter` 失败 | 环境问题;调用方应显式 SKIP 并计数 |
| `platform.gpu.device` | `request_device` 失败 | 适配器存在但设备不可用 |
| `platform.gpu.limit` | 目标超过 `max_texture_dimension_2d` | 把 wgpu panic 变成可诊断错误 |
| `platform.gpu.target` | 纹理/目标/管线/bind group 被验证拒绝;零尺寸目标 | 资源创建失败 |
| `platform.gpu.readback` | 缓冲映射/轮询失败 | 读回失败 |

资产缺失/解码失败继续使用 soft 的 `asset.*` 码:这些是**逐 op 跳过**,
不是帧失败,与软后端一致。

## 验证

```text
$ cargo test -p yu-backend-wgpu
6 lib + 7 integration + 1 doc = 14 passed; 0 failed

$ cargo test -p yu-backend-soft
10 lib + 5 render + 1 doc = 16 passed; 0 failed

$ cargo clippy -p yu-backend-wgpu --all-targets
yu-backend-wgpu 自身 0 告警(输出中的 warning 来自依赖 yu-rt)

$ grep -rn "println!\|eprintln!\|dbg!" crates/yu-render/wgpu/src
0

$ grep -rn "<game-b>\|<game-a>\|C:\\\\Users\|D:\\\\Art" crates/yu-render/wgpu/src
0

$ cargo check -p yu-backend-wgpu --target wasm32-unknown-unknown --lib
Finished
```

wasm 证据只覆盖**编译**:库内无 `std::fs`/`std::env`/路径拼接;运行时读回
需要宿主提供异步 map 回调/事件循环,本会话不把它当作可用路径。

### 首帧实测(RTX 5070 / Vulkan,默认选项)

**当前 HEAD(任务 J 后,ERIS tick 1):**

```text
frame 3, 800x600, 272 层(visible 8), 15 pass, 2 个组离屏
18 op: 13 executable / 5 skipped
文本 0, 缺失资产 0,filter.source=placeholder
draws=3 uploads=2 upload_bytes=26660 pipelines=1 bind_groups=3
vertices=360 readback=1920000 elapsed≈18.1 ms deterministic=true all_black=true
display_check: OK (passes 15 ops 18 executable 13 skipped 5)
png: 540267 bytes (soft: 540267)
warning: 首帧全黑(当前 boot 锚点未到 title/仅清屏);不是 1:1 证据
```

**历史锚点(`771cf79`,44 op):** 15 executable / 29 skipped,9,451 个半透明
像素不同(最大 RGB 差 61)。该差异由 EVID-J-01 修复:soft alpha 源项改为
`a²`;当前同帧 soft/wgpu 半透明像素逐通道差 **0**。

**非黑微场景(`fidelity-micro4`,RECTPAINT/2/3):**

```text
frame 6, 8 层(visible 8), 11 pass, 17 op, 17 executable / 0 skipped
all_black=false, display_check OK (passes 11 ops 17 executable 17 skipped 0)
png: 1920813 bytes (soft: 1920813)
soft vs wgpu: diff_pixels=0/480000 max_channel_diff=0
```

同机 DX12 + `YURIS_WGPU_ADAPTER=5070` 也通过(计数相同)。

## 已知未实现

全部继承 `yu-gfx` 的挂账,不是本后端的近似:

- MODE 4/8/10..18(公式无 blob);
- `MID` 掩码、多部件层、`FX/FR/...` 仿射/滤镜;
- `COPY/COPY2/TEXCOPY/TEXSSCOPY`、`SIZE*`/`ROTATION`/`RESIZE`/`TRIM`/
  `MONO`/`BLUR*` 等图像加工;`RECTPAINT/2/3` 已实现(EVID-J-02..05),
  `ID2`/快照源/部分区域仍挂账;
- 资源图 + `SX/SY` 的缩放/裁剪;
- 文本 GPU 字形 atlas(当前是 CPU 栅格 + 纹理 quad,与 soft 同源同像素;atlas 只是性能优化);
- 组 `SX/SY` 缺失时的引擎真实尺寸/清屏(DIV-GFX-008);
- 颜色空间逐像素对拍(DIV-GFX-009)。
