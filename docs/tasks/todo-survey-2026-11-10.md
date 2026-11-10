# 全项目待办普查(2026-11-10)

> 目的:在「取证 + 高性能实现」会话开工前,给出**当前 HEAD** 的真实待办面。
> 本文件是普查快照,不替代各台账;状态以 `PROGRESS.md` 与各 DIV 档案为准。
> 口径:官方引擎可观测行为 > SDK 0.495 逆向事实 > 原版数据自证 > 旧线 golden。
> 本文件只列**仍未关闭**的项;已 fixed / withdrawn / explained 的不再重复。

---

## 0. 本次实测基线(先看结论)

| 检查 | 命令 | 结果 |
|---|---|---|
| 全工作区编译 | `cargo check --workspace --all-targets` | **通过**(exit 0) |
| 引擎级 900 帧诊断(完整宿主) | `cargo run --release -p yu-cli --example game_diag_probe -- <project-eris> 900` | **仅 `EMOTE:12`**(deferred);`script_errors=0`;末帧 `ops=13 skipped=0`;`missing_assets=0` |
| 裸 VM 600 帧普查 | `cargo run --release -p yu-rt --example coverage_census -- 600 8` | 只剩「未装宿主」口径的域命令 + `EMOTE` + census 未注入 `_SAVE_PATH`;无其它命令挂号 |
| 同源一致性 | `cargo test -p yu-rt --test consistency`(设 `YURIS_SAMPLE_ERIS_YSBIN`) | **4 passed** |
| 核心测试面 | `cargo test -p yu-rt -p yu-engine -p yu-gfx -p yu-render-core -p yu-backend-wgpu --no-fail-fast` | **全绿**(`eris_first_screen` 已在 2026-11-10 修基线;rt 94+4+11+4+118+3、gfx 全绿、wgpu 16+27+39+1+1) |
| golden 用例 | `cargo run --release -p yu-cli --bin yu-cli -- case <project-eris> --all` | **9/9 PASS**(2026-11-10 修基线 + 补导航,见 §1 G0) |

**结论**:主线 VM/gfx 语义已基本收口;剩余工作集中在
①取证缺口(像素/文本/过渡/格式边缘)、②宿主能力面、
③性能 T4 与残余尖峰、④台账与 CI 一致性。
没有发现 panic/crash 级缺陷。

## 0.1 本次已完成(2026-11-10)

- G0:golden 9/9 与 `eris_first_screen` 双模式修绿(基线过期 + 补全导航);
- `SAVEINFO`/`VAR` = 官方 no-op(`0x44e1c8`/`0x453f88` 的
  `xor eax,eax; pop ecx; ret`);
- `REGINFO` = `RegistryHost` 端口 + 桌面 Windows 实现;
- `FILEINFO[FOLDER]` = 散盘目录 `FILE_ATTRIBUTE_DIRECTORY` 检查;
- `CGINFO[FX/FY/FBX/FBY/FEX/FEY/FEZ]`、`FONTINFO[PITCH]` 落地。

---

## 1. P0 · 必须先处理

### G0 golden 用例 9/9 失败(需判定「基线过期」还是「真回归」)

实测(soft,800×600,默认 470–600 帧):

| 用例 | expected ops | actual ops | 备注 |
|---|---:|---:|---|
| `title-config-back` | 96 | 13 | 末帧回到标题(13) |
| `save-title-flow` | 96 | 13 | 无输入,末帧标题 |
| `symptom-enter-story` | 122 | 33 | 点 START 后未进入剧情态 |
| `symptom-ingame-config` | 404 | 38 | |
| `symptom-ingame-save` | 159 | 57 | |
| `until-visible` | 85 | 75 | 首可见帧少 10 op |

事实与线索:

- `cases/` 最后一次基线更新 = `2550a44`(2026-10-26);此后有 30+ 行为提交
  (文本字节 ID、DIALOG、MOVIE、帧契约、表面池、TRIM、VM 预解码等),
  **基线必然过期**。
- 标题按钮实测位置(soft,`probe_buttons`):`START y=270/300`,`CONFIG y=360`,
  `BACK` 在配置页 `(726,554)`;`title-config-back` 的第二次点击 `(750,565)`
  现在**能命中 BACK** 并回到标题 ⇒ 当前行为比基线更符合用例名语义
  (基线很可能冻结了「BACK 不生效」的旧缺陷)。
- 但 `symptom-enter-story` 只到 33 op,说明点击 START 后**没有进入剧情**;
  可能同属基线语义变化,也可能是真缺口。必须逐用例判定,不能直接 `--update`。
- `save-title-flow` 有 `save:true`,依赖 `target/tmp/yssd-corpus` 播种;
  本机该目录不存在时存档摘要与菜单状态都会不同。
- **同类红测**:`yu-gfx --test eris_first_screen` 的
  `eris_first_screen_display_list` / `..._with_synchronous_info_queries`
  在 HEAD 失败:`layers` 期望 272、实际 **199**。该断言最后一次更新在
  J3/`7ec23d5` 时期;`9aca9fa` 落地 `DIV-GFX-016`(CG 无 FILE/RID
  **不建节点**)后层数必然下降,但测试 golden 没同步 ⇒ 高度疑似
  「基线过期」而非引擎回归。需按同一流程复核 visible/passes/ops 后重生成。

**处置**:
1. 对每条用例记录「点击后屏幕层清单 + 关键变量」,判断实际状态是否官方合理;
2. 对确认属于**有意行为变更**的用例,重生成基线并在 PROGRESS 注明理由;
3. 对 `symptom-enter-story` 这类「该进剧情没进」的,先当回归查
   (输入时间线 / 标题按钮命中 / 菜单脚本分支);
4. 基线重生成必须同时留像素截图,不能只更新哈希。
5. 建议增加 `case --update` 的人工审阅清单(哪些哈希变了、为什么)。

**收口(2026-11-10,✅)**:
- `eris_first_screen` 判定为基线过期:`9aca9fa`/DIV-GFX-016 后 272→199、
  8→7 可见;`passes/ops/screen_ops` 全不变,`first-screen` 类 PNG 哈希
  逐字节未变 ⇒ 被删 73 层无绘制贡献。已更新测试 census + display golden。
- 9 条 case 的根因是**旧输入只点 START**,而当前 START 进入 β4 功能
  选择菜单。已补全导航(`START → β４機能紹介 → 画像表示関連機能 →
  フェイスウィンドウサンプル`),并分别验证 設定/セーブ/QS/QL 的真实
  屏幕;`first-screen` 类 ops 85→75 是引擎帧内清掉 10 条无消费者
  texture pass 的等价结果(像素哈希不变)。
- 结果:`eris_first_screen` 双模式 4/4、`case --all` 9/9 绿。

### G1 测试门禁未进 CI

- `.github/workflows/` 原先只有 `release.yml`;2026-11-10 已新增
  `ci.yml`(windows-latest):`cargo build --workspace --all-targets` +
  8 个 crate 的 `--lib` 免样本测试(实测全绿)。
- B15 仍要求:`case --all` + `fatal_codes` 断言进 CI。该门禁需要真实
  语料,当前 runner 无 `<project-eris>`;不能空跑,留给自托管/受保护
  artifact 的样本 job。
- 缺样本的测试当前是「打印 SKIP 后 panic」,直接跑会失败;CI 必须显式
  提供 `YURIS_SAMPLE_ERIS_YSBIN/ROOT` 或把无样本环境标成 skip。

---

## 2. P1 · 取证缺口(缺官方证据,不能猜)

> 这些项都有明确 REA 锚点或已定位机制,差的是**官方截图/trace/逐行反编译**。
> 本会话应优先做这一组。

### 2.1 渲染 / 文本像素

| 编号 | 缺什么 | 现状 | 建议取证 |
|---|---|---|---|
| `DIV-GFX-042` | D3D9 半像素/quad 相位(`-0.5` 网格初值) | 整数矩形;1px 边缘与官方不同 | 官方 1px 边框/四角微场景截图,逐像素对拍 |
| B8 oracle | 官方首帧/清屏色/颜色空间/文本截图 | 051/010/009 已 fixed(no-manual),仍缺逐像素确认 | `capture_screen.py` 官方采集;形成对拍基准 |
| `DIV-GFX-075` | 文本「待显示行缓冲 vs 图层表面」、`'P'` 清页、`FUN_00404c60` 折行、每字符样式 | 累积显示缓冲 + flag 换行 + 单样式 | 官方最小脚本 + 截图/trace(H-P2-03) |
| `DIV-GFX-076` | `CGACT[TEXT]` 与图像加工的**通用命令序** | 外字链已 fixed;通用仍层末合成 | A/B 脚本 + REA `FUN_0042f809` 表面读写时序;或主人裁决 |
| `DIV-GFX-056` | 行距/基线/自动换行 | `行高=SY`、基线=顶+ascent(Likely) | 官方文本截图(H-P2-03) |
| `DIV-GFX-057` | 无文本图层时的屏幕叠加/层序/裁剪 | 全视口 overlay(Likely) | 官方最小脚本截图 |
| `DIV-GFX-045` | 过渡 TA≠128 的 mask 生成路径 | TA=128 校准,残差 ≤8/255 | 配对 mask 纹理生成路径 REA(`FUN_00414ba4`) |
| `DIV-GFX-005` | **组图层** MID 掩码 | 图像层 fixed;组仍只画基图 + recoverable | 组/离屏真实像素面 provider |
| `DIV-GFX-007` | `TEXCOPY/TEXSSCOPY` 像素语义(PASTESPRITE/GETSS) | 保留原纹理 + 审计项 | 官方微场景 + `0x42e67f/0x42e68a` 下游 |
| `DIV-GFX-022` | 资源图 `SX/SY` 缩放滤波/裁剪 | 已按矩形缩放,滤波未取证 | 官方截图/微场景 |
| `DIV-GFX-004` | 多部件层几何(`+0xa6/+0xa7/+0xb0/+0x130`) | `Part.raw` 原样保存 | 逐字段 REA + sprite 微场景 |
| `DIV-GFX-060` | `CG[FILE=""]` 像素 alpha | 内核 `[0,0,0,255]`;REA 为 `[0,0,0,0]` | 官方截图逐像素裁定 alpha |
| `DIV-GFX-072 #8` | 加工链 → COPY → CGEND 的中间像素 | 源只记基图,回放可能丢中间加工 | 构造用例 + 两后端对拍 |
| `DIV-GFX-024` | PSB / 未注入资产尺寸 provider | 图像头 provider 已有;PSB 缺 | 资产头解析(E-mote 暂缓,可先只做尺寸) |
| `DIV-GFX-011 RID` | 资源图(`RID`)取图 provider | `state.rs:6729` 缺 provider | REA 资源图装载链 + 语料频度 |
| `DIV-GFX-026/038` | 组/离屏 `COLOR`、缺失目标非 EXIST 字段、`CGINFO` 其余字段 | catch-all `gfx.info.unsupported` | REA `0x43d664` 逐字段返回源 |
| `DIV-GFX-070` | `CGINFO[COLOR]` 半像素口径 | 代码已实现,档案状态不一致 | 统一台账口径(见 §6) |
| render-parity | UV 平铺/镜像(`DAT_00801334` 1/2/3) | 未实现 | 全局显示缩放档 REA |
| render-parity | 主窗口 `+0xa4==2` 的 FRZ/FEX/FEY/FMD 变换 | 未实现 | 主窗口分支单独取证 |
| `EVID-J-06` | `+0x86` alpha 标志绘制路径 | 只记录不建模 | `FUN_00408b1d` 完整分支 |

### 2.2 rt / 表达式 / 系统变量

| 编号 | 缺什么 | 现状 | 建议取证 |
|---|---|---|---|
| `DIV-RT-002` | 表达式类型提升/溢出/字符串↔数值**逐位** | 确定性规则,未逐位对齐 | 官方表达式 trace / 逐 opcode 反编译 |
| `DIV-RT-005` | 非默认槽脚本错误后的继续策略 | 内核挂号后继续 | 官方错误脚本 trace |
| `DIV-RT-009` | `_OS_TIME(0)`、跨 DST | 未注入/未对齐 | 官方 getter + 宿主注入口径 |
| `DIV-RT-046` | `ERROR` 逐命令启用表的**消费者** | 表已写,消费者未定位 | xref 全表 + 官方错误路径 trace |
| `DIV-L-002` | script 60 返回边界后的**任务相位/调度** | open(中);官方场景任务 vs 内核菜单任务 | 官方事件 trace `diff_traces --order-only --script 60`;与 §2.3 影片态调度同源 |
| `VAR` | ~~手册无页、官方处理体语义~~ | ✅ **fixed(2026-11-10)**:官方 no-op(`0x453f88` 守卫后 `xor eax,eax; pop ecx; ret`) | `fixture::saveinfo_and_var_are_official_noops` |
| `CSVINFO` | CSV 表格式/解析/LET 写回 | `Unknown`;官方处理体 `0x43e660` 已定位,CSV 表解析语义仍未取证 | 继续 REA `0x43e660` + `0x40d828/0x40db04` |
| `SAVEINFO` | ~~查询处理体~~ | ✅ **fixed(2026-11-10)**:官方 no-op(`0x44e1c8`,不读参数/不写 LET) | `fixture::saveinfo_and_var_are_official_noops` |
| `REGINFO` | ~~注册表读取宿主能力~~ | ✅ **fixed(2026-11-10)**:`RegistryHost` 端口 + 桌面 Windows 实现;失败写 0/0.0/空串,成功只写字符串目标 | `fixture::reginfo_reads_host_registry_and_matches_official_write_rules` |
| `FILEINFO[FOLDER]` | ~~封包内文件夹查询语义~~ | ✅ **fixed(2026-11-10)**:散盘 `FindFirstFileA` + `FILE_ATTRIBUTE_DIRECTORY`;`SIZE` 0、`RELATION` 默认值、`..` 拒绝 | `fixture::fileinfo_folder_checks_disk_directory` |
| `WINDOWINFO` 字段 | `MAXIMIZE/RESIZE/MENUID/REGIONID/DRAGID` | catch-all unsupported | REA `0x4586ec` 逐字段返回源 + 宿主动作 |
| `FONTINFO` 字段 | `NAME`(未设置)、`PITCH/X/Y` | unsupported | REA `0x444cfc` |
| `DIV-RT-050` | CGS 截图栈(菜单背景透出上一亮帧) | **deferred(master)** | 官方事件 trace;主人重启后做 |

### 2.3 MOVIE / 影片态调度(B14 遗留)

- `\MOVIE(...)` 宏只设 `@es.SCENARIOSTOP=2`,真正播放由
  `ES.GAMEMAIN.LOOP`(`es_Scenario.yst:127..162`)在下一帧
  `IF[@es.SCENARIOSTOP==@e.MOVIE]` 调 `es.S.GOSUB("es.MOVIEPLAY")`。
- 实测该消费未发生:场景任务在 `WAIT[FRAME=1]` 后先于系统任务继续,
  `\TITLE` 宏把 `SCENARIOSTOP` 改回 1。
- 需要:按手册「Z 高先执行」(系统任务默认 100)复核任务链顺序与
  `WAIT[FRAME=N]` 让出语义;或官方 trace 定案。修好后宏路径直接播片,
  不再需要脚本侧绕行。

### 2.4 格式收口(B13)

| 项 | 缺什么 | 建议 |
|---|---|---|
| `DIV-002` YPF NUL-XOR 方言 | 无真样本 | REA 名解码分支 + 自造样本往返 |
| `DIV-007/009` YSTB v2 | 无真样本 | REA 0.495 装载器 v2 分支 + 旧 SDK/开源工具合成样本 |
| YPF 边缘分支 | `entryCount=0`、`isCompressed>1`、`name key≠0xFF`、MurmurHash 多段续读 | 逐分支 REA |
| YSSD | SNP 形态、btype1 配置块段 A/B、btype2 pad 0x0100 | save loader 分支 REA |
| `DIV-RT-056` | 嵌套 DNO1024 第二段负载 | 官方存档样本 + REA |

### 2.5 声音

| 项 | 缺什么 |
|---|---|
| `DIV-SND-003` | 官方动态探针(LOOPCOUNT/TIME 逐帧曲线)未取得;探针未进脚本、断点未命中 |
| `DIV-SND-002` | 负 START/RESTART 的「延迟期 TIME=0/PLAY=1/静音」动态校准;`+0x190/+0x194` 混音线程消费点未逐行取证 |
| N5 | 设备输出(cpal)不进 golden;任务书未收口 |

### 2.6 字体(影响文本像素)

- `DIV-FONT-001`:`FONT[NO]` 列表顺序(GDI 枚举 vs family 名排序)。
- `DIV-FONT-002`:GDI outline/hinting vs swash/skrifa,未宣称字节级一致。
- `DIV-FONT-003/004`:`GRADATION/ROTATION` 0..8 方向映射未取证。
- `DIV-FONT-006`:合成粗体 `(sy*0.04).clamp(0.5,2.0)` 是 Likely。
- `DIV-FONT-009/010/011`:本地化别名、GDI 度量 vs swash advance、ascent 取整。
- REA 待办:①文本合成器是否读 GRADATION/ROTATION;②`0x4450c0` 的
  `lfHeight/SX/SY`;③GDI 合成粗体精确像素;④`FONT[NAME]` 找不到的回退。
- `gdi-reference` feature 仍是占位,未实现。

### 2.7 E-mote(暂缓,只登记)

- `deferred(master)`;`yu-emote` submodule 当前 `1b46604`,而
  `docs/emote-parity.md` 写 `d0f03a1` —— 文档与子仓不一致,待主人核对。
- 内核无 crate 依赖 `yu-emote`;`EMOTE` 13 次仍只发域事件。
- M1 规范化/M2 动画/M3 wgpu 管线/M4 官方截图对拍均未做。
- `SYSTEMINFO[EMOTE]` 临时硬编码 0。

---

## 3. P2 · 宿主能力面(B7)

> 这部分多数**证据已够**,是平台实现,不是语义猜测。

| 项 | 现状 | 动作 |
|---|---|---|
| 主窗口 Z/RESIZE/A、全屏切换 | `WindowState` 存值;**host-desktop 无 `SetWindowPos(HWND_TOPMOST)`/resize/分层/全屏消费** | win32 窗口动作 + `WINDOWINFO` 回读 |
| `MINIMIZE/CAPTION/MAXIMIZE` | VM/Engine 有 setter;host 仅 close 消费,**minimize/caption/maximize 无宿主消费者** | 接 winit 窗口动作 |
| 托盘 `TRAY` | 只发域事件 | win32 托盘图标 + 菜单 |
| `CLIPACT[SET]` 写 OS 剪贴板 | 内核镜像已有,宿主未写 | `arboard`/win32 剪贴板 |
| `REGINFO` | ✅ 已实现宿主端口 + Windows 读取(2026-11-10) | 剩余:桌面窗口宿主无需额外动作 |
| `SAVEINFO` | ✅ 官方 no-op(2026-11-10) | 不需要实现查询 |
| `FPS[SKIP=]` | VM 存 SKIP,渲染循环不消费 | 官方语义=落后时允许丢帧;接呈现策略 |
| `SYSTEM[BACKGROUNDEXEC/SCREENSAVER]` | aligned-stub,逐参数未取证 | REA 参数处理体;`BACKGROUNDEXEC` 极性未定 |
| `WINDOW[WSX/WSY/MAX]` 回写 | headless 按 800×600 回写 | 真实窗口尺寸注入 |
| 音量重算时机 | 渲染时刻折叠祖先;官方命令时刻写 DSB | 出现「叶子当父层」语料时改命令时刻快照 |
| `113,01` 字体号 | fontdb 顺序/过滤与 GDI 不一致;默认下标 0 vs 1 | 字体清单 provider(见 §2.6) |
| `yscfg.dat` 其余 31 项 | 语义表为推断 | 按 `エンジン設定.exe` 控件逐项对号(手册无页) |
| `yssfs.dat` 非零配置 | 无实机非零样本 | 路径前缀常量 `0x681c60/0x6825c0` 取证 |
| serve 能力 | 无 panic 防线、`--save-root`、实时步长上限;首帧 ~1.9s 无输出;无 DIALOG 呈现;无 diag WS | H-P1-01/03/04/05、H-P1-06 `case close`、H-P1-07 时钟差异 |
| 音频回调 | `render_and_advance` panic 穿 cpal FFI = abort;锁中毒无一次性 Diag | `catch_unwind` + 诊断 |
| `key_last/mouse_last` 透出 | Engine 未透出脚本侧计数器 | F1 面板只剩宿主等价物 |

### 跨平台(浏览器/Android)

- 浏览器:`YpfSource::from_bytes`、`SaveFs`(OPFS/localStorage)、字体字节注入、
  wasm 渲染、退出信号/挂起恢复——均未做(见 `docs/cross-platform.md`)。
- Android 系统字体枚举:文档写待做,但 `DIV-AND-001` 标 fixed(host-capability);
  需以代码为准回写文档(避免重复劳动)。
- 浏览器声音:未实现 AudioContext/AudioWorklet(deferred)。

---

## 4. P3 · 性能(不能劣化)

已完成:T0–T3(表面池/资产预取/COPY2 去重延迟/跨层字形图集)、
VM 预解码/紧凑指令/变量槽 SoA、字形图集键去重、TRIM 窄带读回、
安卓 libpng 开关。**T4 未做**。

剩余项:

| 项 | 证据 | 下一步 |
|---|---|---|
| T4 安卓测量与决策门 | 任务书 §7 | 标题 10 轮/首次入场/立绘差分/菜单/存档;帧时间 p50/p95/p99;`[res]`;决定是否转单表面 |
| 文本推进帧 33ms | `text<1.4ms`;同帧 `tick=12–21ms`(es.IDSubTask2 ~6.6 万命令)、`readback=2.5–8.5ms`、`image=1.4–3.9ms` | 分别处理 VM 文本任务与 readback 触发条件 |
| 1.3M 命令帧 ~109ms(84ns/cmd) | PERF-VM-VARSTORE | 量 `LET`/`IF` 单条成本,再决定 handler/栈机优化 |
| 资产缓存无上限 | `evicted=0`,assets 265MB 单调涨 | 上限 + 不淘汰在场资产 + 按场景 LRU |
| 预解码内存随装载脚本增长 | `cmd_cache` 全站点;Android 无开关 | 按需/按场景释放;接 RunConfig |
| 同帧区域 `COPY2 → TRIM` | GPU 表面未提交,边界读回旧内容 | 提交顺序或强制物化 |
| 启动首个可播放 ~1.0s | 0.67s wgpu/adapter + 字体预热 | 异步/提前建窗(结构改动) |
| 单表面重构 Q1–Q6 | 计划书硬门禁 | 先取 SIZE2/COPY2/MERGE2/文本/缩略图/CGACT 分配语义证据 |

---

## 5. P4 · 工程与台账一致性

1. **CI**:新增 test 工作流(`cargo test` 关键 crate + `case --all` + `fatal_codes`);
   样本 env 与 skip 策略写清。
2. **`audit_hardcode.py` 全仓化**:当前只在 `crates/yu-rt/scripts/`;
   需覆盖 vfs/trace/cli 模块感知(B15)。
3. **台账状态统一**:多份文档仍把已修项写成 open,至少包括:
   - `crates/yu-rt/docs/coverage-matrix.md:42/76/87/193`(LET/WINDOWINFO/
     FONTINFO/CGINFO COLOR 口径过时);
   - `crates/yu-gfx/docs/divergences/README.md:68/70/113`(COLOR、DIV-GFX-070、
     FONTINFO NUM/LANG/NAME 过时);
   - `crates/yu-sound/docs/divergences/README.md:6/8`(SND-002/004 代码已实现);
   - `crates/yu-save/README.md:28`(LOAD[ID] 已由 DIV-RT-059 实现);
   - `docs/game-settings-ledger.md` 的 MOVIE「未实现」(plmpeg 已落地)。
4. **坏引用**:`crates/yu-gfx/README.md:48` 指向不存在的
   `docs/gfx-unimplemented-todo.md`(实际在 `docs/divergences/README.md`)。
5. **E-mote 子仓哈希不一致**:文档 `d0f03a1` vs 实际 `1b46604`,待主人核对。
6. **`todo-inventory.md` 机读化 + AU6 清空复核**:
   `Unimplemented` 运行期诊断仍有 gfx INFO/rt VAR·CSVINFO·REGINFO·SAVEINFO/
   vfs YPF 边缘/format YSTB v2;AU6 尚未达成。
7. **examples 清理**:`docs/examples-audit.md` B/C 组未执行(可删/可合并项)。
8. **测试 skip 机制**:无样本时 4 条 consistency 用例直接 FAIL(打印 SKIP 后
   panic)。作为「显式报错」可接受,但 CI 必须知道;建议区分
   `YURIS_REQUIRE_SAMPLES` 与普通本地跑。

---

## 6. 非缺口(不要修)

- 31 个域事件命令(CG/WINDOW/SAVE/FILE/MENU/EMOTE/MOVIE/FLASH/INPUT…):
  VM 只发事件,语义归宿主/子会话。
- `MIDI/MIDIEND/MIDIINFO/CDDA`:官方空桩,`aligned-stub`。
- `S_INT/S_FLT/S_STR/DEBUGLIST/LABEL/PREP/RETURNBREAK`:官方 `0x4634d8`
  真 no-op。
- `F_*/G_*/S_VAR/MACRO/_IF*/SYSTEMMODE/COMPILEMODE/RELEASEMODE/PROJECTFOLDER`:
  官方默认槽脚本错误(`0x459e90`),不是未实现。
- `CG 无 FILE/RID 且层不存在`:官方返回 -1 不建节点(`0x42a63d`),
  `gfx.layer.missing` 是忠实行为。
- `DIV-GFX-067` 资产并行解码:官方无并行;内核保留为**有意分歧**,
  像素/时序语义不变。是否收敛由主人决定(线程数固定 1 即退回串行)。
- `DIV-GFX-014` 同 Z 顺序:手册称不确定,保持确定性。

---

## 7. 建议执行顺序(本会话)

1. **G0 golden 三分类**:逐用例判定「基线过期/真回归」,先修真回归;
   确认后再重生成基线并留截图。
2. **官方证据采集**(本会话核心):
   - 1px 边框/四角 → `DIV-GFX-042`;
   - 文本多行/折行/清页 → `DIV-GFX-075/056/057`;
   - `CG[FILE=""]` → `DIV-GFX-060`;
   - 过渡 TA≠128 → `DIV-GFX-045`;
   - `TEXCOPY/TEXSSCOPY` → `DIV-GFX-007`;
   - 组 MID → `DIV-GFX-005`。
3. **证据已够的实现**:`VAR/CSVINFO/SAVEINFO/REGINFO` 先 REA,
   再按 `no-manual` 口径落地;宿主窗口动作(TRAY/剪贴板/最大化/全屏)。
4. **性能**:先做 T4 测量与归因,再决定是否继续 VM/readback 优化;
   不做单表面重构,直到 Q1–Q6 有证据。
5. **收尾**:CI + 台账勘误 + AU6 复核。

---

## 8. 复现命令

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"
export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YSBIN="$YURIS_SAMPLE_ERIS_ROOT/data/ysbin"

cargo check --workspace --all-targets
cargo run --release -p yu-cli --example game_diag_probe -- "$YURIS_SAMPLE_ERIS_ROOT" 900
cargo run --release -p yu-rt --example coverage_census -- 600 8
cargo test -p yu-rt --test consistency
cargo run --release -p yu-cli --bin yu-cli -- case "$YURIS_SAMPLE_ERIS_ROOT" --all
```
