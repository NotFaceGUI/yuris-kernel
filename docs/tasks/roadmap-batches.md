# 路线图 · 至待办清空(B1–B15,2026-10-24 定稿)

> 依据:`docs/audit-2026-10-24.md`(AU5)。主人令:
> ① **E-mote 暂缓**(B9 保留批次号,不排期);② **其余 SDK 面必须全部到位,
> 不允许任何条目终态为「未实现」**;③ 三层证据(官方 trace / REA 逆向 / 手册)
> 中手册最容易缺——**手册缺页 ⇒ 凭 REA/引擎证据实现并标注,不得以此挂账**。
> 本文件是全部剩余批次的唯一总规划;每批开工前另出任务书
> `docs/tasks/task-bN-*.md`,完成后**强制划台账**(AU5 起的纪律)。

## 0. 终态定义(什么叫做"待办清空")

全部 DIV / coverage / TODO 条目必须落入五种终态之一:

| 终态 | 含义 | 标注要求 |
|---|---|---|
| `fixed` | 手册+REA/引擎双证,或单 REA 证实现落地 | 常规 |
| `fixed(no-manual)` | **手册无页**,凭 REA/引擎证据实现 | 代码注释 + 台账行都写 REA 锚点与「手册无页」 |
| `aligned-stub` | 官方 0.495 处理体本身就是空桩/无操作,内核同型对齐 | 标注官方处理体地址(如 MIDI 族 `0x44aa40/60/80`) |
| `unsupported-by-official` | 取证证明官方同样不支持/静默回退,按官方行为对齐 | 标注分发点证据(如 MODE 4/8/10..18 的分发取证结论) |
| `deferred(master)` | 主人明令暂缓 | 目前仅 E-mote 家族 |

**禁止终态**:`unimplemented` / `blocked` / `open`。判定流程:每条挂账先问
「官方到底做不做」(REA 打开处理体)→ 官方做 ⇒ 实现(手册缺页就 no-manual
标注);官方空桩 ⇒ aligned-stub;官方也不支持 ⇒ unsupported-by-official。
**REA 反汇编本身就是证据层**——「三层缺层」不再构成无限期挂账理由。
`Severity::Unimplemented` 诊断只允许作为过渡态存在;AU6 复核(批次 B15)时
全仓 grep 归零(除 E-mote 域)。

## 1. 批次总表

| 批 | 主题 | 覆盖 | 依赖 | 规模 |
|---|---|---|---|---|
| B1 | 存档闭环(DIV-RT-042) | 任务书已出 `task-b1-save-loop.md` | 主人拍 §0 三契约点 | 中 |
| B2 | 宿主呈现与查询闭环 | DIALOG 接线、WINDOWINFO 字段族、CGINFO[COLOR]/组 ONMOUSE、FONTINFO 清单、INFO 多字段/缺失语义、CDDA 域一行 | — | 中 |
| B3 | CGS 截图栈(DIV-RT-050) | 菜单/存档背景透出 | B2(WINDOWINFO 候选根因)+ 官方事件 trace | 小 |
| B4 | debug_mode 注入链 | CLI 旗标 + 示例门控 + rt debug 分支取证 | — | 小 |
| B5 | VFS 挂载与名字 | update1..9 挂载、DirSource 非 ASCII 折回(DIV-003 归零) | — | 小 |
| B6 | 声音收口 | DATALEFT/DATARIGHT、负 START/RESTART、SOUND[SPEED]、ACM、MIDI/CDDA→aligned-stub | — | 小 |
| B7 | 宿主能力面 I | serve 补齐、WINDOW/TRAY、剪贴板写、REGINFO、文件选择框、SAVEINFO、存档快照/缩略图 | B1(存档部分) | 中 |
| B8 | 文本/像素 oracle campaign | 官方截图采集 → 056/057/042/009/010 定论 | 官方采集窗口(主人环境) | 小(实现随 B12) |
| B9 | **E-mote** | **deferred(主人令,暂缓)**;含 emote-parity §7/§8 对齐方向 | 主人重启 | — |
| B10 | rt 命令与系统变量零挂号 campaign | TASK/TEXT[ID]/DIALOG 扩展/FILEINFO 变体/TASKINFO>19/VARINFO STRTYPE/`_CHR` 族/ERROR/CSVINFO/VAR/前缀扫尾/表达式逐位/DIV-L-002 | — | 大(可拆两波) |
| B11 | gfx 命令面收口 | CGACT 加工剩余、031/039、TEXCOPY、022/016/027/030/032/028、011 extra 分类、037 | — | 大 |
| B12 | gfx 合成/像素取证收口 | 041/048/045/005/004/003/002(MODE 分发取证) | B8 取证 | 中 |
| B13 | ypf 容器全分支 + 格式收口 | NUL-XOR、ypf 边缘分支 ×5、YSTB v2、SNP、YSSD 配置块/pad | — | 中 |
| B14 | 媒体宿主(MOVIE/YMV 族) | 先取证空桩与否 → aligned-stub 或 MF 实现 | — | 中 |
| B15 | trace/工程终局 + **AU6 清空复核** | Platform 边界、CI、audit_hardcode 全仓、examples-audit、台账模板统一、清空宣告 | 全部批次 | 小 |

**波次建议**(可并行的分到不同会话):
- 第 1 波(立即,互不依赖):B1(等拍板)、B4、B5、B6
- 第 2 波:B2 → B3;B8 取证(需主人桌面/官方引擎环境)
- 第 3 波:B7(←B1)、B10、B11、B13
- 第 4 波:B12(←B8)、B14、B15 终局

## 2. 每批细案

### B1 · 存档闭环
见 `docs/tasks/task-b1-save-loop.md`(含三个契约裁决点与强制划台账清单)。

### B2 · 宿主呈现与查询闭环
| 条目 | 锚点 | 证据路径 | 终态 |
|---|---|---|---|
| DIALOG 宿主接线(S5) | 内核就绪 `yu-rt/src/vm.rs:1090-1130`;`yu-cli/engine.rs` 零消费 | 手册 `other/dialog.html` 有页;egui 模态 | fixed;S5 销账 |
| WINDOWINFO 字段族 | `yu-gfx/src/state.rs:1029-1046` | REA `0x4586ec` 逐字段返回源(FULLSCREEN/CAPTION/ACTIVE/MAXIMIZE/RESIZE/CLOSEMODE/MENUID/REGIONID/DRAGID) | fixed |
| CGINFO[COLOR] + 组/离屏 ONMOUSE | `state.rs:769-775, 797-802` | REA `0x43d664`;快照/组 CPU 像素面(推广 `wgpu/src/textures.rs:205-214`,顺带消 soft/wgpu 不对称) | fixed;DIV-GFX-026 归零 |
| FONTINFO NUM/LANG/NAME | `state.rs:956-977` | EnumFontFamiliesExA 公开语义作宿主注入(手册无页 → no-manual 标注) | fixed(no-manual);036 归零 |
| INFO 多字段写回顺序(025) | `state.rs:715-719` 等 | REA 三个 INFO 处理体的多字段处理序 | fixed |
| INFO 缺失目标语义(038) | `state.rs:728-733` | REA 缺失图层非 EXIST 字段返回值 | fixed/unsupported-by-official |
| CDDA 域归类一行 | `handlers.rs:3798-3800` | — | fixed |

验收:coverage-matrix WINDOWINFO/CGINFO/FONTINFO 三行 ✅;title→設定→存读档桌面全通;case 全绿。
划掉:DIV-GFX-025/026/036/038、coverage 三行、S5、H-P2-05。

### B3 · CGS 截图栈(DIV-RT-050)
取证:官方事件级 trace(进菜单/存档,`<side-repo>` 工具链,方法论见
`<repo-old>/docs/windows-handoff.md`);定论 = es.CGSSS/es.CGSDELALL 清栈缺失还是截图时机。
实施:按 trace 结论修内核/gfx。验收:进菜单/存档背景黑底与官方一致;用例哈希入 case。
划掉:DIV-RT-050(根 + yu-rt README 行)、audit §1 P0-2。

### B4 · debug_mode 注入链
CLI 五子命令加 `--debug-mode`;示例/测试环境门控注入(去硬编码 `false`);
rt debug 分支取证:官方 debug 构建 debug_mode=true 能进 title,本仓黑帧——
REA 对照 debug 分支行为(DIV-L-001 复核),按差异修。验收:census 口径与官方
debug 构建可比;`eris_first_screen` 双模式跑通。
划掉:todo-inventory §5 行 3(P0 挂两轮)、audit §1 P0-4。

### B5 · VFS 挂载与名字
- update1..9.ypf:`yu-cli/src/project.rs:160-177` 枚举 `update*.ypf` 按序 push
  (Mount 叠加语义已实现 `yu-vfs/src/mount.rs:122-138`);验收 = 带更新包样本工程资源齐。
- DirSource 非 ASCII:`dir.rs:126-151` 用 `yu-format::codec` 把 Unicode 名折回
  CP932/GBK 字节键;验收 = 0.479 SDK 散盘 1,424 文件 **0 skipped**。
划掉:DIV-003 → fixed、audit §2 行。

### B6 · 声音收口
| 条目 | 锚点 | 路径 | 终态 |
|---|---|---|---|
| DATALEFT/DATARIGHT | `yu-sound/src/state.rs:826-844` | N0-12 已取证,**直接实现** | fixed(no-manual) |
| 负 START/RESTART | `state.rs:627-647` | 字节延迟累加器 `+0x190/+0x194` 已取证;先实现静音区间+TIME 冻结,DIV-SND-003 动态样本后校准 | fixed(标注 Likely 项) |
| SOUND[SPEED=] | `state.rs:535-539` | REA yscm 槽 13 处理 | fixed 或 unsupported-by-official |
| ACM WAVE(CODEC=2) | `decode.rs:157-176` | REA 官方是否走 ACM API:走 → win32 ACM 实现(手册页「不支持」系过时,no-manual 标注);不走 → unsupported-by-official | 二选一,不留 unimplemented |
| MIDI/MIDIEND/MIDIINFO/CDDA | `0x44aa40/60/80`、`0x429fb0` | 官方空桩(N0-9/10 已证) | **aligned-stub**(状态改写,行为不变) |

划掉:DIV-SND-002/004、sound-ledger 相应行、coverage §8 口径注。

### B7 · 宿主能力面 I
- serve 补齐:panic 防线(对齐桌面 `panic_guard`)、`--save-root`、实时步长上限
  (H-P1-05,首帧 ~1.8s 停顿)。销 H-P1-01/05、symptom P1-A/B。
- WINDOW/TRAY 宿主:caption(`vm.rs:1083` 请求端已有)接 win32 窗口标题;
  托盘图标(手册 tray 页);主窗口 A 透明度 0.495 语义 + SX/SY 视口取证
  (DIV-GFX-037 收口)。
- 剪贴板写:CLIPACT[SET] → OS 剪贴板(DIV-RT-045 收尾)。
- REGINFO:注册表读取宿主能力(REA 语义;语料 0 次)。
- 文件选择框:win32 打开/选夹对话框,供 DIALOG[FILEOPEN/FOLDER](B10 接参数)。
- SAVEINFO(REA `0x44e1c8`)+ 存档快照/缩略图(H-P4-02:SAVE/LOAD[ID] 图层
  快照、FILEACT BMP/PNG/JPG 导出;gfx CGETSCREEN 类能力)←B1。
划掉:H-P1-01/02/04/05、P1-A/B、DIV-GFX-037、DIV-RT-045、H-P4-02。

### B8 · 文本/像素 oracle campaign(取证批)
官方引擎截图采集(64 位 Python 驱 32 位引擎,WOW64;`capture_screen.py`):
① 首帧(复核 051 锚点 + 010 清屏色);② 对话多行/自动换行场景(056/057、
yu-font ascent/行距/基线);③ 1px 边缘(042 半像素);④ 色彩阶梯(009 颜色空间);
⑤ 组嵌套半透明(041)。产出对拍基准 → B12 实施。056/057 依据截图改完即
fixed(现 Likely 项全部定论)。

### B10 · rt 命令与系统变量零挂号 campaign(可拆两波)
每条 = REA 定位处理体 → 实现(no-manual 标注)或官方对齐;手册有的直接实现。

| 条目 | REA 锚点/路径 | 归档 |
|---|---|---|
| TASK[TEXTPOS/RESTART]、序号>35 | `0x4506f4` | ✅ fixed(no-manual):RESTART 已实现(`0x450c46/0x450c55`);TEXTPOS(槽 36)官方无分支 → 对齐 no-op(DIV-RT-015) |
| TASKINFO 序号>19 | `0x4523fc` | ✅ fixed(no-manual):A(4)/FILE0/FILENO0/LINE0/TEXTLINE/NEXTVOICE 已实现;FILENO0→SCRIPTPOS fallthrough;SCRIPTPOS 引擎字节格式(`0x452a54`/`0x450fe0`);GOLABEL/GOSUBLABEL/TEXTPOS 官方无分支 → no-op(DIV-RT-025) |
| TEXT[ID] 事件字节契约 | `VmEvent::Text.id → Vec<u8>`(跨 yu-trace/gfx) | DIV-GFX-021、H-P2-01 |
| DIALOG 扩展参数族(STR2..8/A/E/CHECK*/FILEOPEN/FOLDER/CHECKBOX/SLIDER/NOWAIT) | `0x43fc74` + 手册 dialog.html(宿主框接 B7) | DIV-RT-044 残项归零 |
| FILEINFO FOLDER/SIZE/RELATION | `0x44286c` | — |
| VARINFO STRTYPE/SJISCODE/UTF8CODE | var getter 反汇编 | DIV-RT-011 余项 |
| `_CHR/_VARTYPE/_SYSTEM_FRAME/_SYSTEM_TIME` 族 + 系统变量写入语义 | `0x4276xx` getter 分发表(`_CHR` 静态 560) | DIV-RT-007 归零(no-manual) |
| `_OS_TIME(0)`、`$_PROJECT_PATH` | REA 取值链 | DIV-RT-009/010 余项 |
| ERROR 启用表消费者 | REA 定位读取点 | DIV-RT-046 |
| CSVINFO、VAR | REA(手册无页 → no-manual) | DIV-RT-011 归零 |
| 未知变量前缀扫尾 | 前缀分派全表 | DIV-RT-017 余项 |
| 表达式逐位/除零策略/错误继续策略/任务相位 | 官方 trace(DIV-RT-002/003/005、DIV-L-002) | 归 zero |
| 静默忽略面(SYSTEMINFO 槽>5、ERROR NO/SET)转显式对齐 | `handlers.rs:2177,3187` | 策略落档 |

终态:coverage-matrix ⛔/🔁 清零;DIV-RT open 全清。

### B11 · gfx 命令面收口
- CGACT 加工剩余:ROTATION/RESIZE/TRIM/MERGE/MERGE2、ID2 显式目标、区域变体
  (`op.rs:197-201`、`state.rs:2048-2108`)— REA 逐动作像素算法;DIV-GFX-013 归零。
- ID3/ID4/Z2/BNO(031)、RASTER1 族(039)。
- TEXCOPY/TEXSSCOPY(`state.rs:1712-1713`;REA 渲染到纹理路径)— 007 尾项归零。
- 资源图 SX/SY(022)、CG 无 FILE get-or-create(016 重新锚定)、CG[TIDNO](030)、
  IDNO 拼接(027)、SWAP/SWAP2 引擎模式(032 `FUN_0042f57d`)、图像层子目标(028)。
- extra 参数全分类(011:LINT/LINT2/TSX/TSY/MIPMAP/TEX/DXBUF/DXDRAW/RID/MFILE/CASH)。
- WINDOW 属性 gfx 侧(037;与 B7 宿主配套)。
- 官方首帧截图复核 051(随 B8)。
终态:gfx-unimplemented-todo 的 unimplemented/open 全清(留 decision/note)。

### B12 · gfx 合成/像素取证收口(←B8)
- 041 子组 RT alpha(生产 RT 格式/写掩码 REA);042 半像素、009/010(依 B8 截图修)。
- 045 过渡 TA 残差(mask 生成路径 REA);048 组 TID 转场(YSCom 离线 CLI 编译
  合成微场景,绕开 attach 阻塞)。
- 005 MID 外部掩码(通道/尺寸 REA);004 多部件几何(`+0xa6/+0xa7/+0xb0/+0x130`);
  003 FX/FY 仿射(槽位 `+0x1b0..+0x200` 绑定 + 公式)。
- **002 MODE 4/8/10..18**:**已结案(2026-10-30)** —— `FUN_00414ba4`
  全函数体(9804 B)扫描:只有对 `1/2/3/5/6/7/9` 的 `CMP`,这 11 个值不设任何
  D3D 状态、也不因 MODE 进入 HLSL effect 块(`0x416ad1` 是 effect 指针读取,
  按转场条件进入)⇒ `unsupported-by-official`,内核归约到通常路径。
  证据见 `docs/shader-parity.md` §3.2 / `docs/render-parity.md` §5.3。
终态:DIV-GFX 全表归五种终态。

### B13 · ypf 容器全分支 + 格式收口
- NUL-XOR 方言(DIV-002):REA ypf 名解码分支 → 实现 + 自造样本往返测试。
- ypf 边缘分支逐个 REA:`version>495`(`ypf.rs:327`)、`entryCount=0`(342)、
  `isCompressed>1`(617)、`name key≠0xFF`(705)、MurmurHash 多段续读(480)。
- YSTB v2(DIV-007/009):REA 0.495 装载器的 v2 分支(无样本也有代码)+
  旧 SDK/开源工具合成样本;实现 + 往返测试。
- YSSD:SNP 形态(`yssd.rs:16`)、btype1 配置块段 A/B 字段语义(441)、
  btype2 pad 0x0100(485)— REA save loader 分支。
终态:fmt/vfs 零 unsupported(no-manual 均标注)。

### B14 · 媒体宿主(MOVIE/YMV 族)
先取证 0.495 的 `MOVIE(0x44b134)/YMV(0x458ee0)/YMVEND/YMVINFO(0x459b98)`:
空桩/外挂 DLL ⇒ aligned-stub(标注);真播放 ⇒ Windows Media Foundation 宿主
实现(手册 movie.html 有页)。DIV-RT-045 媒体侧收尾。

### B15 · trace/工程终局 + AU6 清空复核
- Platform 命令边界记录(yu-trace M3)。
- CI:`case --all` + `fatal_codes` 断言(现门禁靠手跑)。
- `audit_hardcode.py` 全仓化(vfs/trace/cli 模块感知);examples-audit B/C 组执行。
- 台账状态行模板统一(todo-inventory §6.1 遗留),todo-inventory 重写为机读。
- **AU6 清空复核**:重跑 audit §7 扫描——`Unimplemented` 诊断挂账点归零
  (E-mote 域除外)、open/blocked/unimplemented 状态行归零 → **宣告待办清空**,
  PROGRESS 记成果。

## 3. 标注约定(no-manual / aligned-stub 怎么写)

- 代码注释统一形:`// REA 0xXXXXXX;手册无页(2026-10-24 政策:按 REA 实现并标注)`。
- 台账状态列用 `fixed(no-manual)` / `aligned-stub` / `unsupported-by-official`,
  影响列首句写证据锚点。
- `Severity::Unimplemented` 不再用于"手册缺页";仅官方行为冲突待裁决的过渡期使用。
- PROGRESS 成果条目引用锚点,便于回查。

## 4. 执行纪律(每批相同)

1. 任务书先行(`task-bN-*.md`)→ 主人过目 → 实施;跨 crate 契约先拍板。
2. 零 git 写操作;证据产物只放 `target/tmp`;`src/` 无 stdout 宏。
3. 验收含该批"清零 grep"(例:B10 后 `grep -c "STRTYPE" crates/yu-rt -r` 类
   逐条目核销);全批结束重跑 AU5 §7 扫描对比。
4. **强制划台账**:任务书里列明本批要改的每一行;不划不算完成。
5. 取证推翻任务书假设 ⇒ 先改档案再继续,禁止按错误前提写码。
