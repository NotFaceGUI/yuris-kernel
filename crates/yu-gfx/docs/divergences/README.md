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
| DIV-GFX-012 | 文本字形栅格化/行距/描边/窗口定位 | **fixed(P2;行距/基线细节归 056)** | 两后端字形执行落地(`TextComposer`/`TextLayoutFrame`);行距/基线为 Likely ⇒ 056;描边按 `SET=1/0` 合成次数(REA `FUN_004056b8`) |
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
| DIV-GFX-025 | INFO 多字段同时置位时 `LET` 写回顺序未取证 | unimplemented | `gfx.info.multi`;VM 不写 LET,避免猜顺序;**B2 子波 2 排期中(2026-10-24 门控未过,等并行 CGS 会话落地后按 REA 取证实现;roadmap-batches §2 B2)** |
| DIV-GFX-026 | INFO 的 `COLOR/ONMOUSE*/CAPTION/ACTIVE/FULLSCREEN/...` 需要像素/鼠标/窗口 host | partial | `ONMOUSE/ONMOUSE2/ONMOUSE3` 已实现(REA `0x43d664`/`0x43de01`;宿主注入 `TextureSampler`;快照按 `snapshot_files` 源资产采样);`COLOR`/组/离屏像素仍 `gfx.info.unsupported`;**B2 子波 2 排期中(2026-10-24 门控未过,等并行 CGS 会话落地后按 REA 取证实现;roadmap-batches §2 B2)** |
| DIV-GFX-027 | `IDNO` 的 `ID + "." + 十进制` 拼接未重新锚定(旧地址失效) | open | 路径按现规则;若引擎不同则需改 |
| DIV-GFX-028 | 图像层下挂子层的目标 RT(同一目标 vs 组 RT)只有遍历汇编证据,无真实样本 | open | 当前按后序扁平绘制到父目标;需官方合成脚本确认 |
| DIV-GFX-029 | pass 的 copy/image 分组顺序与脚本命令交错序不同 | fixed(J2) | EVID-T-15:官方命令处理器立即执行;`frame()` 按 `CopyOp::seq`/`ImageOp::seq` 交错输出纹理 pass(微场景 `j2-micro-copy2` 软/硬/官方一致) |
| DIV-GFX-030 | `CG[TIDNO]` 未处理(引擎可能按 `TID + "." + N` 解析) | open | 解析为 `TID` 的 IDNO 变体未取证;当前忽略该参数并只留档 |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层运算、`Z2`/`BNO` 修饰参数未取证 | unimplemented | ID3/ID4 已诊断;Z2/BNO 原样留存,不执行 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 的精确对应仅 Likely | open | 当前 SWAP=图像+坐标+可见属性,SWAP2=图像+坐标;需 `FUN_0042f57d` 调用点确认 |
| DIV-GFX-033 | `TEXT` 的 `OUTPUTMODE/WAIT/RETURN/PAGE/CLICK/AUTO*/…BUTTON` 显示节奏未实现 | unimplemented | VM 只记录命令事实;点击/跳过按钮属 host |
| DIV-GFX-034 | 文本跨帧持久 | **fixed(P2)** | 跨帧 `BTreeMap<LayerId, TextBuffer>`;`TEXT[CLEAR]` 经宿主同步传播;`FrameStart` 不再清空 |
| DIV-GFX-035 | `FONT` 字段解析与默认字体 | partial | P2:`TextFontRequest`(NAME>`NO`>BNO>默认)+ GRADATION/ROTATION clamp 保存;P6:BNO 未取证分支按帧去重报 `text.font.bno_fallback`(DIV-FONT-007);PITCH/LANG 仍只留值 |
| DIV-GFX-036 | `FONTINFO NUM/LANG/GRADATION/ROTATION/PITCH/X/Y` 需要字体列表;未设置的 COLOR2/NAME 默认值未取证 | unimplemented | 返回 `gfx.info.unsupported`,不写 LET;**B2 子波 2 排期中(2026-10-24 门控未过,等并行 CGS 会话落地后按 REA 取证实现;roadmap-batches §2 B2)** |
| DIV-GFX-037 | `WINDOW` 的 CAPTION/ACTIVE/FULLSCREEN/MINIMIZE/CLOSEMODE/MENUID/REGIONID/DRAGID/Z 归 host;主窗口 `A` 透明度(β4 仅显隐)/`SX/SY` 是否改视口未取证 | open | gfx 记录 X/Y/SX/SY/Z;Screen 只消费 E/A 的显隐与 CGID |
| DIV-GFX-038 | INFO 对“缺失图层/窗口”的非 EXIST 字段、以及 `MFILE/TEX/FX/TRIM/...` 未取证 | unimplemented | 返回 `gfx.info.missing/unsupported`;VM 不写 LET;**B2 子波 2 排期中(2026-10-24 门控未过,等并行 CGS 会话落地后按 REA 取证实现;roadmap-batches §2 B2)** |
| DIV-GFX-039 | `CGACT` 未分类参数:`RASTER1`(手册有、ERIS 表无)、`FSX/FSY/FX2/FY2`、`Z2` | unimplemented | `RASTER1` 已识别为图像动作;其余原样留存 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态查询 | **fixed(M-2/P2)** | `yu-rt` `op_textinfo` 按工程 ysc 参数名路由(OUTPUTMODE/CX/CY/CLICK/AUTO*/LET);ERIS 语料 0 使用,状态源 = P2 文本状态机 |
| DIV-GFX-041 | 主组/子组 RT 的结构与 alpha 格式:主组 `W0` 官方直绘到屏幕;子组 RT alpha 不参与合成(行为已由 EVID-J-08 微场景确认),生产路径的 `D3DFMT`/alpha 写掩码未定位 | partial | 主组直绘已落地(DIV-GFX-008 fixed);子组 RT 仍按 RGBA8 直通 alpha,生产格式/写掩码待取证 |
| DIV-GFX-042 | D3D9 半像素/quad 采样相位:官方矩形顶行与背景混合(1px 边缘),内核整数矩形全填充 | open | `RECTPAINT` 区域内部已逐通道一致;1px 边缘差异单独挂账,不纳入 Phase B 门禁 |
| DIV-GFX-047 | WebGPU 颜色混合因子 | **fixed(J4 复核)** | WebGPU `BlendFactor::Src/OneMinusDst/OneMinusSrc` 是 D3D9 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR` 的逐通道等价物;MODE 3/5/9 两端执行,`fixed_pipeline_modes_3_5_6_7_9_match_soft` 逐像素验证 |
| DIV-GFX-048 | 组(layer set)上的 `TID` 转场 | open(Likely) | gfx 已输出 `Op::Transition { from: Offscreen(组), to: 配对 }`,不再当普通组画掉;引擎按层表统一解析 TID。官方微场景因 YSCom 编译器无法 attach(`state0 != 1`)暂缺,见 `docs/shader-parity.md` §6 |
| DIV-GFX-049 | `yscfg.image_ext_order` 为空时的探测序兜底(A5) | **fixed(AU1)** | 移除内置默认序;`from_tables` 在 `yscfg` 空且宿主未显式注入时拒绝构造(`gfx.config.probe_order_missing`)。显式注入允许(不是兜底)。用例 `empty_image_ext_order_is_rejected_not_defaulted` | — |
| DIV-GFX-050 | 引擎 shader 集合与 MODE 4/8/10..18 | partially fixed | 全 exe 扫描只有 4 个真实 ps_2_0 blob(FXO VA `0x58d420`),pass0/1=交叉淡化/mask 转场,pass2/3=显示缩放滤镜;`CreatePixelShader` 由静态 D3DX9 在 `D3DXCreateEffect` 内调用。MODE 4/8/10..18 的 Photoshop 公式仍无 blob,保持 Unimplemented。详见 `docs/shader-parity.md` |
| DIV-GFX-051 | 主组直绘后 `COPY ID=:/W0/` 的首帧源 | **fixed(AU1)** | 首帧无后缓冲时用 `frame.clear` 合成视口大小的后缓冲初值(D3D9 设备创建后后缓冲即为初始化清屏色,DIV-GFX-010);两端一致 |
| DIV-GFX-052 | `REVERSE`/`HORIZONTAL` 与同层先加工的组合 | **fixed(AU1)** | wgpu 补上与 soft 逐字对齐的分支;跨后端用例 `reverse_after_rectpaint_matches_soft` 逐像素一致 |
| DIV-GFX-053 | 复制源层同帧被 `CGEND` 删除 | **fixed(AU1)** | `CopyOp.source_texture` 在命令时刻固定源纹理(EVID-T-15);按钮快照 68 条 `COPY2` 恢复 |
| DIV-GFX-054 | COPY 区域缺省坐标 = 整幅 | **fixed(AU1)** | `CopyRegion::Partial` 坐标改 `Option` + `resolve()`;ERIS `X/X2` 无 `Y/Y2` = 整高切片 |
| DIV-GFX-055 | `CGINFO[ONMOUSE*]` 的快照/组像素面 | partial | 快照层按 `snapshot_files` 的源资产采样(按钮 `BT.OFF` 即此路径);组/离屏快照无独立 CPU 像素面 ⇒ `INFO_UNSUPPORTED`;`COLOR` 仍未实现 |
| DIV-GFX-058 | `COPY`/按钮快照不携带源层的文本覆盖(`CGACT[TEXT]` 画在临时按钮层 → `COPY` 到按钮精灵,文本丢失) | fixed(P8) | 标签实际画在**快照精灵层**(COPY2 目标)上且画一次跨帧常驻(明文 es_Button/es_Font/es_SaveLoad);修复 = 文本绘制项跨帧持久+去重+像素重写失效、`CopyOp` 命令时刻冻结源层文本、后端快照层文本基面(Soft Surface / wgpu CPU 镜像)。存读档行文本随 S4 复验 || DIV-GFX-058 | `COPY`/按钮快照不携带源层的文本覆盖(`CGACT[TEXT]` 画在临时按钮层 → `COPY` 到按钮精灵,文本丢失) | fixed(P8) | 标签实际画在**快照精灵层**(COPY2 目标)上且画一次跨帧常驻(明文 es_Button/es_Font/es_SaveLoad);修复 = 文本绘制项跨帧持久+去重+像素重写失效、`CopyOp` 命令时刻冻结源层文本、后端快照层文本基面(Soft Surface / wgpu CPU 镜像)。存读档行文本随 S4 复验 |
| DIV-GFX-059 | `FONT[NAME=""]`/本机未装名字体时**零字形**;`COLOR/COLOR2` 用位掩码而非带符号 clamp;文本逐帧重烤 | fixed(P8) | 官方 FONT 处理体对 NAME 做名字查找,查不到落**槽默认 family**(回退表 `0x403b8e..0x403bdc`);COLOR/COLOR2 为 `clamp(v,0,0xFFFFFF)`(REA `0x444620..0x44464a`:负数→0)——`es._txcolor` 的 `COLOR2=-1` 哨兵因此是**纯黑**。修复:gfx 空 NAME 归一化(清 NAME/NO/BNO→默认 family)+ 带符号颜色 clamp;后端名字查找失败回落默认 family(`text.font.name_fallback`);`compose_layer_cached` 按层修订号缓存合成结果 |
| DIV-GFX-056 | 文本行距/基线:`line_height()`/`ascent_px` 的引擎值未逐位取证 | open(Likely) | P2 落地的 `TextStyle::line_height()`/`FontSource::ascent_px` 为 Likely 口径;对话正文视觉可读,无官方截图 oracle ⇒ 与 H-P2-03 一并取 |
| DIV-GFX-057 | 无图层屏幕的文本叠加(屏幕级自动文本) | open(Likely) | P2:屏幕级文本走全视口覆盖 quad(与 wgpu 同公式);无图层时的层序/裁剪未取证 |


---

## DIV-GFX-058 · COPY/快照不携带源层文本覆盖(2026-10-23,存读档菜单行文本空白)

- **现象**(用户桌面实跑,S4 存读档界面):8 个选择行按钮的底图可见、**标签文本全部缺失**;底栏系统按钮(セーブ/ロード…)有字 —— 那些是烤好文字的整图素材,不走文本管线。
- **机制(P8 取证修正,见 `target/tmp/p8/M-A-notes.md`)**:明文源码证明按钮标签**不是**画在 `BT.OFF.TMP` 源层再 COPY;真实链是 `\BT.ES1` 构建(`CG[ID="BT.OFF.TMP" FILE=…]` → 逐列 `COPY2` 切片到按钮精灵层 → `CGEND[TMP]`,es_Button.yst:2376-2682)之后,`\BT.FONT.DRAW` → `es.BT.ID.OFF.GET` 返回**按钮 OFF 精灵层**(COPY2 的目标)→ `es.FONT.DRAW.SET` 对该层发 `CGACT[TEXT]`(影/`SET=1` 轮廓/`SET=0` 填色,es_Font.yst:640-672)。存读档行文本 = `es_SaveLoad.yst:1008-1027`:`BT.CGCLEAR`(对标签精灵层 `RECTPAINT3 SET=0` 清面)后逐行画 16 行描述。即**标签画在快照精灵层上,画一次、跨帧常驻**。内核里该层 `TextureRef::Snapshot(copy_id)`:① 后端文本基面解析 `layer_base` 拒绝 Snapshot 源(`text.layer_source_unsupported`)⇒ 标签一帧都不显示(S4 直接病灶);② `text_draws` 逐帧清空 ⇒ 即便显示也只活一帧;③ COPY 源层的已画文本不进快照(方案 A 缺口)。
- **官方语义**:引擎 `CGACT[TEXT]` 即时把字符写进图层表面 —— REA(0.495,sha256 `4f5eefa3…138f`):处理体 `0x404398` → 驱动 `0x4046fc` → 旗标流循环 `0x404914` → 单字合成 `0x4056b8`(GDI mask `0x40450c0` 后按 SET 走 `0x406184`/`0x407cf0` 合成 **1/9/3 次进表面字节**;`0x404914` 的 `'P'/'p'` 页断分支还把文本表面整面清零再续画)——写表面、无显示列表;手册「元のイメージには戻せません」同证。因此官方 COPY 天然携带已画文本,标签画一次即常驻。
- **方案决策(P8,动手前定案)**:
  1. **`CGACT[TEXT]` 绘制项改为跨帧持久**(DIV-GFX-055 预登记的演进):同层命令时刻**精确去重**(对 `DRAWMODE=1` 的重复合成,重复项不是幂等的,去重是正确性前提)+ 每层上限;**像素重写失效** = `CGEND`(既有)/`CG FILE` 换图(既有)/`CGACT[COPY]` 目标层(P8 新增)/Executable 的图像加工目标层(P8 新增,BT.CGCLEAR 即此)/`SWAP` 随纹理交换文本项(P8 新增)。对话逐字路径可见结果不变(每帧重发同集合,去重后逐像素等价)。
  2. **方案 A:COPY 命令时刻冻结源层文本**(`CopyOp.texts`,内部字段不进 JSON,沿用 `source_texture` 先例),后端解析快照面 = 基图 + 冻结文本;先 COPY 后文本不得进入快照(DIV-GFX-053 同门)。
  3. **快照层文本基面**:`layer_base` 支持 `TextureRef::Snapshot`(soft 读快照 Surface;wgpu 在快照捕获时保留 CPU 镜像;组/后缓冲源镜像不可得时显式诊断挂账,不猜)。
  - 否决方案 B(CACT 时刻整层烘焙进 gfx 像素面):字形栅格化归 `yu-font`/后端,gfx 无像素面可烤,且逐字高频重合成有性能风险。
- **门禁**:title 600 帧 soft/wgpu 与基线(P8 改动前,`target/tmp/p8/baseline-*.tsv`)逐帧 0 差异;微场景「先文本后 COPY」「先 COPY 后文本」双 golden;S4 更新 expect 前新 display/PNG 先给主人过目;soft/wgpu 同帧逐像素一致。
- **落地与实测(2026-10-23 收口)**:
  - gfx:`LayerTexts{draws,seen,revision}` 持久 + 精确去重 + 上限 16384;失效点 = CGEND / CG(FILE) / Executable COPY 目标 / Executable 图像加工目标 / SWAP(交换并改写 target);`CopyOp.texts` 命令时刻冻结;`CompositorFrame.text_revisions` 供后端缓存。
  - soft:快照层文本基面(`layer_base` 支持 Snapshot,含 `text_blocks` 语义的 S4 标签层);COPY 源面合成冻结文本;`text_override_ids` 区分文本覆盖(P COPY 源跳过)与图像加工覆盖;每层每帧只合成一次。
  - wgpu:快照 CPU 镜像(`TargetSet.snapshot_cpu`,捕获时按目标尺寸 + 区域裁剪构造,GPU 原地加工后失效);1.4 段按命令序预捕获;1.6 段合成冻结文本为 COPY 源;1.5 段文本基面读镜像;`layer_image` 跳过文本覆盖。
  - **性能契约**:`TextComposer::compose_layer_cached`(层 + revision + 基面身份)避免逐帧重烤 —— 未缓存时 wgpu 菜单帧 48-65ms,缓存后 **1.6-2.0ms**;自动文本层(布局每帧变化)不进缓存。**修订号必须全局单调**:早期实现按层从 0 计数,文本项被 `BT.CGCLEAR`/COPY 清除后重建会复用旧号 → 缓存撞键,二级菜单文字与点击换色停留在旧合成(已改为全局发号器 + 清除即换新号;实测父菜单→子菜单文字正确更新)。
  - 微场景(双端):`snapshot_after_text_carries_frozen_glyphs`、`text_after_copy_does_not_enter_snapshot`、`text_on_snapshot_layer_persists_across_frames`(soft `tests/render.rs`)、`snapshot_text_timing_matches_soft`、`text_on_snapshot_layer_matches_soft`(wgpu `tests/wgpu.rs`)。
  - S4 实测:存档板可见、锁图标/底栏正常;板的浅色文字是**开界面前一帧的后缓冲截图**在 50% 暗化背景上(`COPY2 :/W0 -> SLTEMP/CGS1`,EVID-J3-06),与官方的截图背景机制一致;空槽无描述文本 ⇒ 板内标签为空属于**正确**(标签渲染能力已由选项菜单/设定界面证实)。
  - **仍挂账**:「既読色」(点过的选项变蓝)= `es.SELD` 经 `SAVE/LOAD[FILE="seld.sd"]` 持久化,属 SAVE 域(DIV-RT-042 写盘门禁),P8 未动;后端缓存不影响可见结果(600 帧 0 差异 + 双端逐像素一致)。弹窗/存档背景透出上一屏 = 场景 CGS 截图栈生命周期,**见 DIV-RT-050**(`docs/divergences/`),与本档案的文本语义无关。

---

## DIV-GFX-059 · `FONT` 字体身份与颜色 clamp(2026-10-23,按钮/对话文字全不显示)

- **现象**:桌面实测「点击开始游戏后」的选项/对话文字**整块缺失**(层、坐标、颜色命令都在,字形为零)。渲染报告:`text_draws=25 glyphs=0 diags=["text.font_missing"]`。
- **根因链**:
  1. `es._txfont` 系列把**空字体名**送进 `FONT[NAME=""]`(本轮语料由 `\FONT.FONT($FONT.FONT)`/`$T(36,11)` 空值路径触发);`TextStyle::font_request` 返回 `Name([])` → `find_family("")` 为 `None` → 后端 `font_missing`,**每个字符都不栅格化**。
  2. 官方语义(REA FONT 处理体 + `docs/font-provenance.md` §4.2):NAME 槽做名字查找,**默认值 = 槽默认 family**;名字查找失败/空串落 5 项回退表首项(`0x403b8e..0x403bdc`),不是“不画”。BNO/NO 被 NAME 覆盖后同理。
  3. 颜色 clamp:REA `0x444620..0x44464a` 为 `clamp(value, 0, 0xFFFFFF)`(SBB/JL + `MOV EAX,0xFFFFFF`);`es._txcolor` 的 `COLOR2=-1` 哨兵因此得到 **0x000000 纯黑**(影子的正确颜色)。旧实现用 `& 0xFFFFFF` 位掩码 → -1 变白 → 影子呈“画了两遍”的亮重影。
- **修复**:
  - gfx:`FONT[NAME=""]` → 清 `name/font_no/font_bno`(字体身份回默认 family;字符串槽出现即覆盖全局索引);`COLOR/COLOR2` 解析时带符号 `clamp_color`(负数→0)。
  - soft(两后端共用):`resolve_face` 名字查找失败回落 `default_face`(回退表),并挂一次性诊断 `text.font.name_fallback`;`compose_layer_cached`(层 + `text_revisions` + 基面身份)只重烤变化内容(菜单帧 48→2ms 级)。
  - json:`to_json`/`to_display_json` 的 `text_layout`/`text_draws` 由“写在 `"text":[...]` 数组内部”改为**数组闭合后的帧级兄弟成员**(空 text + 非空 extras 曾产出非法 JSON;`--full` 转储受影响,display golden 形状随 S4/config 用例 expect 一并复核)。
- **回归**:`group.rs::swap_exchanges_layer_texts` 等文本语义用例;微场景 5 条(见 058);S4/设定/正文三用例画面按新渲染出图待主人批准后 `--update`。
- **仍挂账**:字体列表 `FONTINFO[NUM/LANG/NAME]`(设置界面字体选择器)仍需宿主注入系统字体清单(TEXT 域跨宿主接线);既読色依赖 SAVE 域 `seld.sd`。

## DIV-GFX-060 · `CGACT[TEXT]` 的 `SET` 合成是**偏移**外扩,不是同位置叠加

- 状态: **fixed(P9,2026-10-24)**
- 优先级: 高(所有按钮/选项标签的描边粗细)

### 现象

选项菜单标签与官方逐字步进一致(191 vs 192 px),但墨迹明显更粗、更白、更糊;
`SET=1` 轮廓把字压实成实心。

### 证据(REA 0.495 `FUN_004056b8`,asm `0x405703..0x405c32`)

`SET` 分支里 9 次 `FUN_00406184` 调用**每次传入不同的 X/Y 偏移**:

```text
0x40571c: MOV EAX, [ESP+0x80]        ; 基准
0x405727: LEA EDI, [EDI+EAX-0xa]     ; SET-10
0x405789: MOV EAX, [ESP+0x84]
0x405794: LEA EDI, [EDI+EAX-0xa]
...      每处 LEA EDI,[EAX+EDI] / SUB EDI,EAX 组装出 ±step
```

- `SET=0`:1 次,`(0,0)`;
- `SET=1..=10`:9 次,3×3 邻域,步长 = `SET`;
- `SET>=11`:3 次,`(d,0)/(0,d)/(d,d)`,`d = SET-10`。

### 旧实现(错)

`for _ in 0..9 { draw_at(same_pos) }` —— 同位置 9 次 alpha 合成把抗锯齿边缘
压实成实心,轮廓与字心同色 ⇒ 字变粗变白。官方是外扩 1 px 但保留 AA。

### 修复

`TextDraw::composite_offsets() -> ([(i32,i32); 9], usize)`;soft 渲染循环按
`offsets[..passes]` 逐次偏移绘制。回归:`yu-gfx` lib 单测
`composite_passes_follow_rea_branches` 断言三种分支的偏移集合。
