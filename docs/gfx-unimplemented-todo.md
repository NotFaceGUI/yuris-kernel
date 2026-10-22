# yu-gfx 未实现模块 TODO 清单

> 会话 F 续。原则:**未取证 = 不实现 + 显式 `Unimplemented` + 本清单/DIV**。
> 本清单与 `crates/yu-gfx/docs/divergences/README.md`、`docs/render-parity.md`
> 同步;当前覆盖 DIV-GFX-001..050(含 decision/note/withdrawn 项)。
> 任务 J2 关闭 006/007/017/029、撤回 044、新增 043(跨 crate blocked)/045。
> 任务 J3 关闭 043(`yu-rt FILEINFO[EXIST]`,title `asset.missing=0`),
> 045 保持 open 但 soft/wgpu 已同式/同量化(title 不触发 TA≠128)。
> 任务 J4:001/019/049 关闭;002 明确“shader 集合已定位但 MODE 4/8/10..18
> 公式仍无 blob”;047(颜色因子)/048(组 TID,待官方微场景)新增,
> 047 复核后 fixed(WebGPU 有逐通道颜色因子)。
> 代码中以 `TODO(DIV-GFX-xxx)` 标记。
>
> 状态:
> - `blocked` = 缺工具/样本/跨层 ABI;
> - `unimplemented` = 语义已定位但像素/公式未取证;
> - `open` = 证据不足,保持现状;
> - `partial` = 有可执行子集,剩余部分仍挡住。

| ID | 模块 | 状态 | 当前行为 | 解除条件 |
|---|---|---|---|---|
| DIV-GFX-001 | MODE 3/5/6/7/9 固定管线 | **fixed(J4)** | 辅助纹理=白/黑;stage1=BLENDCURRENTALPHA;soft/wgpu 全部执行(颜色因子见 047) | 状态表 + 单元向量;官方微场景待 YSCom 恢复 |
| DIV-GFX-002 | MODE 4/8/10..18 着色器 | unimplemented | shader 集合已定位(4 个 ps_2_0:交叉淡化/mask/两种缩放),但 MODE 4/8/10..18 的 Photoshop 公式无对应 blob;层标 Unimplemented | 找到 MODE 公式的调用点/blob + 像素对拍 |
| DIV-GFX-003 | `FX/FY/FBX/.../F` 仿射/滤镜 | unimplemented | 原值进 `transform.filter` | 槽位→`+0x1b0..+0x200` 逐项绑定 + 变换公式 |
| DIV-GFX-004 | 多部件层 | unimplemented | `Part.raw` 原样保存 | `+0xa6/+0xa7/+0xb0/+0x130` 几何语义 |
| DIV-GFX-005 | `MID` 外部掩码 | unimplemented | 层标 Unimplemented | 掩码取值通道(α/亮度)/尺寸/采样 |
| DIV-GFX-006 | 过渡配对缺失/推进者 | fixed | 双纹理可执行;配对缺失/TA<1=普通层;T 由脚本推进;mask=配对 G 通道 | EVID-T-01..09(`docs/title-motion-ledger.md`) |
| DIV-GFX-007 | `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` | fixed(COPY/COPY2) | `Op::Copy` 可执行;区域=源坐标含端点、快照按 CopyId 冻结;TexCopy/TexSsCopy 仍 unimplemented | EVID-T-11/12/15 |
| DIV-GFX-008 | 组离屏尺寸/清屏/RT alpha | **fixed(J3)** | 主组直绘已实现:`build_group_passes` 跳过主组、屏幕 pass 直接含子层(`emit_children_ops`);title 淡入淡出与官方逐帧一致(EVID-J3-07) | 子组 RT 生产格式/alpha 写掩码仍挂 041 |
| DIV-GFX-009 | 颜色空间 | open | `gamma_encoded_srgb8` | 官方截图逐像素对拍 |
| DIV-GFX-010 | 帧清屏 | open | `default_black` | 引擎帧入口/设备初始化观测 |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/...` | unimplemented | 原样进 `extra` | 手册/引擎逐项分类 |
| DIV-GFX-012 | 文本字形栅格化/排版 | unimplemented | 只输出 `TextRun`/`TextStyle` | 字体会话(`yu-font`) |
| DIV-GFX-013 | `CGACT` 图像加工像素 | partial | RECTPAINT/2/3(EVID-J-02..05)+ REVERSE/HORIZONTAL(EVID-T-13)已实现;SIZE/ROTATION/MONO/BLUR/... 仍 Unimplemented | 逐动作定位像素算法 + 官微场景对拍 |
| DIV-GFX-016 | `CG` 无 `FILE` 的 get-or-create | open | 建无纹理层(不绘制) | 重新锚定 CG 处理体 |
| DIV-GFX-017 | COPY 属性复制范围 | fixed | REA 块明确 A/E/MODE/T/TA/X/Y/尺寸/TID/MID/滤镜,未见 Z;当前保持复制 Z(Likely),调用点未定位前不改 | 重新锚定 Z 字段/调用点 |
| DIV-GFX-043 | 按钮状态后缀 `_na/_off/_on/_over` | **fixed(J3)** | 脚本用 `FILEINFO[EXIST=1]` 探测后拼名;`yu-rt` 已实现 FILE+EXIST+LET(EVID-J3-01,REA `0x44286c`)。title `asset.missing=0`,`ops 27/27/0` | `docs/title-motion-ledger.md` §9.1 |
| DIV-GFX-044 | SPLTRBUF/分屏命令 | withdrawn | 不是引擎命令,是脚本图层名 + 普通 COPY/REVERSE/SIZE/TID 组合 | — |
| DIV-GFX-045 | 过渡覆盖度 TA 残差 | open(低风险) | 模型按 TA=128 校准(残差 ≤8/255);TA≠128 在 T≈255 偏差 ~0.12;J3 起 wgpu shader 与 soft 同式/同量化,title 用 T=256/TA=128 不触发 | mask 纹理生成路径 REA 取证 |
| DIV-GFX-046 | ERIS sprite boot logo 的 BG 保持/FOUT | **fixed(J3)** | 根因不是缺 sprite 渲染器:脚本 `SP.BGCOPY`→`CGACT[SIZE]` 的缩放被忽略 ⇒ `SX/SY=0` ⇒ `es.CGACT_SIZE` 判屏幕外。MATH3D 投影 + `CGINFO[SX/SY]` 资产头 + `CGACT[SIZE]` 落地后,tick 63–190 全程保持/淡出正常(EVID-J3-03/04) | — |
| DIV-GFX-018 | 子窗口叠放/位置 | unimplemented | 只输出窗口状态/离屏 pass | window/host 会话 |
| DIV-GFX-019 | 顶点 DIFFUSE RGB | **fixed(J4)** | 顶点色=`A` 灰度(asm `0x414d09..0x414d21`);以 `T·A`/`T·A+(1−C.a)` 建模,不补顶点 RGB | — |
| DIV-GFX-020 | INFO `LET` 同步查询 | partial | `GfxHost` 已接通已取证字段 | — |
| DIV-GFX-021 | `TEXT[ID]` 非 UTF-8 CP932 | unimplemented | 清空并报告 | 事件 ID 改字节类型 |
| DIV-GFX-022 | 资源图 `SX/SY` | unimplemented | 层标 Unimplemented | 真实样本确认缩放/裁剪 |
| DIV-GFX-023 | 复制/图像日志上限 | note | 16,384 + `dropped_ops()` | — |
| DIV-GFX-024 | `CGINFO[SX/SY]` 资源图尺寸 | blocked | Unimplemented,不写 LET | 资产元数据 provider(图像头/解码器) |
| DIV-GFX-025 | INFO 多字段写回顺序 | unimplemented | `gfx.info.multi`,不写 LET | 引擎多字段查询顺序 |
| DIV-GFX-026 | INFO `COLOR/ONMOUSE*/CAPTION/...` | unimplemented | `gfx.info.unsupported` | 像素/鼠标/窗口 host |
| DIV-GFX-027 | `IDNO` 十进制后缀拼接 | open | `ID + "." + 十进制` | 重新锚定 CGEND/CGINFO 处理体 |
| DIV-GFX-028 | 图像层下挂子层目标 | open | 按后序扁平绘制 | 官方引擎合成脚本确认目标 RT |
| DIV-GFX-029 | pass 复制/图像交错序 | fixed | 按 `CopyOp::seq`/`ImageOp::seq` 交错输出(EVID-T-15) | — |
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
| DIV-GFX-041 | 主组/子组 RT alpha | partial | 主组直绘已实现(DIV-GFX-008 fixed);子组 RT alpha 不参与合成仍按 RGBA8 直通,生产格式/写掩码未定位 | 生产路径 RT 格式/写掩码取证 |
| DIV-GFX-042 | 半像素/quad 相位 | open | 矩形内部逐通道一致;1px 顶/边与官方不同 | D3D9 half-pixel 偏移逐行取证 |
| DIV-GFX-014 | 同 Z 顺序 | decision | 创建序 `(z,seq,id)` | 手册称不確定;保持确定性 |
| DIV-GFX-015 | 首屏逻辑帧号 | note | golden 取首个有可见层的帧 | VM 事件帧号;非语义缺口 |

| DIV-GFX-047 | WebGPU 颜色混合因子 | **fixed(J4 复核)** | `BlendFactor::Src/OneMinusDst/OneMinusSrc` 就是 D3D9 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`;MODE 3/5/9 两端执行,soft/wgpu 逐像素一致 | — |
| DIV-GFX-048 | 组(layer set)上的 TID 转场 | open(Likely) | gfx 输出 `Op::Transition{from:Offscreen}`;不再静默当普通组 | 官方微场景(YSCom attach 阻塞) |
| DIV-GFX-049 | `image_ext_order` 空值兜底(A5) | **open(2026-10-22 审查回退)** | J4 曾标 fixed,但实现仍是“保留默认顺序 `[png,bmp,jpg,gif,psb]` + 一条 Info 诊断”;A5 原文要求缺表 = 拒绝启动或 `Unimplemented`,记录不等于允许。见 `docs/audit-2026-10-22.md` §A2 | 二选一:拒绝启动,或所有依赖探测序的层标 Unimplemented |
| DIV-GFX-050 | 引擎 shader 集合 | **fixed(J4)** | 4 个真实 blob、FXO/pass 映射、`D3DXCreateEffect` 链、`CreatePixelShader` 虚表事实 | 见 `docs/shader-parity.md` |
| DIV-GFX-051 | 主组直绘后 `COPY ID=:/W0/` 的首帧源 | **open(2026-10-22 审查新增)** | 主组不再产出 Offscreen pass,首帧无后缓冲时 COPY 报 `gfx.copy.source.offscreen_missing`;实测 `first_frame` tick 1 的 `CGS100` 被跳过 | 裁决“回退本帧子层自合成”或“显式 `no_backbuffer` 挂账”;见 `docs/audit-2026-10-22.md` §A1 |
| DIV-GFX-052 | `REVERSE`/`HORIZONTAL` 与同层先加工的组合 | **open(2026-10-22 审查新增)** | soft 有“源已在覆盖表则基于当前像素再加工”分支,wgpu 无对应分支;无官方微场景 | 补两端一致测试,或删分支并标 Unimplemented |

## 代码标记

- `crates/yu-gfx/src/state.rs`:`CG TIDNO` 未处理(在 `apply_layer_params`
  TID 分支旁)、`CGACT ID3/ID4/Z2/BNO`(apply_cgact)、TEXT 持久缺失。
- `crates/yu-gfx/src/layer.rs`:`BlendMode::fixed_state()` / `confidence()` 的
  Unverified/Unimplemented 分支。
- `crates/yu-gfx/src/state.rs`:`MID`、滤镜、`RECTPAINT` 之外的图像加工、资源图尺寸、
  组尺寸、图像子层。`RECTPAINT/2/3` 的 `Executable` 前置检查在
  `rect_paint_precheck`;`REVERSE`/`HORIZONTAL` 的前置检查在 `cgact_image`。
- `crates/yu-gfx/src/op.rs`:`CopyOp`/`ImageOp` 带 `seq`(命令交错序);
  `CopyTiming::CommandCurrent`;`ImageAction::Reverse/Horizontal`。
- `crates/yu-gfx/src/frame.rs`:清屏/颜色空间。
- `crates/yu-gfx/src/text.rs`:字形栅格化/排版。
- `crates/yu-gfx/src/transition.rs`:双纹理契约(已实现)。
- `crates/yu-gfx/src/path.rs`:`IDNO` 拼接方式。
- `crates/yu-rt/src/vm/handlers.rs`:`TEXT` 显示节奏属性、非 UTF-8 ID。
  `FILEINFO[EXIST]` 已于 J3 实现(DIV-GFX-043 fixed);FOLDER/SIZE/RELATION
  仍按手册页挂号。

## 推进顺序建议

1. **DIV-GFX-051**(主组 COPY 首帧源):首帧 golden/case 全依赖;先裁决回退
   自合成还是显式 `no_backbuffer`。
2. **DIV-GFX-049**(A5 兜底):与 051 同为 P0 级契约项。
3. 资产元数据 provider → 解锁 `CGINFO SX/SY`(ERIS 按钮布局的最大缺口;
   图像头部分已由 J3 落地)。
4. 官方引擎受控脚本/截图 → 过渡无配对、COPY 时序、组 RT 清屏/尺寸、像素对拍。
5. `MODE 3/5/6/7/9` 辅助纹理已实现;`4/8/10..18` 着色器。
6. 图像加工/多部件/MID。
7. 文本栅格化(与 `yu-font` 会话合流)。
