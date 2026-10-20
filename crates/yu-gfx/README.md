# yu-gfx

YU-RIS 渲染语义层:把脚本/引擎的图形命令固化成**可序列化的显示列表**
[`CompositorFrame`](src/frame.rs) —— 唯一决定"画什么"。本 crate 不含窗口、
GPU、wasm、平台代码;后端只执行显示列表,不得修改语义。

## 数据流

```text
yu-rt 事件流(VmEvent::DomainCall / Text)
  → GfxState(src/state.rs:图层树/组/窗口/过渡/文本模型)
  → CompositorFrame(src/frame.rs + pass/op/layer/group/transition/text)
  → json::to_display_json(稳定字段顺序,可逐字节 golden)
  → yu-backend-soft / yu-backend-wgpu
```

## 核心契约(后端必读,详见 `docs/render-parity.md` §12)

- pass 顺序 = 纹理 copy/image → 组 offscreen(子先父后)→ screen;
  `status.executable=false` 的 op 必须跳过并计数,不得近似;
- `A` 保留引擎单位 0..256:固定管线 `min(A,255)/255`,着色器 `A/256`;
  坐标 i64 像素,gamma 空间混合(不线性化);
- blend 19 模式的 D3D9 状态表见 `json::blend_table_json()`;
  0/1/2 可执行,3/5/6/7/9 状态有、辅助纹理未取证,4/8/10..18 未实现;
- `filter` 是全局采样器设置,非逐层;
- screen 只合成主窗口 `:/W0`,子窗口 `W1..W3` 只出 offscreen pass;
- 槽号 → 参数名一律查工程 `ysc.ybn`(`table::ParamTable`),无内置表。

## 同步宿主与查询

[`GfxHost`](src/host.rs)(`Rc<RefCell<GfxState>>`)接 `yu-rt` 后,事件产生时
同步消费;`query_info` 支持脚本期查询:CGINFO(EXIST/E/A/X/Y/Z/MODE/T/TA/TID、
组与显式尺寸层的 SX/SY)、FONTINFO(当前样式)、WINDOWINFO(主窗口几何)。
多字段同时置位顺序未取证 ⇒ 拒答不猜;资源图尺寸需资产元数据 ⇒ 挂账。

## 确定性

无散列表迭代序、无墙钟、无线程、无绝对路径;同一输入两次
`to_display_json` 逐字节一致。

## 当前未实现(全部有编号与解除条件)

`docs/gfx-unimplemented-todo.md`(DIV-GFX-001..040):MODE 3/5/6/7/9 辅助纹理、
着色器模式、FX 仿射族、多部件、MID、COPY 时序、图像加工像素、文本栅格化、
颜色空间、组 RT 尺寸/清屏等。首屏 golden:
`tests/golden/eris_first_screen_display.json`(272 层 / 15 pass / 44 op)。

## 验证

```bash
cargo test -p yu-gfx
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-gfx --test eris_first_screen
```

进度与证据:`PROGRESS.md` 成果 M3-GFX / F1 / F2;复核台账
`docs/gfx-verification-ledger.md`。
