# DIV-GFX 系列 · yu-gfx 与官方引擎的已知差异与挂账

> 制度:`docs/architecture.md` §8、`docs/render-parity.md`。
> 每条含:现象、证据、处置、复核方式。状态:`open` / `fixed` /
> `unimplemented`(已知语义未实装,后端必须跳过)/ `blocked`(缺样本/工具)。

| 编号 | 主题 | 状态 | 影响 |
|---|---|---|---|
| DIV-GFX-001 | MODE 3/5/6/7/9 的辅助纹理/两阶段归约 | **fixed(J4)** | 辅助纹理已定案:`DAT_00801d98`=白(`WHITETEXBUF ERROR.`)、`DAT_00801d9c`=黑(`BLACKTEXBUF ERROR.`);stage1 = `BLENDCURRENTALPHA`(值 16)。源色归约:`0/1=T`、`2/5/6/9=T·A`、`3/7=T·A+(1−C.a)`。soft 全部执行;wgpu 同样执行 3/5/6/7/9(DIV-GFX-047 已复核) |
| DIV-GFX-002 | MODE 4/8/10..18 的着色器本体未定位 | unimplemented | 同上 |
| DIV-GFX-003 | `FX/FY/FBX/FBY/FRX/FRY/FRZ/FCX/FCY/FEX/FEY/FEZ/FSD/FMD/FQU/FID/F` 的槽位→结构体偏移未逐项绑定 | unimplemented | 参数原样进 `Transform.filter`;图层标 Unimplemented |
| DIV-GFX-004 | 多部件层(`+0xa6/+0xa7/+0xb0/+0x130`)几何语义未取证 | unimplemented | 契约保留 `Part` 位置,首屏未出现 |
| DIV-GFX-005 | 外部掩码 `MID` 的取值通道(α/亮度)/尺寸要求未取证 | unimplemented | 带 `MID` 图层标 Unimplemented |
| DIV-GFX-006 | 过渡 `TID` 配对层缺失时的第二纹理来源、`T` 的推进者(`+0x82` vs `+0x1e8`) | fixed(J2) | EVID-T-01..09:配对缺失/`TA<1` = 单纹理普通层(asm `0x415558→0x416f8d`);`T` 由脚本推进;mask = 配对纹理 G 通道;覆盖度模型见 `transition_coverage`。两端执行 `Op::Transition` |
| DIV-GFX-007 | `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` | fixed(J2;TexCopy/TexSsCopy 仍 unimplemented) | EVID-T-11/12/15:区域=源坐标含端点、裁剪块写目标(0,0)、时序=命令当前、快照按 `CopyId` 冻结;软/硬后端执行复制与快照绘制 |
| DIV-GFX-008 | 图层组离屏尺寸/清屏/RT alpha 语义:主组 `W0` 官方直接画到屏幕,子组离屏 alpha 不参与父级合成(EVID-J-08) | **fixed(J3)** | 主组直绘已实现:主组不产出离屏 pass,`frame()` 的屏幕 pass 直接含其子层(`emit_children_ops`)。title 淡入淡出与官方逐帧一致(EVID-J3-07);首屏 census 15→14 passes / 18→17 ops |
| DIV-GFX-009 | 颜色空间:引擎未设 `D3DRS_SRGBWRITEENABLE`,按 gamma 空间混合;未用官方截图逐像素验证 | open | 帧输出 `gamma_encoded_srgb8` |
| DIV-GFX-010 | 帧入口无显式 `Clear`;可见背景黑来自 D3D9 默认/设备初始化 | open | `Clear::default_black()`(Likely) |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH/...` 的视觉语义未分类 | unimplemented | 原样进 `Layer.extra`,每参数一条诊断 |
| DIV-GFX-012 | 文本字形栅格化/行距/描边/窗口定位未实现 | unimplemented | 只输出 `TextRun`/`TextStyle` 模型 |
| DIV-GFX-013 | `CGACT` 图像加工:`RECTPAINT/2/3`(EVID-J-02..05)与 `REVERSE`/`HORIZONTAL`(EVID-T-13)已实现;`SIZE`/`ROTATION`/`MONO`/`BLUR`/... 像素算法仍未取证 | partial | 上述动作在整层、Asset/1×1 黑/快照基图时 Executable;`SIZE`(SIZE/SIZE2 重采样)、`MONO`(SET=128 实测全黑,语义未定)、ID2/区域变体保持 Unimplemented |
| DIV-GFX-014 | 同 Z 顺序:手册称「不確定」;内核取创建序 | fixed(decision) | `(z, seq)` 稳定排序 |
| DIV-GFX-015 | 首屏逻辑帧号由 VM 事件决定;`frame 0` 是初始化,首个可见画面在后续逻辑帧 | note | golden 取“首个有可见层的逻辑帧” |
| DIV-GFX-016 | `CG` 无 `FILE` 时按引擎 get-or-create 建无纹理节点;手册例 4 称“图层不存在=错误” | open | 无纹理节点不产生绘制,只保留状态;会话 F 未能重新锚定 CG 处理体,保持现状 |
| DIV-GFX-017 | `COPY` 属性复制范围:`FUN_0042f809` 行 365-447 明确复制 A/E/MODE/T/TA/X/Y/尺寸/TID/MID/滤镜;**未见 Z** | fixed(J2,重新锚定) | 结论:属性块无 Z,但 `copy_attributes` 复制 Z 的调用点尚未定位;当前保持复制 Z(与旧实现一致),台账记 **Likely**;重新锚定调用点前不改行为 |
| DIV-GFX-043 | 按钮状态后缀 `_na/_off/_on/_over` 由脚本 `FILEINFO[EXIST=1]` 探测后拼接 | **fixed(J3)** | EVID-T-20/21/22 + EVID-J3-01:`yu-rt` 已实现 FILE+EXIST+LET(REA `0x44286c`);title `asset.missing=0`、`ops 27/27/0`。见 `docs/title-motion-ledger.md` §9.1 |
| DIV-GFX-046 | ERIS sprite 系统的 boot logo 保持/淡出(BG 提升) | **fixed(J3)** | 根因 = `MATH3D` 未实现 ⇒ 投影缩放 0 ⇒ `es.CGACT_SIZE` 判屏幕外;`SP.BGCOPY`/`FOUT` 本身不需要新契约。已实装 MATH3D、`CGINFO[SX/SY]` 资产头回退、`CGACT[SIZE/SIZE2]` 缩放(EVID-J3-03/04)。见 `docs/divergences/DIV-GFX-046-sprite-bg-hold.md` |
| DIV-GFX-044 | `SPLTRBUF`/分屏是否为独立引擎命令 | withdrawn(J2) | EVID-T-10:`SPLTRBUF/SPL/SPLTR` 是脚本图层名约定,由普通 `CG/CGACT(COPY/REVERSE/SIZE)` + `TID` 转场组合;无独立命令 |
| DIV-GFX-045 | 过渡覆盖度模型的 TA 依赖残差 | open(低风险) | `transition_coverage` 按 TA=128 校准(T∈{64,128,192} 残差 ≤8/255);TA=64/192 在 T≈255 处偏差最大 ~0.12;title 用 TA=128。J3 已把同一模型/量化链同步到 wgpu shader(600 帧 0 差异);TA≠128 残差仍需固定管线 mask 纹理生成路径的 REA 取证 |
| DIV-GFX-018 | 子窗口(`WINDOW[NO=1..3] CGID`)的叠放/位置/可见性不在 gfx | unimplemented | gfx 输出全部窗口状态;Screen 只合成主窗口(CGID/E/A),子窗口组只产出 Offscreen pass,交 window/host 会话 |
| DIV-GFX-019 | 顶点 DIFFUSE RGB | **fixed(J4)** | `FUN_00414ba4` asm `0x414d09..0x414d21` 把顶点色写成 `A` 灰度;`0x414d72` 对 shader 模式写 `0xFF000000`。固定管线 stage0 的 `D` 因此等价于层 A,后端以 `T·A`/`T·A+(1−C.a)` 建模,无需新增顶点 RGB 属性 |
| DIV-GFX-020 | `CGINFO/FONTINFO/WINDOWINFO` 的 `LET` 查询需要脚本执行期同步读图形状态 | partial | `yu-rt::GraphicsHost` + `yu_gfx::GfxHost` 已实现同步查询与 LET 写回;`EXIST/E/A/X/Y/Z/MODE/T/TA/TID`、组/黑图尺寸、FONTINFO 当前样式、WINDOWINFO 主窗口几何可用 |
| DIV-GFX-021 | `TEXT[ID]` 事件字段是 UTF-8 `String`;CP932 非 UTF-8 ID 无法无损承载 | unimplemented | 非 UTF-8 ID 清空并显式报告,不做有损替换;ERIS 未使用 TEXT[ID] |
| DIV-GFX-022 | 资源图 + `SX/SY` 的缩放/裁剪语义未取证(仅 `FILE=""` 黑图有真实用法) | unimplemented | `SolidBlack1x1` 按尺寸可执行;资源图带尺寸标 `gfx.image.size.unsupported` |
| DIV-GFX-023 | 复制/图像日志上限 16,384 条,超限丢弃最旧一半 | note | 记录 `dropped_ops()` + `gfx.log.truncated`;正常首屏为 0 |
| DIV-GFX-024 | `CGINFO[SX/SY]` 对资源图返回真实纹理尺寸需要资产元数据(图像文件头/解码器);gfx 不读文件 | blocked | 有显式 `SX/SY` 的层、组、`SolidBlack1x1` 可答;资源图无显式尺寸时 Unimplemented,不猜 0 |
| DIV-GFX-025 | INFO 多字段同时置位时 `LET` 写回顺序未取证 | unimplemented | `gfx.info.multi`;VM 不写 LET,避免猜顺序 |
| DIV-GFX-026 | INFO 的 `COLOR/ONMOUSE*/CAPTION/ACTIVE/FULLSCREEN/...` 需要像素/鼠标/窗口 host | unimplemented | `gfx.info.unsupported`;不伪造 |
| DIV-GFX-027 | `IDNO` 的 `ID + "." + 十进制` 拼接未重新锚定(旧地址失效) | open | 路径按现规则;若引擎不同则需改 |
| DIV-GFX-028 | 图像层下挂子层的目标 RT(同一目标 vs 组 RT)只有遍历汇编证据,无真实样本 | open | 当前按后序扁平绘制到父目标;需官方合成脚本确认 |
| DIV-GFX-029 | pass 的 copy/image 分组顺序与脚本命令交错序不同 | fixed(J2) | EVID-T-15:官方命令处理器立即执行;`frame()` 按 `CopyOp::seq`/`ImageOp::seq` 交错输出纹理 pass(微场景 `j2-micro-copy2` 软/硬/官方一致) |
| DIV-GFX-030 | `CG[TIDNO]` 未处理(引擎可能按 `TID + "." + N` 解析) | open | 解析为 `TID` 的 IDNO 变体未取证;当前忽略该参数并只留档 |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层运算、`Z2`/`BNO` 修饰参数未取证 | unimplemented | ID3/ID4 已诊断;Z2/BNO 原样留存,不执行 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 的精确对应仅 Likely | open | 当前 SWAP=图像+坐标+可见属性,SWAP2=图像+坐标;需 `FUN_0042f57d` 调用点确认 |
| DIV-GFX-033 | `TEXT` 的 `OUTPUTMODE/WAIT/RETURN/PAGE/CLICK/AUTO*/…BUTTON` 显示节奏未实现 | unimplemented | VM 只记录命令事实;点击/跳过按钮属 host |
| DIV-GFX-034 | 文本跨帧持久:gfx 每帧清空 `text`,`TEXT[CLEAR]` 未传播到 gfx | unimplemented | 当前 `TextRun` 只保留一帧;引擎文本窗口状态机未建模 |
| DIV-GFX-035 | `FONT[NO/BNO]` 未解析到真实字体;`GRADATION/ROTATION/PITCH/LANG/X/Y` 未分类;默认字体/颜色/AA 未取证 | unimplemented | 保存原值;字体解析归 `yu-font` |
| DIV-GFX-036 | `FONTINFO NUM/LANG/GRADATION/ROTATION/PITCH/X/Y` 需要字体列表;未设置的 COLOR2/NAME 默认值未取证 | unimplemented | 返回 `gfx.info.unsupported`,不写 LET |
| DIV-GFX-037 | `WINDOW` 的 CAPTION/ACTIVE/FULLSCREEN/MINIMIZE/CLOSEMODE/MENUID/REGIONID/DRAGID/Z 归 host;主窗口 `A` 透明度(β4 仅显隐)/`SX/SY` 是否改视口未取证 | open | gfx 记录 X/Y/SX/SY/Z;Screen 只消费 E/A 的显隐与 CGID |
| DIV-GFX-038 | INFO 对“缺失图层/窗口”的非 EXIST 字段、以及 `MFILE/TEX/FX/TRIM/...` 未取证 | unimplemented | 返回 `gfx.info.missing/unsupported`;VM 不写 LET |
| DIV-GFX-039 | `CGACT` 未分类参数:`RASTER1`(手册有、ERIS 表无)、`FSX/FSY/FX2/FY2`、`Z2` | unimplemented | `RASTER1` 已识别为图像动作;其余原样留存 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态查询未接线(`text/textinfo.html`) | unimplemented | VM 目前归 Unknown;ERIS 未使用;需与 TEXT 状态机一起做 |
| DIV-GFX-041 | 主组/子组 RT 的结构与 alpha 格式:主组 `W0` 官方直绘到屏幕;子组 RT alpha 不参与合成(行为已由 EVID-J-08 微场景确认),生产路径的 `D3DFMT`/alpha 写掩码未定位 | partial | 主组直绘已落地(DIV-GFX-008 fixed);子组 RT 仍按 RGBA8 直通 alpha,生产格式/写掩码待取证 |
| DIV-GFX-042 | D3D9 半像素/quad 采样相位:官方矩形顶行与背景混合(1px 边缘),内核整数矩形全填充 | open | `RECTPAINT` 区域内部已逐通道一致;1px 边缘差异单独挂账,不纳入 Phase B 门禁 |
| DIV-GFX-047 | WebGPU 颜色混合因子 | **fixed(J4 复核)** | WebGPU `BlendFactor::Src/OneMinusDst/OneMinusSrc` 是 D3D9 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR` 的逐通道等价物;MODE 3/5/9 两端执行,`fixed_pipeline_modes_3_5_6_7_9_match_soft` 逐像素验证 |
| DIV-GFX-048 | 组(layer set)上的 `TID` 转场 | open(Likely) | gfx 已输出 `Op::Transition { from: Offscreen(组), to: 配对 }`,不再当普通组画掉;引擎按层表统一解析 TID。官方微场景因 YSCom 编译器无法 attach(`state0 != 1`)暂缺,见 `docs/shader-parity.md` §6 |
| DIV-GFX-049 | `yscfg.image_ext_order` 为空时的探测序兜底(A5) | **open(2026-10-22 审查回退)** | J4 曾标 fixed,但实现仍是“保留默认顺序 `[png,bmp,jpg,gif,psb]` + 一条 Info 诊断”;A5 原文要求缺表 = 拒绝启动或 `Unimplemented`,记录不等于允许。见 `docs/audit-2026-10-22.md` §A2 | 二选一:拒绝启动,或所有依赖探测序的层标 Unimplemented |
| DIV-GFX-050 | 引擎 shader 集合与 MODE 4/8/10..18 | partially fixed | 全 exe 扫描只有 4 个真实 ps_2_0 blob(FXO VA `0x58d420`),pass0/1=交叉淡化/mask 转场,pass2/3=显示缩放滤镜;`CreatePixelShader` 由静态 D3DX9 在 `D3DXCreateEffect` 内调用。MODE 4/8/10..18 的 Photoshop 公式仍无 blob,保持 Unimplemented。详见 `docs/shader-parity.md` |
| DIV-GFX-051 | 主组直绘后 `COPY ID=:/W0/` 的首帧源 | **open(2026-10-22 审查新增)** | 主组不再产出 Offscreen pass,首帧无后缓冲时 COPY 报 `gfx.copy.source.offscreen_missing`(`first_frame` 实测 `CGS100` 被跳过);golden/用例仍标 executable。见 `docs/audit-2026-10-22.md` §A1 |
| DIV-GFX-052 | `REVERSE`/`HORIZONTAL` 与同层先加工的组合 | **open(2026-10-22 审查新增)** | soft 有“源已在覆盖表则基于当前像素再加工”分支,wgpu 无对应分支;无官方微场景。见 `docs/audit-2026-10-22.md` §A4 |
