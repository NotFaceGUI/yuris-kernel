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
| DIV-GFX-003 | `FX/FY/FBX/.../F` 仿射/滤镜 | **fixed(绑定;公式归 070)** | 逐项落到 `Transform.affine`(引擎 `+0x1b0..+0x214`),`FX/FY` 即图层位置 | 见 `docs/divergences/DIV-GFX-069-cg-affine-binding.md` |
| DIV-GFX-004 | 多部件层 | unimplemented | `Part.raw` 原样保存 | `+0xa6/+0xa7/+0xb0/+0x130` 几何语义 |
| DIV-GFX-005 | `MID` 外部掩码 | unimplemented | 层标 Unimplemented | 掩码取值通道(α/亮度)/尺寸/采样 |
| DIV-GFX-006 | 过渡配对缺失/推进者 | fixed | 双纹理可执行;配对缺失/TA<1=普通层;T 由脚本推进;mask=配对 G 通道 | EVID-T-01..09(`docs/title-motion-ledger.md`) |
| DIV-GFX-007 | `CGACT[COPY/COPY2/TEXCOPY/TEXSSCOPY]` | fixed(COPY/COPY2) | `Op::Copy` 可执行;区域=源坐标含端点、快照按 CopyId 冻结;TexCopy/TexSsCopy 仍 unimplemented | EVID-T-11/12/15 |
| DIV-GFX-008 | 组离屏尺寸/清屏/RT alpha | **fixed(J3)** | 主组直绘已实现:`build_group_passes` 跳过主组、屏幕 pass 直接含子层(`emit_children_ops`);title 淡入淡出与官方逐帧一致(EVID-J3-07) | 子组 RT 生产格式/alpha 写掩码仍挂 041 |
| DIV-GFX-009 | 颜色空间 | open | `gamma_encoded_srgb8` | 官方截图逐像素对拍 |
| DIV-GFX-010 | 帧清屏 | open | `default_black` | 引擎帧入口/设备初始化观测 |
| DIV-GFX-011 | `LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH` | **fixed(no-manual)** | 逐项定案:`LINT/LINT2`=按钮号字段、`TSX/TSY/MFILE/CASH` CG 从不读取、`DXDRAW` 只受全局开关门控、`MIPMAP` 无 mip 链、`DXBUF` 是全局开关、`RID`=资源图(缺 provider) | DIV-GFX-069 §2 |
| DIV-GFX-012 | 文本字形栅格化/排版 | **fixed(P2)** | 两条路径都执行:①`CGACT[TEXT]`→`TextDraw`→图层基图合成(ERIS 正文);②`TEXT[ID]` 自动层→`_TEXT_FLAG` 逐字 `'1'/'2'` 排版(`layout_text`)→屏幕/图层覆盖。字形用 `yu-font`;soft/wgpu 同帧逐像素一致 | 行距/基线/GDI 栅格差异见 DIV-GFX-056、DIV-FONT-002 |
| DIV-GFX-013 | `CGACT` 图像加工像素 | partial | **soft 全量实现**(手册有页的全族 + 无页 `SIZE3/4/5`/`BLUR2..6`/`ROTATION2` 按同族);**模糊族已定案(2026-10-30)**:action 码 `BLUR 0x02..BLUR6 0x07`(REA `FUN_0042f809`),算法 = 可分离盒式、半径 `max(1,SET)`、R/G/B 整数平均、边界收缩、alpha 保留(`FUN_00431d28`/`FUN_00434b7c` 逐行);soft 与 wgpu(mode 10/11 两遍)都实现且逐像素一致,`BLUR2/3/4/5` 核未逐个反编译 ⇒ 标 `Likely`。wgpu 仍缺 `DIFFUSION`/`WHIRLPOOL`/`RASTER*`(报 skip);`TEXCOPY/TEXSSCOPY` 手册无页,ERIS 3 处全为自复制,保持 Unimplemented | 两后端逐动作对拍 + wgpu 滤镜 shader/回退标注 |
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
| DIV-GFX-024 | `CGINFO[SX/SY]` 资源图尺寸 | partial(J3) | 图像头 provider 已落地(`GraphicsHost::note_asset_size`);组/黑图/快照/已注入资产可答,未注入资产仍 `gfx.info.asset_size` 不写 LET | PSB/E-mote 资产头 provider |
| DIV-GFX-025 | INFO 多字段写回顺序 | unimplemented | `gfx.info.multi`,不写 LET | 引擎多字段查询顺序 |
| DIV-GFX-026 | INFO `COLOR/ONMOUSE*/CAPTION/...` | partial | `ONMOUSE/ONMOUSE2/ONMOUSE3` 已实现(样本/快照源资产);`COLOR` 等仍 `gfx.info.unsupported` | 组/离屏快照真实像素面;COLOR 显式坐标 |
| DIV-GFX-027 | `IDNO` 十进制后缀拼接 | **fixed(no-manual)** | `ID + 十进制`(引擎 `__itoa` 直接追加,无分隔符) | DIV-GFX-069 §2 |
| DIV-GFX-028 | 图像层下挂子层目标 | open | 按后序扁平绘制 | 官方引擎合成脚本确认目标 RT |
| DIV-GFX-029 | pass 复制/图像交错序 | fixed | 按 `CopyOp::seq`/`ImageOp::seq` 交错输出(EVID-T-15) | — |
| DIV-GFX-030 | `CG[TIDNO]` | open | 忽略并留档 | 确认是否 `TID + "." + N` |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层、`Z2/BNO` 修饰 | partial | 四个并列图层引用(可带 `IDNO*` 后缀);`Z2` 引擎从不读取(合法无副作用),`BNO` 只被 `TEXT` 消费 | 非 TRANSITION 的多图层动作 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 对应 | open | 当前按手册实现 | `FUN_0042f57d` 调用点确认 |
| DIV-GFX-033 | `TEXT` 显示节奏/OUTPUTMODE/按钮 | partial(P2) | VM 记录命令事实与 `_TEXT_FLAG`(M-2);gfx 已有 `set_text_reveal` + 后端截断入口;VM reveal 查询待 H-P2-02(WAIT/CLICK 计时未取证,不用墙钟猜) | H-P2-02 |
| DIV-GFX-034 | 文本跨帧持久 + CLEAR 传播到 gfx | **fixed(P2)** | `GfxState` 持跨帧文本缓冲;`FrameStart` 不再清空;CLEAR = 宿主同步空 `TextSnapshot`(VM `TEXT[CLEAR]` 无事件)或 `clear_text()`;`CGEND`/`CG(FILE)` 重建清目标层 | `tests/text_pipeline.rs` |
| DIV-GFX-035 | `FONT NO/BNO` 解析、GRADATION/ROTATION/PITCH/LANG、默认字体 | **partial(BNO 已定案)** | `GRADATION/ROTATION/COLOR/COLOR2/SX/SY/BOLD/AA/DRAWMODE` 全量保存 + clamp;默认值按 `FUN_00464fac`;字体身份经 `TextStyle::font_request()` 交 `yu-font`(`NAME`>`NO`→默认回退表)。**`BNO` = 文字样式槽号选择器**(REA `FUN_004039d0` 写 `[0x5d1684]`;8 槽;`CGACT[BNO]` 同源),已实现为 8 套样式 + 当前槽;PITCH/LANG 仍只留值 | H-P2-03 |
| DIV-GFX-036 | FONTINFO 字体列表字段/未设置默认值 | partial(P2) | `COLOR/COLOR2/SX/SY/BOLD/AA/DRAWMODE/GRADATION/ROTATION` 经 `GfxHost` 写回 LET;`NAME`(未设置)/`NO` 列表/`NUM/PITCH/LANG` 仍 Unimplemented(需要系统字体列表) | `tests/info_query.rs` |
| DIV-GFX-037 | WINDOW host 属性/主窗口 A 透明度/SX-SY 视口 | partial(no-manual) | `CAPTION/FULLSCREEN/FULLSCREENBUTTON/MAXMODE/MINMODE` 存 `WindowState` 并由 `WINDOWINFO` 读回;`MAXMODE/MINMODE` = 每窗口按钮开关(WINDOW 槽 16/17 ↔ WINDOWINFO 槽 18/19,创建时落 `WS_MIN/MAXIMIZEBOX`,REA `FUN_00457d8c`);`MENUID/REGIONID/DRAGID/FLASH/FLASHTIME` 只归类不实现 | 窗口 host + 0.495 A 语义 |
| DIV-GFX-071 | `WINDOW[MINIMIZE/CLOSEMODE/ACTIVE/WSX/WSY]` 读写配对 | **fixed(手册有页)** | 手册 `window/window.html`/`windowinfo.html`:`MINIMIZE` = 切换动作(写 `minimized`)、`CLOSEMODE` = 0..2(写 `close_mode`)、`ACTIVE=0` 无副作用、`WSX/WSY` = `outer_size` 写侧;`FULLSCREENBUTTON` 与 `FULLSCREEN2` 互为别名 | `MAXIMIZE` 的窗口动作仍交宿主 |
| DIV-GFX-038 | INFO 缺失目标非 EXIST / 未分类字段 | unimplemented | 不写 LET | 引擎缺失返回语义 |
| DIV-GFX-039 | `CGACT RASTER1/FSX/FSY/FX2/FY2/Z2` | **fixed(no-manual)** | `RASTER1` 不在 0.495 参数表(已删);`FX/FY/X2/Y2/FX2/FY2/FSX/FSY` 像素/百分位双口径已实现;`Z2` 无副作用 | DIV-GFX-069 §2 |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态 | **fixed(M-2,P2 复核)** | `yu-rt` `op_textinfo` 按参数存在性回答 OUTPUTMODE/CX/CY/CLICK/AUTO*(手册 + REA `0x4538cc`);gfx 文本模型已消费同源 `_TEXT_FLAG` | `fixture::textinfo_*` |
| DIV-GFX-041 | 主组/子组 RT alpha | partial | 主组直绘已实现(DIV-GFX-008 fixed);子组 RT alpha 不参与合成仍按 RGBA8 直通,生产格式/写掩码未定位 | 生产路径 RT 格式/写掩码取证 |
| DIV-GFX-042 | 半像素/quad 相位 | open | 矩形内部逐通道一致;1px 顶/边与官方不同 | D3D9 half-pixel 偏移逐行取证 |
| DIV-GFX-014 | 同 Z 顺序 | decision | 创建序 `(z,seq,id)` | 手册称不確定;保持确定性 |
| DIV-GFX-015 | 首屏逻辑帧号 | note | golden 取首个有可见层的帧 | VM 事件帧号;非语义缺口 |

| DIV-GFX-047 | WebGPU 颜色混合因子 | **fixed(J4 复核)** | `BlendFactor::Src/OneMinusDst/OneMinusSrc` 就是 D3D9 `SRCCOLOR/INVDESTCOLOR/INVSRCCOLOR`;MODE 3/5/9 两端执行,soft/wgpu 逐像素一致 | — |
| DIV-GFX-048 | 组(layer set)上的 TID 转场 | open(Likely) | gfx 输出 `Op::Transition{from:Offscreen}`;不再静默当普通组 | 官方微场景(YSCom attach 阻塞) |
| DIV-GFX-049 | `image_ext_order` 空值兜底(A5) | **fixed(AU1)** | 移除内置默认序:`GfxConfig::new` 探测序为空;`from_tables` 在 `yscfg` 为空且宿主未显式注入时拒绝构造(`gfx.config.probe_order_missing`)。显式注入仍允许(不是兜底)。用例 `empty_image_ext_order_is_rejected_not_defaulted` | — |
| DIV-GFX-050 | 引擎 shader 集合 | partial(J4) | shader 集合 fixed:4 个真实 blob、FXO/pass 映射、`D3DXCreateEffect` 链、`CreatePixelShader` 虚表事实;pass0..3 全部有独立 WGSL(含 psA/psB 缩放),`render_frame_scaled` 可执行。**但 MODE 4/8/10..18 公式仍无 blob**(归 002,unimplemented)——两处注册表口径已统一为 partial | 见 `docs/shader-parity.md` |
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
  (DIV-GFX-069),绘制侧合成公式未对拍(DIV-GFX-070)⇒ 非默认值图层标
  `gfx.affine.unverified`。
- `crates/yu-gfx/src/layer.rs`:`BlendMode::fixed_state()` / `confidence()` 的
  Unverified/Unimplemented 分支。
- `crates/yu-gfx/src/state.rs`:`MID`、`RECTPAINT` 之外的图像加工、资源图尺寸、
  `RID` 资源图取图、组尺寸、图像子层。`RECTPAINT/2/3` 的 `Executable` 前置检查在
  `rect_paint_precheck`;`REVERSE`/`HORIZONTAL` 的前置检查在 `cgact_image`。
- `crates/yu-gfx/src/op.rs`:`CopyOp`/`ImageOp` 带 `seq`(命令交错序);
  `CopyTiming::CommandCurrent`;`ImageAction::Reverse/Horizontal`。
- `crates/yu-gfx/src/frame.rs`:清屏/颜色空间。
- `crates/yu-gfx/src/text.rs`:字形栅格化/排版。
- `crates/yu-gfx/src/transition.rs`:双纹理契约(已实现)。
- `crates/yu-gfx/src/path.rs`:`IDNO` 十进制后缀(已按 `__itoa` 定案)。
- `crates/yu-rt/src/vm/handlers.rs`:`TEXT` 显示节奏属性、非 UTF-8 ID。
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
