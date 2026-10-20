# yu-gfx 未实现模块 TODO 清单

> 会话 F 续。原则:**未取证 = 不实现 + 显式 `Unimplemented` + 本清单/DIV**。
> 本清单与 `crates/yu-gfx/docs/divergences/README.md`、`docs/render-parity.md`
> 同步;当前覆盖 DIV-GFX-001..040(含 decision/note 项)。代码中以
> `TODO(DIV-GFX-xxx)` 标记。
>
> 状态:
> - `blocked` = 缺工具/样本/跨层 ABI;
> - `unimplemented` = 语义已定位但像素/公式未取证;
> - `open` = 证据不足,保持现状;
> - `partial` = 有可执行子集,剩余部分仍挡住。

| ID | 模块 | 状态 | 当前行为 | 解除条件 |
|---|---|---|---|---|
| DIV-GFX-001 | MODE 3/5/6/7/9 固定管线 | unimplemented | 层标 Unimplemented | 定位 `DAT_00801d98/d9c` 辅助纹理内容 + 像素对拍 |
| DIV-GFX-002 | MODE 4/8/10..18 着色器 | unimplemented | 层标 Unimplemented | 定位 `ID3DXEffect` 源/字节码 + 像素对拍 |
| DIV-GFX-003 | `FX/FY/FBX/.../F` 仿射/滤镜 | unimplemented | 原值进 `transform.filter` | 槽位→`+0x1b0..+0x200` 逐项绑定 + 变换公式 |
| DIV-GFX-004 | 多部件层 | unimplemented | `Part.raw` 原样保存 | `+0xa6/+0xa7/+0xb0/+0x130` 几何语义 |
| DIV-GFX-005 | `MID` 外部掩码 | unimplemented | 层标 Unimplemented | 掩码取值通道(α/亮度)/尺寸/采样 |
| DIV-GFX-006 | 过渡配对缺失/推进者 | open | 空 TID=普通层;非空缺失=Unimplemented | 受控脚本确认双 pass/无配对行为;`+0x1e8` 推进者 |
| DIV-GFX-007 | `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` | unimplemented | `Op::Copy` 恒 Unimplemented | 受控实验确认上一帧时序 + 区域语义 |
| DIV-GFX-008 | 组离屏尺寸/清屏 | open | `size=None`;后端用视口/透明黑 | 引擎非调试路径 RT 创建/清屏取证 |
| DIV-GFX-009 | 颜色空间 | open | `gamma_encoded_srgb8` | 官方截图逐像素对拍 |
| DIV-GFX-010 | 帧清屏 | open | `default_black` | 引擎帧入口/设备初始化观测 |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/...` | unimplemented | 原样进 `extra` | 手册/引擎逐项分类 |
| DIV-GFX-012 | 文本字形栅格化/排版 | unimplemented | 只输出 `TextRun`/`TextStyle` | 字体会话(`yu-font`) |
| DIV-GFX-013 | `CGACT` 图像加工像素 | unimplemented | `Op::Image` 记录原值 | 像素算法定位 + 对拍 |
| DIV-GFX-016 | `CG` 无 `FILE` 的 get-or-create | open | 建无纹理层(不绘制) | 重新锚定 CG 处理体 |
| DIV-GFX-017 | COPY 属性复制范围 | open | 复制 A/E/MODE/T/TA/坐标/滤镜/Z | `FUN_0042f809` 属性块逐项确认 |
| DIV-GFX-018 | 子窗口叠放/位置 | unimplemented | 只输出窗口状态/离屏 pass | window/host 会话 |
| DIV-GFX-019 | 顶点 DIFFUSE RGB | open | 未建模 | 还原 `0x414ba4` 顶点色构造 |
| DIV-GFX-020 | INFO `LET` 同步查询 | partial | `GfxHost` 已接通已取证字段 | — |
| DIV-GFX-021 | `TEXT[ID]` 非 UTF-8 CP932 | unimplemented | 清空并报告 | 事件 ID 改字节类型 |
| DIV-GFX-022 | 资源图 `SX/SY` | unimplemented | 层标 Unimplemented | 真实样本确认缩放/裁剪 |
| DIV-GFX-023 | 复制/图像日志上限 | note | 16,384 + `dropped_ops()` | — |
| DIV-GFX-024 | `CGINFO[SX/SY]` 资源图尺寸 | blocked | Unimplemented,不写 LET | 资产元数据 provider(图像头/解码器) |
| DIV-GFX-025 | INFO 多字段写回顺序 | unimplemented | `gfx.info.multi`,不写 LET | 引擎多字段查询顺序 |
| DIV-GFX-026 | INFO `COLOR/ONMOUSE*/CAPTION/...` | unimplemented | `gfx.info.unsupported` | 像素/鼠标/窗口 host |
| DIV-GFX-027 | `IDNO` 十进制后缀拼接 | open | `ID + "." + 十进制` | 重新锚定 CGEND/CGINFO 处理体 |
| DIV-GFX-028 | 图像层下挂子层目标 | open | 按后序扁平绘制 | 官方引擎合成脚本确认目标 RT |
| DIV-GFX-029 | pass 复制/图像交错序 | open | copy→image 分组 | 引擎命令执行顺序取证 |
| DIV-GFX-030 | `CG[TIDNO]` | open | 忽略并留档 | 确认是否 `TID + "." + N` |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层、`Z2/BNO` 修饰 | unimplemented | ID3/ID4 诊断;Z2/BNO 留存 | 引擎多图层运算/修饰语义 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 对应 | open | 当前按手册实现 | `FUN_0042f57d` 调用点确认 |
| DIV-GFX-033 | `TEXT` 显示节奏/OUTPUTMODE/按钮 | unimplemented | VM 只记录命令事实 | 文本窗口 host ABI |
| DIV-GFX-034 | 文本跨帧持久 + CLEAR 传播到 gfx | unimplemented | gfx 每帧清空 text | 文本窗口状态机/事件 |
| DIV-GFX-035 | `FONT NO/BNO` 解析、GRADATION/ROTATION/PITCH/LANG、默认字体 | unimplemented | 保存原值 | `yu-font` + 引擎默认值取证 |
| DIV-GFX-036 | FONTINFO 字体列表字段/未设置默认值 | unimplemented | 不写 LET | 字体列表 provider |
| DIV-GFX-037 | WINDOW host 属性/主窗口 A 透明度/SX-SY 视口 | open | gfx 只消费 CGID/E/A 显隐 | 窗口 host + 0.495 A 语义 |
| DIV-GFX-038 | INFO 缺失目标非 EXIST / 未分类字段 | unimplemented | 不写 LET | 引擎缺失返回语义 |
| DIV-GFX-039 | `CGACT RASTER1/FSX/FSY/FX2/FY2/Z2` | unimplemented | RASTER1 已识别;其余留存 | 参数语义取证 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态 | unimplemented | VM 归 Unknown;ERIS 未使用 | 文本状态机/`text/textinfo.html` |
| DIV-GFX-014 | 同 Z 顺序 | decision | 创建序 `(z,seq,id)` | 手册称不確定;保持确定性 |
| DIV-GFX-015 | 首屏逻辑帧号 | note | golden 取首个有可见层的帧 | VM 事件帧号;非语义缺口 |

## 代码标记

- `crates/yu-gfx/src/state.rs`:`CG TIDNO` 未处理(在 `apply_layer_params`
  TID 分支旁)、`CGACT ID3/ID4/Z2/BNO`(apply_cgact)、TEXT 持久缺失。
- `crates/yu-gfx/src/layer.rs`:`BlendMode::fixed_state()` / `confidence()` 的
  Unverified/Unimplemented 分支。
- `crates/yu-gfx/src/state.rs`:`MID`、滤镜、图像加工、COPY、资源图尺寸、
  组尺寸、图像子层。
- `crates/yu-gfx/src/op.rs`:`CopyOp`/`ImageOp` 的 status。
- `crates/yu-gfx/src/frame.rs`:清屏/颜色空间。
- `crates/yu-gfx/src/text.rs`:字形栅格化/排版。
- `crates/yu-gfx/src/transition.rs`:推进者/配对缺失。
- `crates/yu-gfx/src/path.rs`:`IDNO` 拼接方式。
- `crates/yu-rt/src/vm/handlers.rs`:`TEXT` 显示节奏属性、非 UTF-8 ID。

## 推进顺序建议

1. 资产元数据 provider → 解锁 `CGINFO SX/SY`(ERIS 按钮布局的最大缺口)。
2. 官方引擎受控脚本/截图 → 过渡无配对、COPY 时序、组 RT 清屏/尺寸、像素对拍。
3. `MODE 3/5/6/7/9` 辅助纹理;`4/8/10..18` 着色器。
4. 图像加工/多部件/MID。
5. 文本栅格化(与 `yu-font` 会话合流)。
