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
  0/1/2/3/5/6/7/9 可执行(两阶段源色归约,见 `docs/render-parity.md` §5.2);
  4/8/10..18 官方绘制体没有分支(不是"我们没做"),按
  `unsupported-by-official` 归约到通常路径,见 DIV-GFX-002;
- `CompositorFrame.dx_mode`(默认 0)是宿主注入的脚本 `_DXMODE` 映射;
  非 0 时后端选择引擎 psC/psD 转场(独立 WGSL),见 `docs/shader-parity.md`;
- `filter` 是全局采样器设置,非逐层;
- screen 只合成主窗口 `:/W0`,子窗口 `W1..W3` 只出 offscreen pass;
- 槽号 → 参数名一律查工程 `ysc.ybn`(`table::ParamTable`),无内置表。

## 同步宿主与查询

[`GfxHost`](src/host.rs)(`Rc<RefCell<GfxState>>`)接 `yu-rt` 后,事件产生时
同步消费;`query_info` 支持脚本期查询:CGINFO(EXIST/E/A/X/Y/Z/MODE/T/TA/TID/
SX/SY/COLOR/ONMOUSE 族)、FONTINFO(当前样式与字体清单)、WINDOWINFO(主窗口
几何)。CGINFO 多字段按官方 if/else 优先级取首个置位字段(REA
`0x43d758..0x43dddf`);未分类字段与缺失目标的非 EXIST 字段仍拒答。

## 确定性

无散列表迭代序、无墙钟、无线程、无绝对路径;同一输入两次
`to_display_json` 逐字节一致。

## 当前未收口(全部有编号与解除条件)

`docs/gfx-unimplemented-todo.md`(DIV-GFX-001..080)。已关闭的包括:MODE 3/5/6/7/9
固定管线、MODE 4/8/10..18 定性为官方无实现、`CGACT` 图像加工软后端全族、
`MID` 外部掩码、`CG[TIDNO]`/`SWAP`/`ID3 ID4 Z2 BNO`、文本层 ID 字节化、
`CGACT[TEXT]` 空串降噪、psC/psD 转场、屏幕快照冻结与烘焙回执。

仍挂账:多部件层几何语义(004)、`CGINFO[SX/SY]` 未注入资产与部分
`COLOR` 字段(024/026)、缺失目标的非 EXIST 字段(038)、半像素/quad
相位与过渡覆盖度 TA 残差(042/045)、官方文本行缓冲/页清屏/折行/每字符
样式(075)、自动换行与行高基线(056)、无文本图层时的屏幕叠加层序(057)。

首屏 golden:`tests/golden/eris_first_screen_display.json`
(199 层 / 7 visible / 82 pass / 85 op;`9aca9fa`/DIV-GFX-016 删除 73 个
无 FILE/RID 空壳层后,绘制 op 不变。`cargo test -p yu-gfx --test
eris_first_screen` 当前 4/4 通过)。

## 验证

```bash
cargo test -p yu-gfx
YURIS_REQUIRE_SAMPLES=1 cargo test -p yu-gfx --test eris_first_screen
```

进度与证据:`PROGRESS.md` 成果 M3-GFX / F1 / F2;复核台账
`docs/gfx-verification-ledger.md`。
