# yu-gfx 审查主张验证台账(会话 F)

> 输入 = `docs/tasks/task-f-gfx-repair.md` 的 A/B/C/D/E/F 主张。
> **本文件先于任何代码修改产生**;未进本台账的条目不得改代码。
>
> 真值层级:官方引擎可观测行为 > 官方手册原文 > 真实脚本/官方产物 >
> 反编译/REA > 旧线文档 > 审查主张。
>
> 引擎 = 官方 SDK 0.495 `yu-ris.exe`,sha256
> `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`
> (Ghidra 12.1.4 + REA;本会话只读,未 annotate)。
> 手册 = `<side-repo>/target/sdk/0.495/yu-ris_0495_032/マニュアル/YU-RIS/html/`,
> CP932 逐页解码读取。
> 脚本 = `<side-repo>/workbench/project-eris/save/data/script/ERIS/`。
> 反编译树 = `<side-repo>/workbench/analysis/decomp/`。

## 0. 结论统计

A/B/C = 28 条“审查主张”(逐条结论):

| 结论 | 数量 | 说明 |
|---|---:|---|
| Confirmed | 19 | 其中 15 项进入修复(A5 为部分修复);4 项维持现状/挂 DIV(A9/B6/B9/B10) |
| Refuted | 7 | B1/B2/B3/B4/B7/B8/C6;审查读错手册/反编译或把决策当成错误 |
| Uncertain | 2 | A8(CG 无 FILE)、B5(顶点 DIFFUSE RGB);只挂 DIV,不改 |

D(16)/E(10)/F(8) 是缺失/健壮性/测试检查清单:除明确挂 Unimplemented 的
D6/D7/D10/D11/D12/D13 外,其余按“Confirmed → 修复/保持”处理
(D2 已部分修复,D8/D9 归 host,D13 原样留档)。

审查中最严重的三处误读:

1. **B4(TFACTOR 只在 0/3/7)是错的**。`FUN_00414ba4` 行 144-154 的集合是
   `{0,1,2,3,5,6,7,9,100,98}`;3/7 只是分支内再覆盖成白色。现有代码正确。
2. **B7(A 被清零)是错的**。`in_stack_00000010==0 → A=0` 的实参来自调用点
   `0x413f29: MOV dword ptr [ESP+0xc], 1`,图层遍历恒传 1;清零路径属于别的调用者。
3. **B2(E 可能是 +0xa4)是错的**。+0x89 是 E,已由遍历 `0x413e47` 与析构
   `0x42c400` 双向确认;+0xa4 是层种类(1=图像,2=组,0=已删),正是代码的
   `visible` / `is_group` 两个字段。

另有审查地址失效(B12):`0x42c4a4/0x43d334/0x43d4a4` 在本地反编译树中
**不存在**;本台账改用重新锚定的 `0x413dc8`(遍历)、`0x4141e4`(路径规范化)、
`0x414559`(路径查表)、`0x43d4bc`(层析构)。

---

## 1. A 类:手册 / 真实脚本冲突

| ID | 审查主张 | 复核证据 | 结论 | 决策 |
|---|---|---|---|---|
| A1 | `TID=""` 被归一化为 `:/W0` 并保留旧 `TA`,产生整屏过渡 | 脚本 `Button/es_Button.yst:4526/4540`(注释/调用语义=`初期化`)、`Sprite/es_Sprite.yst:4238/6111`;引擎 `FUN_00413dc8` 汇编 `0x413e7b: MOV EDX,[EBX+0x4c]; MOVSX ECX,byte[EDX]; TEST; JNZ 0x413e89`,空串直接 `XOR EDI,EDI` 跳过 TID 路径查询;非空才 `0x413e9a` 取 +0x4c、`0x413ea3 CALL 0x4141e4`、`0x413ebf CALL 0x414559` | **Confirmed** | **修复**:空 `TID` = 清除配对(`tid=None`),不得规范化为 `:/W0`;无 TID 时按普通层输出,不再产出 `to=None` 的过渡 op |
| A2 | 图像层 `SX/SY` 被丢弃,`FILE="" SX/SY` 黑幕变 1×1 | 手册 `cg/cg.html`「SX/SY…レイヤセットに対してのみ有効」;真实脚本 `es_ERIS.yst:579/600`(`es.CGSBLACK/CGTBLACK`: `CG[ID=$id … SX=@es.GSX SY=@es.GSY FILE=""]`)、`es_First.yst:170`、`es_Font.yst:220/231`;引擎 `FUN_00465730` 层默认 `+0x78/+0x7c=1`,图像装载 `FUN_0040af1c` 写 `+0xa6/+0xb0/+0x130`,`FUN_0043772c` 用 SX/SY 生成缩放纹理并写 `+0x78/+0x7c`,`FUN_00408724` 用 `+0x68/+0x70/+0x78/+0x7c` 作层矩形 | **Confirmed**(手册与引擎/脚本冲突,以引擎/脚本为准) | **修复**:图像层保存显式绘制尺寸;`SolidBlack1x1 + SX/SY` 按该尺寸绘制(可执行)。资源图 + SX/SY 无真实样本,保持 Unimplemented + DIV |
| A3 | `WINDOW` 域整体被忽略,主窗口 CGID/E/Z/X/Y 无效 | 手册 `window/window.html`「NO=0 がメインウィンドウ」「CGID=表示対象のレイヤセット」「E=表示/非表示」;脚本 `es_ERIS.yst:22`(子窗口 CGID)、`es_First.yst:140`(`WINDOW[NO=0 E=1]`)、`es_First.yst:97/98`(X/Y);`yu-rt` 把 `WINDOW*` 归 `Domain::Window` 事件,`GfxState::on_event` 只接 `Domain::Graphics` | **Confirmed** | **修复**:消费 `Domain::Window`;主窗口 `CGID/E/A/X/Y/SX/SY` 进状态,Screen pass 用主窗口图层组与可见性;子窗口状态输出给 host,叠放仍归 host(DIV-GFX-018) |
| A4 | `TEXT[ID/OUTPUTMODE/CLEAR/…]` 被丢弃,`TextRun.id` 恒空 | 手册 `text/text.html`「ID=テキスト自動出力モード時の出力するレイヤＩＤ」「CLEAR=システム内部のテキストバッファを完全にクリア」;脚本 `es_First.yst:22`、`Text/es_Text.yst:154`;代码 `yu-rt/src/vm/handlers.rs:1976-1992`(求值后丢弃)、`:2011/2035`(id 恒空) | **Confirmed** | **修复**:VM 保存 TEXT ID 并为后续 `WORD`/`_` 事件带上;`CLEAR=1` 清文本缓冲。WAIT/RETURN/PAGE/CLICK/AUTO*/BUTTON 属显示节奏,保持未实现并挂 DIV |
| A5 | `CGINFO/FONTINFO/WINDOWINFO` 查询不写 `LET` | 手册 `cg/cginfo.html`/`font/fontinfo.html`/`window/windowinfo.html` 均要求 `LET`;脚本 `es_ERIS.yst:629/630`、`Button/es_Button.yst:2380…`、`es_Font.yst:157`、`es_First.yst:126/133`;`yu-rt` 中 `CGINFO/FONTINFO/WINDOWINFO` 只走 `HandlerKind::Domain`,**没有 LET 写回**;`yu-gfx` 只在 `state.rs:319-332` 挂号 | **Confirmed**(审查引用的 `handlers.rs:2426-2451` 是 `domain_of`,与查询无关) | **部分修复(会话 F 续)**:`yu-rt` 增加 `GraphicsHost` 同步接口,INFO 在脚本执行期查询并写 `LET`;gfx 实现 `EXIST/E/A/X/Y/Z/MODE/T/TA/TID`、组/黑图尺寸、FONTINFO 当前样式、WINDOWINFO 主窗口几何。资源图尺寸/字体列表/鼠标/像素/多字段顺序保持 Unimplemented(DIV-GFX-024/025) |
| A6 | `CGACT SIZE/SIZE2` 的 SX/SY 丢失;`RESIZE/TRIM` 未识别 | 手册 `cg/cgact.html` 例 3/4/21/22:`SIZE=1 SX=315 SY=190`、`RESIZE=1 X=10 Y=20 X2=-30 Y2=-40`、`TRIM=1 SET=15`;真实 ysc 表含 `SX/SY/RESIZE/TRIM/ROTATION2/TRANSITION2/XOR/AND/BLUR2..6`;脚本 `es_ERIS.yst:583/603` 等;代码 `state.rs:819-832` 未取 SX/SY,`:1368-1397` 动作表缺项;引擎 `FUN_0043c971`(RESIZE: `w+X+X2-1`, `h+Y+Y2-1`)、`FUN_0043772c`(SIZE:`/100` 后重采样并写 `+0x78/+0x7c`) | **Confirmed**(参数契约;像素算法仍未取证) | **修复**:`ImageOp` 增加 SX/SY;动作表补 `RESIZE/TRIM/ROTATION2/TRANSITION2/XOR/AND/BLUR2..6`;全部继续标 Unimplemented |
| A7 | `FONT[NO/BNO]` 不生效 | 手册 `font/font.html`「NO=フォントリスト番号指定」(有页);BNO 手册无页;脚本 `Sprite/es_Sprite.yst:420/425/589` 使用 `FONT[BNO=1 …]`;代码 `state.rs:925-931` 只诊断、不保存 | **Confirmed**(NO 有手册;BNO 只有真实脚本) | **修复**:`TextStyle` 保存 `font_no`/`font_bno`;NO 可执行;BNO 保留值并挂 DIV(无可解析字体列表) |
| A8 | `CG` 无 `FILE` 创建鬼层,与手册“层不存在=错误”冲突 | 手册 `cg/cg.html` 例 4「レイヤが存在していない場合はエラー」;审查所引旁线 §3.24 的 get-or-create 事实本会话未能用 REA 重新锚定(CG 处理体仍在巨型 dispatch 中,函数边界缺失);真实 ERIS 未找到“无 FILE 更新不存在层”的可判定样本 | **Uncertain** | **保持现状 + DIV-GFX-016**:无纹理层不产生绘制,可见结果无差异;不猜错误/建层二选一 |
| A9 | MODE 3/5/6/7/9 与 4/8/10..18 被整层跳过 | 手册 `cg/cg.html` MODE 全表;首屏真实值只有 MODE 0(1015)/2(1);`FUN_00414ba4` 对 3/5/6/7/9 有固定管线状态但依赖 `DAT_00801d98/d9c` 辅助纹理;4/8/10..18 无固定管线分支;代码 `state.rs:1473-1502` 已分别标 `Unverified/Unimplemented` | **Confirmed**(限制存在) | **2026-10-30 收口**:3/5/6/7/9 已实现(DIV-GFX-001 `fixed`);4/8/10..18 经全函数体扫描确认官方**无分支**,归 `unsupported-by-official`(DIV-GFX-002),内核归约到通常路径绘制,不再整层跳过 |

---

## 2. B 类:REA / 引擎对齐

| ID | 审查主张 | 复核证据 | 结论 | 决策 |
|---|---|---|---|---|
| B1 | 每帧按 `(z,seq,id)` 重排是错的;引擎只走链表 | `FUN_00413dc8` 汇编:`0x413e12 MOV ESI,EAX`(头)、`0x413f4d MOV ESI,[ESI+0xc]`(next),循环内**没有任何排序调用**;手册 `basic/layer.html`「優先度の値が大きいほど手前に表示」「同じ優先度…不確定」 | **Refuted**(“重排是错的”不成立;引擎确实维护链表,但可见顺序 = Z 升序,与内核排序等价) | **保持**:同 Z 创建序是已记录的确定性决策(DIV-GFX-014);只做比较器去分配(E5),不改语义 |
| B2 | `E` 可能写 `+0x89` 而非 `+0xa4`;代码只有一个 `visible` | 遍历 `0x413e34 MOVSX ECX,byte[EBX+0xa4]`(0 跳过)、`0x413e47 MOVSX EDX,byte[EBX+0x89]`(0 跳过);`FUN_0042c400` 把待生效 `+0x8a` 写回 `+0x89`;`FUN_0040a38c` 释放 RT 后置 `+0xa4=1`;`FUN_0040e02c` PSB 装载置 `+0xa4=2`;`FUN_0042f809` 生成复制目标置 `+0xa4=1/2`;`FUN_0043d4bc` 析构置 `+0xa4=0` | **Refuted**:E=+0x89,层种类=+0xa4(1 图像/2 组/0 死);代码 `visible`/`is_group` 正是这两者 | **保持** |
| B3 | `+0x61c`/`+0x90` 可能不是 RT/合成纹理 | `FUN_00413dc8` `0x413f6f-0x413fab`:组(`+0xa4==2`)时 `PUSH [EBX+0x61c]` → `CALL [0x80143c]`(SetRenderTarget)、`CALL [0x801d5c+0xac]`(Clear)、`PUSH [EBX+0x21c]` → `[EAX+0x48]`(GetSurfaceLevel);`0x413fef` 清屏色 `0x808080` | **Refuted**:+0x61c 是组 RT,+0x21c 是纹理对象,+0x90 是纹理句柄 | **保持** |
| B4 | TFACTOR 只在 MODE 0/3/7,代码给 1/2/5/6/9 也填 AlphaGray | `FUN_00414ba4` 行 144-148 的分派集合明列 `0,1,2,3,5,6,7,9,100,98`;行 149-154 统一 `A<<24|A<<16|A<<8|A`;行 567(MODE 3)、754(MODE 7)分支内再覆盖 `RS(0x3c,0xffffffff)` | **Refuted**:审查只看了覆盖行;现有 `fixed_state()` 与引擎一致 | **保持** |
| B5 | 顶点 DIFFUSE RGB 未建模,可能整体色偏 | `FUN_0041a308` 顶点结构 `[0..2]=pos,[3]=1.0,[4]=diffuse,[5..]=uv`;diffuse 的 EDX 来源在 `0x415289 CALL` 前为 `[esp+0x2e8]`,该槽的赋值链在反编译中与 `local_c8/local_198` 重叠,未能可靠还原;模式 2/3/5/6/7/9 才在 color stage 使用 DIFFUSE,而这些模式已 Unimplemented | **Uncertain** | **保持 + DIV-GFX-019**:后端在 Unverified 模式下不得自行假定顶点色;可执行模式 0/1 的 DIFFUSE 只进 alpha stage,RGB 不影响可见结果 |
| B6 | MODE 3/5 stage1 白/黑辅助纹理缺失 | `FUN_00414ba4` 行 562/635:`SetTexture(1,DAT_00801d98/d9c)`;纹理内容未定位 | **Confirmed**(缺项存在) | **保持 Unimplemented + DIV-GFX-001** |
| B7 | `in_stack_00000010==0 → A=0` 未建模 | `FUN_00413dc8` 调用点 `0x413f29: MOV dword ptr [ESP+0xc],1` → 被调 `FUN_00414ba4` 的 `in_stack_00000010` 恒为 1;`0x413f31` 传 MODE(+0x87),`0x413f3c` 传 0 | **Refuted**:图层遍历不会触发 A 清零;该分支属于其他调用者 | **保持** |
| B8 | X/Y 单位可能不是像素原值(/100、16.16、-0.5) | 脚本 `es_First.yst:97/98`、`Button/es_Button.yst:945-954` 的 X/Y 是 0..屏幕宽高的像素值;`FUN_00408724` 把 `+0x68/+0x70` 与 `+0x78/+0x7c` 直接当整数矩形边界;`/100` 出现在 `FUN_00414ba4` 的 `+0x1b0..+0x1d8`(滤镜/仿射字段),不是 `+0x68/+0x70` | **Refuted**(对 X/Y 而言) | **保持**:X/Y 存 i64 像素;滤镜参数继续原值 + Unimplemented |
| B9 | 屏幕/组清屏为不透明黑/透明黑,引擎帧入口无 Clear | `FUN_0043cd6c` 帧入口只有 BeginScene/过滤状态/`FUN_00414ba4`/EndScene,无 Clear;唯一 Clear 在 `FUN_00413dc8` 的调试组分支(`DAT_00801750 && +0xa4==2`,色 `0x808080`) | **Confirmed**(无显式清屏) | **保持**:`Clear::default_black()` 继续标 Likely(DIV-GFX-010);不把调试色当正式语义 |
| B10 | 颜色空间 `GammaEncodedSrgb8` 是 Likely | 反编译全树无 `D3DRS_SRGBWRITEENABLE`(state 194)调用;无官方截图逐像素对拍 | **Confirmed**(等级就是 Likely) | **保持 + DIV-GFX-009** |
| B11 | COPY 全部 Unimplemented,但文档写“可执行复制”;pass 顺序 copy→image→组 | `state.rs:736` 所有 `CopyOp` 固定 `Unimplemented`;`state.rs:220-254` pass 按 copy_log→image_log 分组,不是命令交错序;`docs/render-parity.md` §2 写“1 个可执行复制” | **Confirmed**(文档与代码不一致) | **修复文档 + 测试**:§2 改“1 个可记录但不可执行的复制”;pass 顺序如实写成审计分组;不把 Unimplemented 复制改成可执行 |
| B12 | 旧地址 `0x42c4a4/0x43d334/0x43d4a4` 不存在 | `ls decomp | grep` 三者均无;`docs/render-parity.md:44/49/93-94`、`path.rs:9-10` 引用失效 | **Confirmed** | **修复引用**:改用 `0x413dc8`(遍历/TID/MID)、`0x4141e4`(路径规范化)、`0x414559`(哈希查表)、`0x43d4bc`(析构)、`0x42f57d`(SWAP)、`0x42f809`(CGACT 属性块) |
| B13 | 过渡 `T/TA/TID` 写侧/推进者未取证;TID 当层路径可能是假设 | TID 字符串在 `+0x4c`,遍历时按“当前路径 `[0x80170c]` + TID”经 `FUN_004141e4` 规范化后 `FUN_00414559` 查表(`0x413e9a-0x413ecf`);`+0x210` 同法用于 MID;`FUN_00414ba4` 行 881-971 双 pass、行 905-909 的 FadeVal 用 `+0x82/+0x84` | **Confirmed**(TID 路径/配对已取证;推进者仍未定位) | **修复 A1 的空 TID**;过渡仍整体 Unimplemented + DIV-GFX-006(推进者) |

---

## 3. C 类:硬编码 / A5

| ID | 审查主张 | 复核证据 | 结论 | 决策 |
|---|---|---|---|---|
| C1 | 视口尺寸未从 yscfg 装载,`from_tables` 忽略 `boot.config.width/height` | `state.rs:104-112` 只取 `image_ext_order`;`yu-format/yscf.rs` 的 width/height 有引擎校验(0x80..=0x2000);ERIS yscfg=800×600 | **Confirmed** | **修复**:`from_tables` 在宽高同时非 0 时覆盖 `GfxConfig.viewport`(仍允许调用方对 0 值回退) |
| C2 | 主窗口固定 `:/W0`,`WINDOW[CGID]` 不消费 | 同 A3 | **Confirmed**(与 A3 同一根因) | **修复**(随 A3) |
| C3 | `IMAGE_ACTIONS` 仅 29 项,缺 `ROTATION2/TRANSITION2/XOR/AND/TRIM/RESIZE/BLUR2..6` | 真实 ysc 表(support/ERIS)含这些参数;手册 `cg/cgact.html` 有 RESIZE/TRIM;代码 `state.rs:1368-1397` | **Confirmed** | **修复**:补表;动作仍全部 Unimplemented |
| C4 | 创建默认值 A=256/E=1/MODE=0/T=TA=0/FONT 白/antialias/Transition 0/0 无证据 | `FUN_00465730` 构造: `+0x89=1`(E)、`+0x80=0x100`(A)、`+0x78/+0x7c=1`、`+0x82=0x100`(T)、`+0x1c0=10000`、`+0x204=0x80`;MODE `+0x87` 零初始化=0;TA `+0x84` 无写入=0 | **Confirmed**(部分):E/A/MODE/T 有证据;TA=0 合理但未单独写;字体默认无证据 | **修复 T 默认=256**(仅设 TA 时 FadeVal 才正确);字体默认维持现状 + DIV |
| C5 | 扩展名 code 5 未映射 | `FUN_0040c9b4` 行 383 明确 `".psb"`;`yu-format/yscf.rs` 文档也写 5=psb;ERIS yscfg=`01 02 03 04 05` | **Confirmed** | **修复**:`ImageFormat::Psb`,扩展名 `.psb` |
| C6 | 域分类用 `starts_with("CG")/("FONT")`,不是 ysc 数据 | `domain_of` 只做路由,不解释参数;ERIS ysc 中 `CG*`/`FONT*` 均为图形命令,无同名冲突样本 | **Refuted**(不是可见语义错误;是启发式路由) | **保持**:若未来表出现非图形 `CG*` 再改为显式域表 |

---

## 4. D 类:缺失 / 未考虑项

| ID | 事项 | 结论 | 决策 |
|---|---|---|---|
| D1 | WINDOW 域 | Confirmed(见 A3) | 修复主窗口状态;子窗口只输出状态(DIV-GFX-018) |
| D2 | CGINFO/FONTINFO/WINDOWINFO LET | Confirmed(见 A5) | 部分修复:GraphicsHost 同步查询 + LET 写回;未取证字段保持 Unimplemented(DIV-GFX-020/024/025) |
| D3 | TEXT 管线 | Confirmed(见 A4) | 修复 ID/CLEAR;显示节奏未实现 |
| D4 | 图像层 SX/SY | Confirmed(见 A2) | 修复(FILE="" 黑幕);资源图尺寸挂 Unimplemented |
| D5 | SIZE/RESIZE/TRIM | Confirmed(见 A6) | 修复参数契约;像素未实现 |
| D6 | 多部件层 | Confirmed 缺项(`+0xa6/+0xa7/+0xb0/+0x130` 已定位,几何未取证) | 保持 Unimplemented(DIV-GFX-004) |
| D7 | MID 掩码 | Confirmed 缺项(TID 同法查表,MID=`+0x210`) | 保持 Unimplemented(DIV-GFX-005) |
| D8 | 组过渡 | TID 可指向组(查表结果 `+0x34`),gfx 已支持 `Offscreen` 纹理 | 保持(仍随过渡整体 Unimplemented) |
| D9 | 子窗口叠放 | 手册明确 1..3 为子窗口 | 归 host(DIV-GFX-018);gfx 只输出状态/离屏 pass |
| D10 | COPY 时序 | 旧线实测=上一帧已渲染目标;本会话未重做受控实验 | 保持 Unimplemented + DIV-GFX-007 |
| D11 | RECTPAINT 等图像加工 | 参数/命令已确认,像素算法未定位 | 保持 Unimplemented(DIV-GFX-013) |
| D12 | 文本栅格化 | 需字体会话 | 保持 Unimplemented(DIV-GFX-012) |
| D13 | LINT 语义 | 首屏 29,068 次;手册无页;REA:图层 i64 字段 `+0x58`(`0x42b71f`),按钮号 | 原样保留在 `extra`(DIV-GFX-011 已定案) |
| D14 | psb 扩展名 | Confirmed(见 C5) | 修复 |
| D15 | 主组缺失 fallback | 现实现“主组不存在→合成所有顶层节点”是测试便利,不是引擎证据;W0 在引擎启动时总存在 | 修复:显式主组缺失时 Screen 为空,不再猜测所有顶层 |
| D16 | 图像层下挂子层 | `FUN_00413dc8` 对每个节点先递归子节点(`0x413e1a-0x413e24`),与层种类无关;gfx 只在组节点递归 | **Confirmed** | 修复:图像层子层按引擎后序(子先父后)扁平绘制,不再静默丢层 |

---

## 5. E 类:健壮性 / 性能

| ID | 审查主张 | 复核证据 | 结论 | 决策 |
|---|---|---|---|---|
| E1 | 诊断永久累积、每帧 clone+sort、重复诊断被吞 | `state.rs:98/99` 两个无界容器;`FrameStart` 不清;`frame()` `clone+sort`;`push_diag` 只保留首条、无计数 | **Confirmed** | **修复**:诊断按帧清空;用有序 map 去重并累计 `count`;`frame()` 不再排序 |
| E2 | `image_log` 只增不减、每帧重放、retain 全量扫描 | `state.rs:859-871` push 不合并;`frame():242-254` 每帧遍历;`apply_cg`/`cgact_copy` 全量 retain | **Confirmed** | **修复**:日志设硬上限(16,384),超限丢弃最旧并计数 + 诊断;仍保留命令序(图像加工可能链式,不能按目标合并) |
| E3 | `Unimplemented` 粘滞,后续合法更新不恢复 | `state.rs:1348-1352` 只从 Executable 降级,从不恢复;`apply_layer_params` 直接改 `status` | **Confirmed** | **修复**:拆分 `texture_status`/`param_status`,每次属性更新重算;`FILE` 换图重置纹理状态 |
| E4 | 每命令多次小分配 | `resolved_owned` 对每个参数分配 String/Value | **Confirmed**(性能) | **挂账**:不改可见语义;本会话优先正确性,记录为后续优化 |
| E5 | `frame()` 无缓存、比较器内 BTreeMap 查找/id clone | `state.rs:287-292` `sort_by_key` 每次比较 clone id;`sorted_children:1125-1129` 每比较两次 BTreeMap 查找 | **Confirmed** | **修复**比较器:E5 目标(去比较期分配);整帧缓存不改 API 前提下挂账 |
| E6 | 三处无界递归(ensure_parents/group_has_visible/build_group_pass) | `state.rs:1057-1068/1133-1149/1152-1185` | **Confirmed** | **修复**:ensure_parents 迭代化;组遍历改显式栈后序;可见性判断改显式栈;深路径/深组测试 |
| E7 | 图像层可挂子层,子层静默不绘制 | 见 D16 | **Confirmed** | **修复**(随 D16) |
| E8 | 缺必需命令表不 fail-fast | `table.rs:66-79` 只查命令名非空;缺 `CG` 时运行期逐条诊断 | **Confirmed** | **修复**:`validate` 要求 `CG/CGACT/CGEND`,缺失即 `gfx.table.missing` |
| E9 | `fmt_f64` 整数值用 `{:.1e}`(128→1.3e2),NaN/Inf 非法 JSON | `json.rs:49-65` | **Confirmed** | **修复**:整数浮点 `128.0`;非有限输出 `null`;`-0.0` 保留符号;加测试 |
| E10 | CGACT 动作检测 O(29·P²) | `state.rs:1399-1416` 每个动作都 `flag()` 全表扫描 | **Confirmed** | **修复**:先建参数名集合,再查动作表 |

---

## 6. F 类:测试真实性

| ID | 审查主张 | 结论 | 决策 |
|---|---|---|---|
| F1 | golden 自生成,UPDATE 写后不断言 | Confirmed | 保留 UPDATE(唯一生成入口),新增 `tests/golden/README.md` 记录样本 sha256/生成命令;默认模式严格逐字节比较 |
| F2 | ERIS 首屏只断言 >100/>10/>=2 | Confirmed | **修复**:断言精确 layer/visible/pass/W0 ops/screen 内容 + 样本哈希 |
| F3 | 真实 MODE 2/A=0/E=0 未断言 | Confirmed | **修复**:首屏断言真实 `MODE=2`、`A=0`、`E=0` 图层存在且行为正确 |
| F4 | COPY2 未真执行;SWAP2 没断言 X/Y | Confirmed(测试覆盖不足) | **修复**:补 X/Y 交换断言;COPY2 属性不变已执行 |
| F5 | filter 的 ERIS yscfg 是硬编码数组;Other(5) 当期望 | Confirmed | **修复**:期望改 `Psb`;有样本时读真实 yscfg 校验 |
| F6 | blend_table JSON 未与 fixed_state 逐字段交叉验证 | Confirmed | **修复**:逐模式比对 JSON 中的 stage/arg/op/blend 码 |
| F7 | 无像素/官方截图对拍 | Confirmed(不在本会话范围) | 挂后端会话;本会话只保证语义/显示列表 |
| F8 | 缺样本默认 SKIP 仍算 passed | Confirmed(机制) | 保持:验收固定 `YURIS_REQUIRE_SAMPLES=1`;SKIP 打印原因 |

---

## 7. 修复清单(仅 Confirmed 且进代码)

1. A1/B13:`TID=""` 清配对;无 TID 不产出过渡 op。
2. A2:图像层显式尺寸;`FILE=""+SX/SY` 可执行。
3. A3/C2:`Domain::Window` 主窗口状态 + Screen 选择/可见性。
4. A4(yu-rt):TEXT ID/CLEAR。
5. A6:`ImageOp` SX/SY + 动作表补全。
6. A7:TextStyle `NO/BNO`。
7. C1:from_tables 使用 yscfg 宽高。
8. C4:Transition 默认 `T=256`。
9. C5:`Psb`。
10. B12:证据地址勘误(文档)。
11. D15/D16/E7:主组缺失不猜;图像层子层后序绘制。
12. E1/E2/E3/E5/E6/E8/E9/E10:健壮性/分配/递归/fail-fast/JSON。
13. F1-F6:测试与 golden 补强。

## 8. 保持 Unimplemented / 挂 DIV 的项

- A5(D2)CGINFO/FONTINFO/WINDOWINFO LET:跨层同步查询未设计。
- A8(D?)`CG` 无 FILE:引擎 get-or-create 未在本会话重新锚定,保持 DIV-GFX-016。
- A9 MODE 3/5/6/7/9、4/8/10..18:DIV-GFX-001/002。
- B5 顶点 DIFFUSE RGB:DIV-GFX-019(新增)。
- B9/B10 清屏/颜色空间:DIV-GFX-010/009。
- D6/D7/D10/D11/D12/D13:多部件/MID/COPY 时序/图像加工/文本栅格化/LINT,沿用原 DIV。
- E4 分配优化:记录,不在本会话做。

---

## 9. 会话 F 续发现(不在原审查清单,补入 TODO)

以下条目是复核/实现过程中新发现的非 1:1 点,原始审查未列;均未猜测实现,
已登记 DIV 与 `docs/gfx-unimplemented-todo.md`。

| ID | 缺口 | 证据/状态 | 处置 |
|---|---|---|---|
| DIV-GFX-030 | `CG[TIDNO]` 未处理 | 真实 ysc 表有 `TIDNO`;其与 `TID` 的组合规则未重新锚定 | 原样留档 + Unimplemented |
| DIV-GFX-031 | `CGACT ID3/ID4` 多图层、`Z2/BNO` 修饰 | REA:`ID/ID2/ID3/ID4` 是四个并列图层引用(`0x42c4ed/0x42c622/0x42c6df/0x42c798`);`Z2` 下标 8 **无读取点**;`BNO` 只被 `TEXT` 消费 | `Z2` 静默接受;非 TRANSITION 的 ID3/ID4 仍挂号 |
| DIV-GFX-032 | `SWAP/SWAP2` 与引擎模式 1/2 对应 | `FUN_0042f57d` 已读,但调用点模式值未逐项确认;现按手册实现 | 保持 Likely,不改 |
| DIV-GFX-033 | `TEXT OUTPUTMODE/WAIT/RETURN/PAGE/CLICK/AUTO*/BUTTON` | 手册 `text/text.html` 有页;属显示节奏/宿主输入 | 只记录事实,未实现 |
| DIV-GFX-034 | 文本跨帧持久 + `TEXT[CLEAR]` 传播到 gfx | `GfxState` 每帧清空 `text`;VM 的 CLEAR 未变成 gfx 事件 | Unimplemented(当前无文本栅格化,暂不可见) |
| DIV-GFX-035 | `FONT NO/BNO` 解析、GRADATION/ROTATION/PITCH/LANG、默认字体 | 手册只覆盖 NO/NAME 等;默认字体/颜色/AA 无一手证据 | 保存原值;解析归 `yu-font` |
| DIV-GFX-036 | FONTINFO 字体列表字段/未设置默认值 | `NUM/LANG/...` 需安装字体列表;COLOR2/NAME 未设置默认值未取证 | 不写 LET |
| DIV-GFX-037 | WINDOW host 属性、主窗口 A 透明度/SX-SY 视口 | 手册 `window/window.html`;0.495 的 A 是否仍仅显隐未实测 | 只消费 CGID/E/A 显隐 |
| DIV-GFX-038 | INFO 缺失目标非 EXIST / 未分类字段 | 手册只定义 EXIST;缺失层其他字段返回未取证 | 不写 LET |
| DIV-GFX-039 | `CGACT RASTER1/FSX/FSY/FX2/FY2/Z2` | 手册有 RASTER1,0.495 ysc 表**无**(引擎按下标取参 ⇒ 取不到);`FX/FY/X2/Y2/FX2/FY2/FSX/FSY` 像素/百分位双口径已取证 | `RASTER1` 已删;双口径已实现;`Z2` 无副作用(DIV-GFX-031) |
| DIV-GFX-040 | `TEXTINFO` 只读文本状态查询 | 手册 `text/textinfo.html`;VM 未接线,ERIS 未使用 | 随文本状态机一并实现 |

**结论**:原 TODO 不是全部;补齐后仍有 001..039 与 3 个无编号决策/开放项。
这些均已显式标记,不把“未实现”伪装成 1:1。

---

## 10. 任务 J 后续(2026-10-21)

- **EVID-J-01**:soft 的 alpha 源项从 `a` 改为 `a²`;soft/wgpu 半透明
  像素逐通道差 0。B5 的“MODE 2 diffuse=白”仍是独立未取证项。
- **EVID-J-02..05**:`RECTPAINT/RECTPAINT2/RECTPAINT3` 已实现;DIV-GFX-013
  从 unimplemented 改为 partial。首屏 10/11 image op 可执行。
- **EVID-J-08**:新增 DIV-GFX-008/041:主组 `W0` 官方直绘、子组 RT alpha
  不参与合成;内核仍用 `Offscreen(W0)`+RGBA8,半透明层与官方不同。
- **EVID-J-11**:旧线 `COPY` = 上一帧已渲染目标的结论 **Refuted**(至少
  group 源;命令执行时当前状态)。COPY 仍未实现,契约变更待主人确认。
- **DIV-GFX-042**:1px 半像素/quad 边界差异;RECTPAINT 区域内部已逐像素
  一致。
- 阻塞:当前 HEAD 的 `yu-rt` boot 链只到 8 visible / 18 op,未到 title;
  两条 golden 测试失败,未用 UPDATE 掩盖。详见 `PROGRESS.md` 成果 J1。
