# yu-gfx 未实现模块 TODO 清单

> 会话 F 续。原则:**未取证 = 不实现 + 显式 `Unimplemented` + 本清单/DIV**。
> 本清单与 `crates/yu-gfx/docs/divergences/README.md`、`docs/render-parity.md`
> 同步;当前覆盖 DIV-GFX-001..071(含 decision/note/withdrawn 项;
> 058/059 于 P8 落地,069/070/071 于 2026-10-30 新增)。
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
| DIV-GFX-002 | MODE 4/8/10..18 | **unsupported-by-official** | 官方绘制体 `FUN_00414ba4` 的 MODE 分支(第 5 个栈参数 if/else 链)只有 `0/1/2/3/5/6/7/9`;其余 11 个值落进最后的 `else`,**一条 D3D 纹理阶段/混合状态都不设**(早期分派链 `0x4155dd` 的比较集合相同)。手册 `cg/cg.html` 列了除算/差分/Overlay..PinLight 名字(属脚本语言参数域,同页还注 `覆い焼きリニア＝加算モード`),引擎未实现 ⇒ 不是"手册缺页",是官方无实现。内核按 `BlendMode::engine_effective` 归约到通常路径绘制(不再丢层),置信度标 `Unsupported`。exe 内 D3DX 编译器签名共 5 处(编译后的 shader blob),已映射的 4 个 pass(psA/psB/psC/psD)是转场/缩放,没有一个是混合模式 | `docs/render-parity.md` §5.3/§5.4;`crates/yu-gfx/tests/blend_table.rs` |
| DIV-GFX-003 | `FX/FY/FBX/.../F` 仿射/滤镜 | **fixed(绑定+缩放;半像素归 070)** | 逐项落到 `Transform.affine`(引擎 `+0x1b0..+0x214`),`FX/FY` 即图层位置;0.495 普通绘制只消费 X/Y+FBX/FBY+FCX/FCY,FRX/FRY/FEZ/FQU 绘制不读;`Layer::affine_scale` 已接 soft/wgpu | 见 `docs/divergences/DIV-GFX-069-cg-affine-binding.md` |
| DIV-GFX-004 | 多部件层 | unimplemented | `Part.raw` 原样保存 | `+0xa6/+0xa7/+0xb0/+0x130` 几何语义 |
| DIV-GFX-005 | `MID` 外部掩码 | **fixed(no-manual;图像层)** | REA `0x42a801..0x42a8a4`:两层宽高必须相等,逐像素 `target.a = mask.R`,并置 `+0x86=1`;实现为 `mask_alpha` 合成 op,soft/wgpu 同式;组图层掩码仍 open(evidence) | `DIV-GFX-005-mid-mask-alpha.md` |
| DIV-GFX-006 | 过渡配对缺失/推进者 | fixed | 双纹理可执行;配对缺失/TA<1=普通层;T 由脚本推进;mask=配对 G 通道 | EVID-T-01..09(`docs/title-motion-ledger.md`) |
| DIV-GFX-007 | `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` | fixed(COPY/COPY2;Tex* layer-loss fixed,pixels open) | `Op::Copy` 可执行;区域=源坐标含端点、快照按 CopyId 冻结。`TEXCOPY`=官方 PASTESPRITE(`0x42e67f→FUN_0043ccd8→FUN_0040af1c`)、`TEXSSCOPY`=GETSS(`0x42e68a→FUN_0043cd6c`),不是纹理 blit;现保留目标层原纹理继续绘制,只记审计项 + recoverable 诊断,不发射空快照 | 多部件/表面 provider + 官方微场景(DIV-GFX-007 档案) |
| DIV-GFX-008 | 组离屏尺寸/清屏/RT alpha | **fixed(J3)** | 主组直绘已实现:`build_group_passes` 跳过主组、屏幕 pass 直接含子层(`emit_children_ops`);title 淡入淡出与官方逐帧一致(EVID-J3-07) | 子组 RT 生产格式/alpha 写掩码仍挂 041 |
| DIV-GFX-009 | 颜色空间 | **fixed(no-manual)** | 反编译全树无 `D3DRS_SRGBWRITEENABLE`(state 194)调用 ⇒ D3D9 默认在存储值空间混合;帧输出 `gamma_encoded_srgb8`,不做线性化 | `render-parity §11`;全树扫描 |
| DIV-GFX-010 | 帧清屏 | **fixed(no-manual)** | 引擎帧入口无显式 Clear;D3D9 后缓冲初始化/官方首屏为不透明黑 ⇒ `Clear::default_black()` = `[0,0,0,255]` | `fidelity-ledger` 首屏帧 |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH` | **fixed(no-manual)** | 逐项定案:`LINT/LINT2`=按钮号字段、`TSX/TSY/MFILE/CASH` CG 从不读取、`DXDRAW` 只受全局开关门控、`MIPMAP` 无 mip 链、`DXBUF` 是全局开关、`TEX` = **图层种类读回**(`CGINFO` `0x43d962` 读 `+0xa4`;1=图像/2=组),`CG[TEX=1]` 是空层转图像层动作、`RID`=资源图(缺 provider) | DIV-GFX-069 §2 |
| DIV-GFX-012 | 文本字形栅格化/排版 | **fixed(P2)** | 两条路径都执行:①`CGACT[TEXT]`→`TextDraw`→图层基图合成(ERIS 正文);②`TEXT[ID]` 自动层→`_TEXT_FLAG` 逐字 `'1'/'2'` 排版(`layout_text`)→屏幕/图层覆盖。字形用 `yu-font`;soft/wgpu 同帧逐像素一致 | 行距/基线/GDI 栅格差异见 DIV-GFX-056、DIV-FONT-002 |
| DIV-GFX-013 | `CGACT` 图像加工像素 | partial | **soft 全量实现**(手册有页的全族 + 无页 `SIZE3/4/5`/`BLUR2..6`/`ROTATION2` 按同族);**模糊族已逐个定案(2026-10-30)**:action 码 `BLUR 0x02..BLUR6 0x07`(REA `FUN_0042f809`),六个执行体全部逐行反编译 —— `BLUR`/`BLUR4` = H→V 两遍盒式、`BLUR2` = 只有 H、`BLUR3` = 只有 V、`BLUR5` = alpha-only 遮罩(RGB 归零)、`BLUR6` 按层 `[+0x86]` 二选一;半径一律 `max(1,SET)`、边界窗口收缩、通道口径由 `[+0x86]` 选。soft 与 wgpu(mode 10..15)都实现且逐像素一致。**`DIFFUSION`/`WHIRLPOOL`/`RASTER*` 也已逐个反编译并落地**(`DIFFUSION 0x0a` = 2×2 置换 + CRT `rand()` 顺序流;`RASTER/RASTER2/RASTER3 0x15/0x16/0x17` = 行/列相位正弦位移;`WHIRLPOOL 0x27` = 定点角度表旋涡),soft 与 wgpu(mode 16..20,扩散为 `2×(SET/2)` 次 pass 的 ping-pong)两端逐像素一致,wgpu 不再 skip;`TEXCOPY/TEXSSCOPY` 归 DIV-GFX-007(PASTESPRITE/GETSS,layer-loss 已修) | 两后端逐动作对拍 + wgpu 滤镜 shader/回退标注 |
| DIV-GFX-016 | `CG` 无 `FILE` 的 get-or-create | **fixed(no-manual)** | REA `0x42a63d..0x42bbef`:lookup 失败且无 FILE/RID ⇒ `ebp=0` 最终返回 -1,**不建节点**;只有 FILE(含 `""` 1×1 黑图)或 RID 建层。旧 get-or-create 已删 | `FUN_0042a514`;手册例 4 |
| DIV-GFX-017 | COPY 属性复制范围 | fixed | REA 块明确 A/E/MODE/T/TA/X/Y/尺寸/TID/MID/滤镜,未见 Z;当前保持复制 Z(Likely),调用点未定位前不改 | 重新锚定 Z 字段/调用点 |
| DIV-GFX-043 | 按钮状态后缀 `_na/_off/_on/_over` | **fixed(J3)** | 脚本用 `FILEINFO[EXIST=1]` 探测后拼名;`yu-rt` 已实现 FILE+EXIST+LET(EVID-J3-01,REA `0x44286c`)。title `asset.missing=0`,`ops 27/27/0` | `docs/title-motion-ledger.md` §9.1 |
| DIV-GFX-044 | SPLTRBUF/分屏命令 | withdrawn | 不是引擎命令,是脚本图层名 + 普通 COPY/REVERSE/SIZE/TID 组合 | — |
| DIV-GFX-045 | 过渡覆盖度 TA 残差 | open(低风险) | 模型按 TA=128 校准(残差 ≤8/255);TA≠128 在 T≈255 偏差 ~0.12;J3 起 wgpu shader 与 soft 同式/同量化,title 用 T=256/TA=128 不触发 | mask 纹理生成路径 REA 取证 |
| DIV-GFX-046 | ERIS sprite boot logo 的 BG 保持/FOUT | **fixed(J3)** | 根因不是缺 sprite 渲染器:脚本 `SP.BGCOPY`→`CGACT[SIZE]` 的缩放被忽略 ⇒ `SX/SY=0` ⇒ `es.CGACT_SIZE` 判屏幕外。MATH3D 投影 + `CGINFO[SX/SY]` 资产头 + `CGACT[SIZE]` 落地后,tick 63–190 全程保持/淡出正常(EVID-J3-03/04) | — |
| DIV-GFX-018 | 子窗口叠放/位置 | **fixed(no-manual;gfx 边界)** | gfx 输出全部窗口状态 + 子窗口 Offscreen pass;Screen 只合成主窗口(CGID/E/A)。叠放/位置是宿主窗口职责,不是 gfx 未实现 | 手册 `window/window.html`;host B7 |
| DIV-GFX-019 | 顶点 DIFFUSE RGB | **fixed(J4)** | 顶点色=`A` 灰度(asm `0x414d09..0x414d21`);以 `T·A`/`T·A+(1−C.a)` 建模,不补顶点 RGB | — |
| DIV-GFX-020 | INFO `LET` 同步查询 | **fixed** | `GfxHost` 已接通;CGINFO 字段/TEX 种类/多字段优先级、FONTINFO 样式、WINDOWINFO 几何均可答;残余字段分别归 024/025/026/036 | `tests/info_query.rs` |
| DIV-GFX-021 | `TEXT[ID]` 非 UTF-8 CP932 | **fixed** | `VmEvent::Text.id`/`TextState.layer_id` 改 `Vec<u8>`;`op_text` 不再 `from_utf8`;gfx 路径规范化 + `LayerId` 字节精确 | 合成样本(ERIS `TEXT[ID]`=0) |
| DIV-GFX-022 | 资源图 `SX/SY` | open(evidence; layer-loss fixed) | 层矩形 `+0x78/+0x7c` 直接保存,Asset/Snapshot/Movie 由后端按矩形缩放;单边尺寸缺省轴按 1;不再标 `gfx.image.size.unsupported` | 官方截图/微场景确认缩放滤波与裁剪 |
| DIV-GFX-023 | 复制/图像日志上限 | note | 16,384 + `dropped_ops()` | — |
| DIV-GFX-024 | `CGINFO[SX/SY]` 资源图尺寸 | partial(J3) | 图像头 provider 已落地(`GraphicsHost::note_asset_size`);组/黑图/快照/已注入资产可答,未注入资产仍 `gfx.info.asset_size` 不写 LET | PSB/E-mote 资产头 provider |
| DIV-GFX-025 | INFO 多字段写回顺序 | **fixed(no-manual;CGINFO)** | CGINFO 按官方 if/else 链(REA `0x43d758..0x43dddf`)优先级取第一个置位字段:EXIST→E→A→T→TID→TA→X→Y→Z→MODE→SX→SY→…;FONTINFO/WINDOWINFO 仍单字段 | 另两命令的 REA 链 |
| DIV-GFX-026 | INFO `COLOR/ONMOUSE*/CAPTION/...` | partial | `ONMOUSE/ONMOUSE2/ONMOUSE3` 已实现(样本/快照源资产);`COLOR` 等仍 `gfx.info.unsupported` | 组/离屏快照真实像素面;COLOR 显式坐标 |
| DIV-GFX-027 | `IDNO` 十进制后缀拼接 | **fixed(no-manual),2026-11-03 勘误** | `<ID>.<十进制>`(点分隔:官方先追加常量 `0x808354` = `"."` 再 `__ultoa`;锚点见 DIV-GFX-073 §1) | — |
| DIV-GFX-028 | 图像层下挂子层目标 | **fixed(no-manual)** | `emit_image_subtree` 按引擎遍历后序(子先父)扁平绘制到父目标;组子层仍走 Offscreen。目标 RT 的独立语义若存在归 041(已 fixed) | `FUN_00413dc8`;`image_children_are_drawn_before_the_parent` |
| DIV-GFX-029 | pass 复制/图像交错序 | fixed | 按 `CopyOp::seq`/`ImageOp::seq` 交错输出(EVID-T-15) | — |
| DIV-GFX-030 | `CG[TIDNO]` | **fixed(no-manual)** | REA `0x42a8e3..0x42a936`:TID 拷入缓冲 → 追加 `.`(`0x808354`)→ `__ultoa` 基数 10 ⇒ 查表键 `TID.N`;`Transition.tid_no` 持久保存,参数处理完后统一重建配对键 | 语料 0 次;合成长度断言 |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层、`Z2/BNO` 修饰 | **fixed(no-manual)** | `ID/ID2/ID3/ID4` 四个引用全部解析(`0x42c4ed..`);只有 `TRANSITION` 消费 ID3,其它动作**不读取** ID3/ID4,`Z2`(下标 8)无读取点 ⇒ 合法无副作用,verbose 留痕不报未实现。`BNO` = 文字样式槽选择器(已实现) | `DIV-GFX-069 §2.2.1`;`FUN_0042f809` |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 对应 | **fixed(no-manual)** | CGACT 调用点已定:`SWAP` slot66→`0x42f2ac`→`ecx=1`(只换纹理+X/Y);`SWAP2` slot67→`0x42f1e3`→`ecx=2`(再换 E/A/T/TA/MODE)。手册文字相反,按引擎实现 | `FUN_0042f57d`;REA 调用点 |
| DIV-GFX-033 | `TEXT` 显示节奏/OUTPUTMODE/按钮 | **fixed** | `TextState.advance_reveal(now_ms)` 复刻 `+0x6c` 标志游标/`+0x48` 延迟(全角 WAIT、半角 WAIT/2);`TextSnapshot.reveal_chars` → Engine `set_text_reveal`;遇 `'C'` 停、点击 `finish_line` 提交;后端不自建计时器 | 官方行完成清空/`'P'` 清页/折行见 DIV-GFX-075 |
| DIV-GFX-034 | 文本跨帧持久 + CLEAR 传播到 gfx | **fixed** | `GfxState` 持跨帧文本缓冲;`FrameStart` 不再清空;CLEAR = `TextSnapshot.clear_seq` 变化(空 `bytes` 不再隐式清理)或 `clear_text()`;`CGEND`/`CG(FILE)` 重建清目标层。官方 `0x4638e4` 不清图层表面,见 DIV-GFX-075 | `tests/text_pipeline.rs`, `yu-engine/tests/text_sync.rs` |
| DIV-GFX-075 | 文本待显示行缓冲 vs 图层表面/页清屏/折行/每字符样式 | **open(已登记)** | 官方 `+0x38` 行完成清空、`'P'` 清表面、`FUN_00404c60` 宽度折行、每字符样式快照;内核为累积显示缓冲 + flag 换行 + 单样式 | 官方最小脚本 + 截图/trace(H-P2-03) |
| DIV-GFX-076 | `CGACT[TEXT]` 与图像加工的命令序(ERIS 外字链) | **外字链 fixed(no-manual);通用命令序 open** | 官方 `FUN_0042f809` 直接在 `[层+0x34]+0x90` 表面执行,`CGACT[TEXT]` 也写同一表面 ⇒ 命令序=像素序;内核延迟文字 + 图像加工清文字。**外字链**(`CG→SIZE2→MEIDO→MERGE2`)已由方案 E 降级为 `TextDraw.image`(层末一次合成,前缀保留),证据 `es_Font.yst:573..616` + REA `0x42f809`;其它图像加工落到文本层仍保守清文字,通用命令序需 A/B | 通用命令序待主人裁决;外字页截图已对照 |
| DIV-GFX-035 | `FONT NO/BNO` 解析、GRADATION/ROTATION/PITCH/LANG、默认字体 | **fixed(no-manual)** | `GRADATION/ROTATION/COLOR/COLOR2/SX/SY/BOLD/AA/DRAWMODE` 全量保存 + clamp;默认值按 `FUN_00464fac`;字体身份经 `TextStyle::font_request()` 交 `yu-font`。**`BNO` = 8 槽文字样式选择器**(REA `FUN_004039d0`),已实现;`PITCH/LANG` 原值保存(无消费点证据,不阻塞) | H-P2-03;DIV-GFX-035 |
| DIV-GFX-036 | FONTINFO 字体列表字段/未设置默认值 | **fixed(no-manual)** | `COLOR/COLOR2/SX/SY/BOLD/AA/DRAWMODE/GRADATION/ROTATION` 经 `GfxHost` 写回;`NUM`/`NAME NO=n`/`LANG NO=n` 由宿主字体清单回答;未设置 `NAME`/`PITCH/X/Y` 仍 `gfx.info.unsupported`(无消费点样本) | `tests/info_query.rs`;DIV-GFX-036 |
| DIV-GFX-037 | WINDOW host 属性/主窗口 A 透明度/SX-SY 视口 | **fixed(no-manual;gfx 读侧)** | `CAPTION/FULLSCREEN/FULLSCREENBUTTON/MAXMODE/MINMODE` 存 `WindowState` 并由 `WINDOWINFO` 读回;MINIMIZE/CLOSEMODE/ACTIVE/WSX/WSY 读写配对见 071;`MENUID/REGIONID/DRAGID/FLASH/FLASHTIME` 归 host 动作(B7),gfx 只存值不实现窗口效果 | 窗口 host + 0.495 A 语义 |
| DIV-GFX-071 | `WINDOW[MINIMIZE/CLOSEMODE/ACTIVE/WSX/WSY]` 读写配对 | **fixed(手册有页)** | 手册 `window/window.html`/`windowinfo.html`:`MINIMIZE` = 切换动作(写 `minimized`)、`CLOSEMODE` = 0..2(写 `close_mode`)、`ACTIVE=0` 无副作用、`WSX/WSY` = `outer_size` 写侧;`FULLSCREENBUTTON` 与 `FULLSCREEN2` 互为别名 | `MAXIMIZE` 的窗口动作仍交宿主 |
| DIV-GFX-038 | INFO 缺失目标非 EXIST / 未分类字段 | unimplemented | 不写 LET | 引擎缺失返回语义 |
| DIV-GFX-039 | `CGACT RASTER1/FSX/FSY/FX2/FY2/Z2` | **fixed(no-manual)** | `RASTER1` 不在 0.495 参数表(已删);`FX/FY/X2/Y2/FX2/FY2/FSX/FSY` 像素/百分位双口径已实现;`Z2` 无副作用 | DIV-GFX-069 §2 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态 | **fixed(M-2,P2 复核)** | `yu-rt` `op_textinfo` 按参数存在性回答 OUTPUTMODE/CX/CY/CLICK/AUTO*(手册 + REA `0x4538cc`);gfx 文本模型已消费同源 `_TEXT_FLAG` | `fixture::textinfo_*` |
| DIV-GFX-041 | 主组/子组 RT alpha | **fixed(no-manual)** | 主组直绘已实现(DIV-GFX-008);子组 RT alpha 按 EVID-J-08 不参与父级合成:soft 离屏 pass 末强制 alpha=255,wgpu group draw 用 `OPAQUE_TEX` 采样管线;micro5 四个采样点与官方一致 | `EVID-J-08`;`subgroup_rt_alpha_*` |
| DIV-GFX-042 | 半像素/quad 相位 | **open(evidence)** | REA `FUN_00414ba4` 网格初值 `x0=X/100-0.5`、`y0=Y/100-0.5`(常量 `_DAT_0058ebd0`);D3D9 光栅规则下 1px 顶/边覆盖与当前整数矩形不同。**无官方 1px 边缘截图前不全局位移**,避免改 golden 猜像素 | B8:官方 1px 边缘/四角微场景截图 |
| DIV-GFX-014 | 同 Z 顺序 | decision | 创建序 `(z,seq,id)` | 手册称不確定;保持确定性 |
| DIV-GFX-015 | 首屏逻辑帧号 | note | golden 取首个有可见层的帧 | VM 事件帧号;非语义缺口 |

| DIV-GFX-047 | WebGPU 颜色混合因子 | **fixed(J4 复核)** | `BlendFactor::Src/OneMinusDst/OneMinusSrc` 就是 D3D9 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`;MODE 3/5/9 两端执行,soft/wgpu 逐像素一致 | — |
| DIV-GFX-048 | 组(layer set)上的 TID 转场 | **fixed(no-manual)** | `group_transition_op` 输出 `Op::Transition{from: Offscreen(组), to: 配对}`;引擎按层表统一解析 TID(`FUN_00413dc8`),组也是层。无官方微场景,端到端用例 `group_tid_emits_transition_from_offscreen` | `FUN_00413dc8` 层表解析 |
| DIV-GFX-049 | `image_ext_order` 空值兜底(A5) | **fixed(AU1)** | 移除内置默认序:`GfxConfig::new` 探测序为空;`from_tables` 在 `yscfg` 为空且宿主未显式注入时拒绝构造(`gfx.config.probe_order_missing`)。显式注入仍允许(不是兜底)。用例 `empty_image_ext_order_is_rejected_not_defaulted` | — |
| DIV-GFX-050 | 引擎 shader 集合 | **fixed(J4)** | 4 个真实 ps_2_0 blob、FXO/pass 映射、`D3DXCreateEffect` 链、`CreatePixelShader` 虚表事实;pass0..3 全部有独立 WGSL(含 psA/psB 缩放),`render_frame_scaled` 可执行。MODE 4/8/10..18 无 shader 属官方无实现(归 002 `unsupported-by-official`) | `docs/shader-parity.md` |
| DIV-GFX-051 | 主组直绘后 `COPY ID=:/W0/` 的首帧源 | **fixed(AU1)** | 首帧没有后缓冲时用 `frame.clear` 合成视口大小的后缓冲初值(D3D9 设备创建后后缓冲即存在,内容为初始化清屏色;DIV-GFX-010)。两端一致,不再报 `offscreen_missing`。用例 `main_group_copy_on_first_frame_uses_device_clear` | 仍需官方首帧截图逐像素确认(现锚点为“启动黑屏”) |
| DIV-GFX-052 | `REVERSE`/`HORIZONTAL` 与同层先加工的组合 | **fixed(AU1)** | wgpu 补上与 soft 逐字对齐的分支(源已加工则基于当前像素算一次;否则按 `(源身份, 操作)` 跨帧缓存)。跨后端用例 `reverse_after_rectpaint_matches_soft` 逐像素一致 | — |
| DIV-GFX-053 | 复制源层同帧被 `CGEND` 删除 | **fixed(AU1)** | `CopyOp.source_texture` 在**命令时刻**固定源基础纹理(官方 COPY 在命令处理器立即执行,EVID-T-15);`frame()` 不再因源节点消失丢弃复制。ERIS 按钮快照 68 条 `COPY2` 恢复(首屏 texture pass 12 → 80) | — |
| DIV-GFX-054 | COPY 区域缺省坐标 = 整幅,不是 0 | **fixed(AU1)** | `CopyRegion::Partial` 四个坐标改为 `Option`;`resolve()` 缺省取 `0/0/宽-1/高-1`。证据:ERIS `CGACT[... COPY2=1 X=0 X2=@sx-1]`(无 `Y/Y2`)是整高切片 | — |
| DIV-GFX-055 | `CGACT[TEXT]` 图层合成模型 / `SET` | **fixed(P2,Likely)** | 每帧绘制项合成到图层基图副本;`SET 0→1`、`1..=10→9`、`>=11→3` 次合成(REA `FUN_004056b8`);ERIS 正文路径 | `docs/divergences/DIV-GFX-055-cgact-text-layer-compose.md` |
| DIV-GFX-056 | 自动文本换行/行高/基线 | open(Likely) | 换行只在本行有字符时推进;行高=`SY`;基线=单元顶+ascent | 官方文本截图(H-P2-03) |
| DIV-GFX-057 | 无文本图层时的屏幕叠加 | open(Likely) | 目标层缺失 ⇒ 屏幕覆盖层 + 强制合成(soft/wgpu 已实现全视口 overlay);层序/裁剪未取证 | 官方最小脚本(H-P2-03) |
| DIV-GFX-058 | `COPY`/按钮快照不携带源层文本覆盖(存读档行文本空白) | **fixed(P8)** | 文本绘制项跨帧持久+去重+像素重写失效;`CopyOp` 命令时刻冻结源层文本;后端快照层文本基面(soft Surface / wgpu CPU 镜像)。存读档行文本随 S4 复验 | — |
| DIV-GFX-059 | `FONT[NAME=""]` 零字形;`COLOR/COLOR2` 位掩码截断;文本逐帧重烤 | **fixed(P8)** | 空 NAME 归一化→槽默认 family(回退表 `0x403b8e..0x403bdc`);COLOR/COLOR2 带符号 clamp(REA `0x444620..0x44464a`);`compose_layer_cached` 按层修订号缓存合成 | — |

## 代码标记

- `crates/yu-gfx/src/state.rs`:`CG TIDNO` 未处理(在 `apply_layer_params`
  TID 分支旁);`CGACT` 的 `ID3/ID4` 只在非 `TRANSITION` 动作上挂号
  (`Z2` 已确认引擎无读取点 ⇒ 静默接受)。~~TEXT 持久缺失~~ **已修(P2/058)**。
- `crates/yu-gfx/src/state.rs`:`Transform.affine` 已按引擎偏移落地
  (DIV-GFX-069),普通图层 scale 已实现:`Layer::affine_scale()` =
  `FBX/10000`/`FBY/10000`,soft/wgpu 共用,soft 已支持 f64 尺寸与
  亚像素采样;整层 `Unimplemented` 移除,主窗口/滤镜字段只出
  `gfx.affine.effect-unverified`。FRX/FRY/FEZ/FQU 绘制不读,
  FRZ/FEX/FEY/FMD 只属主窗口分支。**仍缺**:官方 `-0.5` 半像素与
  16.16 最终像素口径的官方截图对拍(DIV-GFX-042/064;`DIV-GFX-070` 是
  `CGINFO[COLOR]`,不是仿射公式)。
- `crates/yu-gfx/src/layer.rs`:`BlendMode::fixed_state()` / `confidence()` 的
  Unverified/Unimplemented 分支。
- `crates/yu-gfx/src/state.rs`:`MID` 与资源图 `SX/SY` 已去整层毒化
  (2026-11-05:基图/层矩形照画 + recoverable 诊断);`TEXCOPY/TEXSSCOPY`
  只留审计项与 recoverable 诊断;`RID` 资源图取图、组尺寸、图像子层仍待
  provider/证据。`RECTPAINT/2/3` 的 `Executable` 前置检查在
  `rect_paint_precheck`;`REVERSE`/`HORIZONTAL` 的前置检查在 `cgact_image`。
- `crates/yu-gfx/src/op.rs`:`CopyOp`/`ImageOp` 带 `seq`(命令交错序);
  `CopyTiming::CommandCurrent`;`ImageAction::Reverse/Horizontal`。
- `crates/yu-gfx/src/frame.rs`:清屏/颜色空间。
- `crates/yu-gfx/src/text.rs`:字形栅格化/排版。
- `crates/yu-gfx/src/transition.rs`:双纹理契约(已实现)。
- `crates/yu-gfx/src/path.rs`:`IDNO` 十进制后缀(已按 `__itoa` 定案)。
- `crates/yu-rt/src/vm/handlers.rs`:`TEXT` 显示节奏属性、非 UTF-8 ID
  **已落地**(H-P2-01/02:字节 ID + `advance_reveal`;表面/页语义见 DIV-GFX-075)。
  `FILEINFO[EXIST]` 已于 J3 实现(DIV-GFX-043 fixed);FOLDER/SIZE/RELATION
  仍按手册页挂号。

## 推进顺序建议

1. **官方首帧截图**:DIV-GFX-051 现以“D3D9 设备初始化的黑屏”为锚点;
   需要官方首帧(启动瞬间)`capture_screen.py --warmup 0` 逐像素确认。
2. 资产元数据 provider → 解锁 `CGINFO SX/SY` 的完整 provider(图像头部分
   已由 J3 落地;PSB/E-mote 仍缺)。
4. 官方引擎受控脚本/截图 → 过渡无配对、COPY 时序、组 RT 清屏/尺寸、像素对拍。
5. `MODE 3/5/6/7/9` 辅助纹理已实现;`4/8/10..18` 着色器。
6. 图像加工/多部件/MID。
7. ~~文本栅格化(与 `yu-font` 会话合流)~~ **已由 P2 落地**(DIV-GFX-012 fixed;
   尾项 056/057 等官方文本 oracle)。
