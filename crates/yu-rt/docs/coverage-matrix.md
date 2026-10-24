# yu-rt 覆盖矩阵(M-1 普查 × M-2 实现)

> 本表由 `examples/coverage_census` 的实测输出生成,不是手抄估计。
> 台账行 `COV-M-xx` 与代码注释互查;证据规则见 `docs/command-provenance.md`。

## 0. 口径与复现

- 语料:`data.ypf`(31,501,753 字节),FNV-1a64 = `64d15de36b4fff30`;
  静态扫描 136 个 `yst`、36,974 组、80,102 窗口。
- 运行:`SYSTEM_START` 固定时钟,`tick_at(f*16)` 共 600 次,602 个逻辑帧;
  首个非空 tick / 首个可见帧(Graphics/Window/Text)都是 0;
  **B4 复跑(2026-10-24,默认 `@_DEBUGMODE=0`):命令 7,735,437 次**
  (与 B6 记录的同树基线一致;较 P3 2026-10-23 的 7,735,679 的差值来自
  B5 折回/CGS revert 等同树并行改动,B6 成果已归因,B4 内核改动不影响
  false 口径计数);事件丢弃 0;主线未结束。
- **debug 构建口径(B4 新增)**:官方 debug 构建运行期 `_DEBUGMODE=1`
  (DIV-L-001:窗口标题【デバッグモード】)。`YURIS_DEBUG_MODE=1` 复跑:
  **命令 8,764,407 次**,与 false 口径差 +1,028,970 = 调试 UI 自身流量
  (es.DebugTask/调试窗;增量集中在 `TASKINFO` 2,985 / `CGINFO` +370 /
  `WINDOWINFO` +1 / `SOUNDINFO` +1);**零 Fatal、零新挂号码**——B4 修复
  `_MOUSE_X0..3/Y0..3`(窗口槽坐标族,见 §3)后 debug 分支可完整进 title。
  §1/§2 表格仍以 false 口径为准(官方 release 游戏对拍的保守锚点),
  debug 口径数字以此注为准。
- `record_events=false`:逐命令触发数来自 `Vm::command_hits`(与事件流无关);
  域事件/文本事件/诊断照常产生。
- 手册页相对 `<sdk495>/マニュアル/YU-RIS/html/`;`—` = 本次索引未找到专门页。
- REA 锚点 = 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)的 opcode 分发表
  `DAT_007ace20[opcode]`(`FUN_0045cf90` 逐槽写入);未覆盖槽位用默认
  `0x459e90`。锚点是**处理体地址**,不等于语义已逐条复核。

```bash
export TEMP="<side-repo>/workbench/tmp"
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
cargo run -p yu-rt --example coverage_census -- 600 8
```

## 1. 全量命令矩阵(119 条,按静态语料频度排序)

| 命令名 | 域 | 语料频度 | 手册页 | REA 锚点 | 状态 | 决策 |
|---|---|---:|---|---|---|---|
| `GOSUB` | vm | 10215 | `control/gosub.html` | `0x4459c4` | ✅ 实现 | 保持 |
| `LET` | vm | 8136 | `variable/let.html` | `0x4468e0` | 🔁 部分(1 次未实现) | 主路径实现;剩 `@_SAVE_PATH` 宿主未注入(DIV-RT-010) |
| `IF` | vm | 3326 | `control/if.html` | `0x4462d0` | ✅ 实现 | 保持 |
| `IFEND` | vm | 3326 | `control/ifend.html` | `0x446518` | ✅ 实现 | 保持 |
| `RETURN` | vm | 1750 | `control/return.html` | `0x44c0fc` | ✅ 实现 | 保持 |
| `INT` | vm | 1428 | `variable/int.html` | `0x44661c` | ✅ 实现 | 保持 |
| `ELSE` | vm | 887 | `control/else.html` | `0x4404f0` | ✅ 实现 | 保持 |
| `IFBLEND` | vm | 887 | `control/if.html` | `0x4463c8` | ✅ 实现 | 保持 |
| `S_INT` | vm | 767 | `variable/s_int.html` | `0x4634d8` | ✅ no-op | 官方 `xor eax,eax; ret`;静态 767 处但不执行(DIV-RT-043) |
| `CG` | graphics | 534 | `cg/cg.html` | `0x42a514` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `LOOPEND` | vm | 522 | `control/loopend.html` | `0x4481d8` | ✅ 实现 | 保持 |
| `LOOP` | vm | 522 | `control/loop.html` | `0x447ef0` | ✅ 实现 | 保持 |
| `WORD` | vm | 464 | `basic/basic.html` | `0x458d44` | ✅ 实现 | 本 600 帧未触发;静态语料有 464 处;COV-M-03 M-2:每字符写 '1'/'2' 到 _TEXT_FLAG(REA 0x458d44) |
| `STR` | vm | 431 | `variable/str.html` | `0x4500d8` | ✅ 实现 | 保持 |
| `RETURNCODE` | vm | 396 | — | `0x44c390` | ✅ 实现 | 本 600 帧未触发;静态语料有 396 处;COV-M-02 M-2:自动模式追加 C/R/P,恒追加 E;仍为让出点(REA 0x44c390) |
| `F_INT` | vm | 337 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 官方默认体报「スクリプト実行エラー」(yse 0xdbbd2)+ 返回 -1;静态 337 处全在 `global_f.yst` 声明段,600 帧 0 执行(DIV-RT-043) |
| `VARACT` | vm | 310 | `variable/varact.html` | `0x453fa8` | ✅ 实现 | 保持 |
| `SAVE` | save | 290 | `file/save.html` | `0x44c45c` | ✅ 实现(B1) | 装存档宿主时命令时刻预约(值快照)/刷盘;`SET` 槽事件携带变量引用;未装宿主仍只发 `DomainCall`。REA 锚勘误:`0x44e1c8` 是 SAVEINFO(DIV-RT-042) |
| `LOAD` | save | 289 | `file/load.html` | `0x44708c` | ✅ 实现(B1) | 装存档宿主时块类型 2 按 var_id 直接恢复、块类型 0 写 `LET` 整组;`LET` 槽事件携带变量引用(DIV-RT-042) |
| `CGACT` | graphics | 274 | `cg/cgact.html` | `0x42c4a4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `END` | system | 205 | `control/end.html` | `0x441334` | ✅ 实现 | 本 600 帧未触发;静态语料有 205 处 |
| `GO` | vm | 195 | `control/go.html` | `0x445830` | ✅ 实现 | 保持 |
| `FLT` | vm | 173 | `variable/flt.html` | `0x4442ec` | ✅ 实现 | 保持 |
| `LOOPBREAK` | vm | 159 | `control/loopbreak.html` | `0x44808c` | ✅ 实现 | 保持 |
| `S_STR` | vm | 144 | `variable/s_str.html` | `0x4634d8` | ✅ no-op | 同 S_INT(DIV-RT-043) |
| `VARINFO` | vm | 119 | `variable/varinfo.html` | `0x456050` | ✅ 实现 | 保持 |
| `CGEND` | graphics | 100 | `cg/cgend.html` | `0x43d334` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `CGINFO` | graphics | 100 | `cg/cginfo.html` | `0x43d664` | 🔁 部分 | 主路径 + `ONMOUSE/ONMOUSE2/ONMOUSE3` 已实现(需宿主纹理采样器);`COLOR`/多字段顺序仍缺,见 §2 |
| `WAIT` | vm | 95 | `control/wait.html` | `0x456ae4` | ✅ 实现 | 保持 |
| `F_FLT` | vm | 72 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 F_INT(DIV-RT-043) |
| `F_STR` | vm | 64 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 F_INT(DIV-RT-043) |
| `FONT` | graphics | 42 | `font/font.html` | `0x444428` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `LOOPCONTINUE` | vm | 40 | `control/loopcontinue.html` | `0x448144` | ✅ 实现 | 保持 |
| `SOUND` | sound | 34 | `sound/sound.html` | `0x44e1e8` | ✅ 宿主 | `yu-sound` 已接线(§8);不装宿主时仍只发 `DomainCall` |
| `_` | vm | 33 | `basic/basic.html` | `0x4567c0` | ✅ 实现 | 本 600 帧未触发;静态语料有 33 处;COV-M-03 M-2:式值追加文本并按字符写 '1'/'2'(REA 0x4567c0) |
| `WINDOWINFO` | window | 31 | `window/windowinfo.html` | `0x4586ec` | 🔁 部分(129 次未实现) | 主路径实现;未实现路径见 §2 |
| `WINDOW` | window | 31 | `window/window.html` | `0x456b70` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SOUNDINFO` | sound | 25 | `sound/soundinfo.html` | `0x44f4bc` | ✅ 宿主 | 装宿主时 `op_soundinfo` → `SoundHost::query` → LET;未装时保持 M-2 |
| `FILEINFO` | file | 22 | `file/fileinfo.html` | `0x44286c` | ✅ 实现(FILE+EXIST+LET) | AU5 勘误:旧口径「➖ 域事件」未随 J3 更新。J3 起 `op_fileinfo` VFS 存在性查询写 LET(EVID-J3-01);B1 补「散盘」回退(REA `0x44316c` 文件系统支:`FILE=$es.SAVEPATH+名` ⇒ save 根判存在;存档槽判定/启动恢复闸门依赖,桌面实测暴露);FOLDER/SIZE/RELATION 变体仍挂账(B10) |
| `MATH` | vm | 21 | `math/math.html` | `0x4482c4` | ✅ 实现 | 保持 |
| `FILEACT` | file | 20 | `file/fileact.html` | `0x441370` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASKINFO` | vm | 19 | `control/taskinfo.html` | `0x4523fc` | ✅ 实现 | 保持 |
| `DIALOG` | system | 17 | `other/dialog.html` | `0x43fc74` | ✅ 实现 | VM 状态机:任务挂起 + `Vm::answer_dialog`;LET=1 基按钮序号,LETSTR=输入文本(DIV-RT-044) |
| `S_FLT` | vm | 17 | `variable/s_flt.html` | `0x4634d8` | ✅ no-op | 同 S_INT(DIV-RT-043) |
| `EMOTE` | emote | 13 | — | `0x4405a4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASK` | vm | 10 | `control/task.html` | `0x4506f4` | ✅ 实现 | 保持 |
| `FONTINFO` | graphics | 8 | `font/fontinfo.html` | `0x444cfc` | 🔁 部分(3 次未实现) | 主路径实现;未实现路径见 §2 |
| `MOVIE` | movie | 6 | `movie/movie.html` | `0x44b134` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SYSTEMINFO` | system | 5 | — | `0x4505e4` | ✅ 实现 | 保持 |
| `SAVEEND` | save | 5 | `file/saveend.html` | `0x44e1a0` | ✅ 实现(B1) | 装存档宿主时清预约(官方 flush 不清表,语料 es_GSD 刷盘后显式 SAVEEND 同型);未装宿主仍只发 `DomainCall` |
| `DEBUGLIST` | system | 5 | — | `0x4634d8` | ✅ no-op | 官方 `xor eax,eax; ret`(DIV-RT-043) |
| `IFBREAK` | vm | 5 | `control/ifbreak.html` | `0x446408` | ✅ 实现 | 本 600 帧未触发;静态语料有 5 处 |
| `MOUSE` | system | 4 | `system/mouse.html` | `0x44aaa0` | ✅ 实现 | 内核输入状态(MODE/TIME/MOVE/CLICK/WHEEL/X/Y/CURSOR/HX/HY/ID);光标显示/形状归宿主 |
| `MENU` | menu | 4 | `menu/menu.html` | `0x449df8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MOVIEINFO` | movie | 4 | `movie/movieinfo.html` | `0x44ba2c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `YMVINFO` | vm | 4 | — | `0x459b98` | ⛔ 宿主能力 | YMV 元数据查询(需宿主媒体层;DIV-RT-045) |
| `EMOTEINFO` | emote | 3 | — | `0x441014` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `YMV` | vm | 3 | — | `0x458ee0` | ➖ 域事件 | 媒体对象生命周期;只发 `Domain::Movie`(DIV-RT-045) |
| `SYSTEM` | system | 2 | — | `0x45023c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FPS` | system | 2 | `system/fps.html` | `0x4457a8` | ✅ 实现 | 保持 |
| `INPUT` | system | 2 | `system/input.html` | `0x446564` | ✅ 实现 | KEY/MOUSE/PAD 源开关;关闭时整源计数器冻结(`FUN_0041a4b8`) |
| `SOUNDEND` | sound | 2 | `sound/soundend.html` | `0x44f308` | ✅ 宿主 | `yu-sound` 删层(§8) |
| `TEXT` | vm | 2 | `text/text.html` | `0x453448` | ✅ 实现 | COV-M-02 M-2 重写:OUTPUTMODE/WAIT/CLICK/RETURN/PAGE/CLICKRETURN/CLICKPAGE/CLEAR/AUTO*;CLICK 自动模式让出(REA 0x453448) |
| `MOVIEEND` | movie | 2 | `movie/movieend.html` | `0x44b9d8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASKEND` | vm | 2 | `control/taskend.html` | `0x4521dc` | ✅ 实现 | 本 600 帧未触发;静态语料有 2 处 |
| `TRAY` | system | 2 | `other/tray.html` | `0x453a4c` | ➖ 域事件 | 托盘图标属宿主;只发 `Domain::System` 事件(DIV-RT-045) |
| `YMVEND` | vm | 2 | — | `0x459a64` | ➖ 域事件 | 同 YMV(DIV-RT-045) |
| `LABELINFO` | vm | 1 | `other/labelinfo.html` | `0x44674c` | ✅ 实现 | 保持 |
| `ERROR` | system | 1 | — | `0x46358c` | ✅ 实现 | 写逐命令启用表 `[0x689b00+opcode]`(默认全 1);消费者未定位(DIV-RT-046) |
| `CLIPACT` | graphics | 1 | `clip/clipact.html` | `0x43e2d0` | ✅ 实现 | 内核镜像 + `DomainCall`;TEXT/SET 都存在才生效(DIV-RT-045) |
| `FLASH` | flash | 1 | `flash/flash.html` | `0x443814` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FLASHEND` | flash | 1 | `flash/flashend.html` | `0x443e64` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FLASHINFO` | flash | 1 | `flash/flashinfo.html` | `0x443eb4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MATH3D` | vm | 1 | — | `0x448900` | ✅ 实现 | 已实现(EVID-J3-03;本表旧口径作废) |
| `MENUINFO` | menu | 1 | `menu/menuinfo.html` | `0x44a6f4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `);` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 宏/编译产物残留;运行时同默认槽(DIV-RT-043) |
| `CDDA` | system | 0 | — | `0x429fb0` | 🔵 aligned-stub | 官方空桩(B6,声音台账 0-9);归类保持现状,诊断 `rt.cmd.aligned_stub` |
| `CLIPINFO` | graphics | 0 | `clip/clipinfo.html` | `0x43e410` | ✅ 实现 | 读镜像写入 LET;丢弃 `<0x20` 字节(DIV-RT-045) |
| `COMPILEMODE` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期;运行时同默认槽(DIV-RT-043) |
| `CSV` | vm | 0 | — | `0x43e640` | ✅ no-op | 官方只做参数求值后返回 0(DIV-RT-046) |
| `CSVINFO` | vm | 0 | — | `0x43e660` | ⛔ 未取证 | 读 CSV 文件并写 LET;文件/解析语义未取证 |
| `ERRORINFO` | vm | 0 | — | `0x46356c` | ✅ no-op | 官方只做参数求值后返回 0(DIV-RT-046) |
| `F_VAR` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 F_INT(DIV-RT-043) |
| `G_FLT` | vm | 0 | `variable/g_flt.html` | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_FLT2` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_FLT3` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_FLT4` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_INT` | vm | 0 | `variable/g_int.html` | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_INT2` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_INT3` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_INT4` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_STR` | vm | 0 | `variable/g_str.html` | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_STR2` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_STR3` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_STR4` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;初值/数组经 `VARACT[INIT]`(DIV-RT-024);运行时落在默认槽(DIV-RT-043) |
| `G_VAR` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 S_VAR(DIV-RT-043) |
| `IFCONTINUE` | vm | 0 | `control/ifcontinue.html` | `0x446490` | ✅ 实现 | 语料未使用;单测覆盖 |
| `LABEL` | vm | 0 | `other/label.html` | `0x4634d8` | ✅ no-op | 标签由 `ysl` 装载;指令本体官方 no-op(DIV-RT-043) |
| `MACRO` | vm | 0 | `other/macro.html` | `0x459e90(默认)` | ✅ 脚本错误槽 | 仅 `macro.yst` 编译期使用;运行时默认槽报脚本错误(DIV-RT-043) |
| `MENUACT` | menu | 0 | `menu/menuact.html` | `0x44a41c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MENUEND` | menu | 0 | `menu/menuend.html` | `0x44a5d0` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MIDI` | sound | 0 | — | `0x44aa40` | ➖ 域事件 | 只发 `DomainCall`;官方处理体为空桩 ⇒ aligned-stub(B6,声音台账 0-9),诊断 `rt.cmd.aligned_stub` |
| `MIDIEND` | sound | 0 | — | `0x44aa60` | ➖ 域事件 | 只发 `DomainCall`;官方处理体为空桩 ⇒ aligned-stub(B6,声音台账 0-9),诊断 `rt.cmd.aligned_stub` |
| `MIDIINFO` | sound | 0 | — | `0x44aa80` | ➖ 域事件 | 只发 `DomainCall`;官方处理体为空桩 ⇒ aligned-stub(B6,声音台账 0-9),诊断 `rt.cmd.aligned_stub` |
| `PLUG` | vm | 0 | — | `0x44bd10` | ✅ no-op | 官方只做参数求值后返回 0(DIV-RT-046) |
| `PREP` | vm | 0 | — | `0x4634d8` | ✅ no-op | 官方 no-op(DIV-RT-043) |
| `PROJECTFOLDER` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期;运行时同默认槽(DIV-RT-043) |
| `REGINFO` | vm | 0 | `reg/reginfo.html` | `0x44bd30` | ⛔ 宿主能力 | 读 Windows 注册表;内核不猜(DIV-RT-045) |
| `RELEASEMODE` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期;运行时同默认槽(DIV-RT-043) |
| `RETURNBREAK` | vm | 0 | — | `0x4634d8` | ✅ no-op | 官方 no-op(DIV-RT-043) |
| `SAVEINFO` | save | 0 | — | `0x44e1c8` | ⛔ 挂账(B7) | 查询处理体未取证、语料 0 次;REA 锚勘误:`0x44e1c8` 是 SAVEINFO 本体(SAVE=`0x44c45c`,B1) |
| `SYSTEMMODE` | system | 0 | `projectconfig/systemmode.html` | `0x459e90(默认)` | ✅ 脚本错误槽 | 仅 `projectconfig.txt` 编译期有效;运行时官方默认槽报错(DIV-RT-043) |
| `S_VAR` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 编译期声明;出现在 yst 即官方脚本错误(DIV-RT-043) |
| `TEXTINFO` | vm | 0 | `text/textinfo.html` | `0x4538cc` | ✅ 实现 | 语料未使用;M-2 按手册 + REA 实现,单测覆盖(COV-M-02) |
| `VAR` | vm | 0 | — | `0x453f88` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `WINDOWEND` | window | 0 | `window/windowend.html` | `0x45866c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `_IFDEF` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 预处理器指令;运行时同默认槽(DIV-RT-043) |
| `_IFEND` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 _IFDEF(DIV-RT-043) |
| `_IFNDEF` | vm | 0 | — | `0x459e90(默认)` | ✅ 脚本错误槽 | 同 _IFDEF(DIV-RT-043) |

## 2. 600 帧运行期未实现/未接线(按触发次数)

> P3 复跑(2026-10-23,`coverage_census 600 8`)的原始计数。`FILEINFO`
> 已归零(实现后不再挂号);`ERROR` 已实现启用表;`LET` 只剩
> `@_SAVE_PATH`;`_WINDOW_EXEC_CAPTION` 的 600 条诊断随宿主 setter 归零。

| 命令 | 触发次数 | 处理器 | 归因/处置 |
|---|---:|---|---|
| `CG` | 80902 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `SOUNDINFO` | 7200 | 域事件:sound | 未装 `SoundHost` 时只发事件;装宿主后走 query(§8) |
| `CGEND` | 5713 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `CGINFO` | 1598 | 已实现 | 未安装 `GraphicsHost`(宿主查询面;INFO 不是 VM 语义) |
| `CGACT` | 631 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `SOUND` | 531 | 域事件:sound | 未装 `SoundHost` 时只发事件;装宿主后由 `yu-sound` 消费 |
| `WINDOWINFO` | 371 | 部分 | AU5 勘误:旧口径「已实现/未装宿主」归因不全。几何(EXIST/E/X/Y/SX/SY)装 `GraphicsHost` 即可答;`FULLSCREEN/CAPTION/ACTIVE/MAXIMIZE/RESIZE/CLOSEMODE/MENUID/REGIONID/DRAGID` **装了宿主仍 `INFO_UNSUPPORTED`**(字段族 = DIV-RT-050 候选根因,REA 锚点 `0x4586ec`)。B4 复跑 371(P3 时 129;+242 发生在 B4 之前——
B4 内核改动不触碰 WINDOWINFO 计数路径,false 口径总命令数与 B6 基线
7,735,437 逐字节一致;精确归因随 B2 字段族收口复核) |
| `SAVE` | 16 | 已实现(B1) | 未装存档宿主时只发域事件;装宿主(命令时刻预约/刷盘,DIV-RT-042 闭环)——`save-load-loop` 用例落盘 gsd/extra/seld/kidoku |
| `EMOTE` | 12 | 域事件:emote | 域事件已发;E-mote 子系统归宿主 |
| `WINDOW` | 10 | 域事件:window | 域事件已发;查询/写回需宿主状态机(+1 来自 `_WINDOW_INIT_POS` 分支) |
| `SYSTEM` | 6 | 域事件:system | 域事件已发;子系统语义归宿主 |
| `FONT` | 5 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `FILEACT` | 3 | 域事件:file | 域事件已发;查询/写回需宿主状态机 |
| `FONTINFO` | 3 | 部分 | AU5 勘误:旧口径「已实现」超前。样式字段(COLOR/COLOR2/SX/SY/BOLD/AA/DRAWMODE/GRADATION/ROTATION)已答;`NUM/LANG/NAME(未设置/NO=n)` 需宿主字体清单,仍挂号(DIV-GFX-036) |
| `SAVEEND` | 3 | 已实现(B1) | 未装存档宿主时只发域事件;装宿主即清预约(DIV-RT-042) |
| `LET` | 1 | 已实现 | AU5 勘误:`@_SAVE_PATH` 已由 `BootConfig::paths` + yu-cli `host_paths()` 注入解决(DIV-RT-010 fixed);仅 census example 未注入,产品装配已解 |
| `MOUSE` | 1 | 已实现 | 2026-10-22:内核输入状态(REA `0x44aaa0`);本表旧口径为未接线 |
| `INPUT` | 1 | 已实现 | 2026-10-22:输入源开关(REA `0x446564`);本表旧口径为未接线 |
| `SOUNDEND` | 1 | 域事件:sound | 未装 `SoundHost` 时只发事件;装宿主后删层 |

> **2026-10-22 输入收口**:`CGINFO[ONMOUSE/ONMOUSE2/ONMOUSE3]` 已在
> `yu-gfx` + 宿主 `TextureSampler` 中实现(REA `0x43d664`/`0x43de01`),
> 按钮系统的 `es.BT.MOUSECHECK` 不再因 `INFO_UNSUPPORTED` 失去命中;
> 上表 1598 次是“未安装 GraphicsHost”的普查口径,不再代表当前缺口。
> `COLOR`(显式坐标取色)与多字段顺序仍未实现。

## 3. 系统变量缺口(运行期高频)

| 变量 | 600 帧诊断 | 手册页 | REA | 状态 | 决策 |
|---|---:|---|---|---|---|
| `_WINDOW_EXEC_CAPTION` | 0 次(P3 前 600 次/每帧 `IF`) | —(0.479/0.495 字节级均无) | 读 `0x427704`(case 0x78 → `[0x801924]`);脚本写 `0x446f70`;引擎 `WM_SYSCOMMAND(0xA3)` 路径 `FUN_0045a9d0` → `0x45a9e7` | ✅ 宿主 setter | 手册仍无页;实现形态按 P1 结论:内核持一次性标志,宿主经 `Vm::request_window_caption()` 置 1,脚本读 1 后写回 0(es_Scenario)。触发条件(`*ecx` 来源)未取证 ⇒ VM 不自行置位(DIV-RT-007/047) |
| `_TEXT_FLAG` 读 | 120 次(M-2 前) | `basic/svariable.html` | `0x446b93`(id `0x7b` → `[state+0x3c]`) | ✅ 已实现 | `COV-M-01`;返回控制字节流,字节表见 `src/text.rs::flag` |
| `_TEXT_FLAG` 写 | 1 次 | 同上(○ 可赋值) | `0x446b98..0x446bb5`(id 0x7b → `[state+0x3c]` 逐字节拷贝) | ✅ 实现 | 写 = 替换控制字节流;不改文本、不逐字符重算(DIV-RT-032) |
| `_TEXT_NUM` | 0 | — | 读 `0x4276e3` ← `0x80174c`;装载 `yst.ybn` 时写 `0x45d73f` | ✅ 实现 | = 工程文本总数;ERIS 396(yst.ybn 头部 == yst_list 求和;DIV-RT-048) |
| `_WINDOW_INIT_POS` | 1 次 | — | 读 `0x4276f3` ← `[0x80133c]`;写 `0x460b74/0x460b87`(← `yscfg.dat` 第 10 项) | ✅ 宿主注入 | `BootConfig::window_init_pos`(ERIS 工程 = 0;DIV-RT-047) |
| `_SAVE_PATH` | 1 次 | `basic/svariable.html` | — | ⛔ 宿主注入 | 路径由宿主给值;示例未注入(DIV-RT-010) |
| `_MOUSE_X0..3`/`_MOUSE_Y0..3` | debug 口径运行期读(s66 调试 UI);false 口径 0 次 | —(手册无页) | 读 `FUN_00427350` case 0x85..0x8e = 4 窗口槽客户区坐标(槽表 `DAT_008019c0..cc`;每槽 HWND@+0x5b0、X@+0x5b8、Y@+0x5bc;case 0x85/0x86 同槽 ⇒ `_MOUSE_X`≡`_MOUSE_X0`);每帧 `FUN_00403ee0`:槽无窗口 ⇒ 坐标 0,不报错 | ✅ fixed(no-manual) | 2026-10-24 政策:按 REA 实现并标注。旧实现误当 ycd 按钮查绑定 → Fatal 杀主任务 = debug 口径黑帧根因(DIV-L-001 复核;`yu-rt/src/sysvar.rs`) |

## 4. 与 `command-provenance.md` 的交叉核对(勘误留痕)

- `TEXT`:`provenance` 原记「➖ 记录事件」;M-2 后为 VM 侧属性状态机
  (OUTPUTMODE/AUTO*/CLEAR/控制标志 + CLICK 让出)。显示绘制仍在宿主。
- `_`:`provenance` 记「✅ 实现」;M-2 补上每字符 `'1'/'2'` 标志写入
  (REA `0x4567c0`),事件字段不变。
- `WORD`:`provenance` 记「✅ 实现」;M-2 补上 `_TEXT_FLAG` 宽度标志
  (REA `0x458d44`)。
- `RETURNCODE`:`DIV-RT-035` 只写了让出;M-2 补上行尾控制标志
  (自动 `C/R/P` + 恒 `E`,手动置清屏请求)。
- `TEXTINFO`:`provenance` 未列;0.495 语料 0 次、600 帧 0 次,但手册
  `text/textinfo.html` 有页且 REA `0x4538cc` 完整,故实现(不是猜测)。
- `_TEXT_FLAG`:`provenance` 记「⛔ 挂账(DIV-RT-032)」;读取与写入
  (`0x446b98..`)均已按 REA 解除挂账。
- `LET` 的运行时未实现 122 → 2(M-2)→ **1**(P3;只剩 `_SAVE_PATH`)。
- `F_*`:`provenance` 记「⛔ Unimplemented」;P3 定案为官方默认槽脚本
  执行错误(静态 473 处全部不执行;DIV-RT-043)。
- `LABEL`/`PREP`/`RETURNBREAK`:`provenance` 未列;P3 证实为官方 no-op
  (`0x4634d8`),已实现。
- `MATH3D`:矩阵旧口径「未取证」作废;代码已实现(EVID-J3-03)。
- `DomainCall`:§5 的「没有 script/pc」缺口已关闭(P4)。

## 5. 已知契约缺口(M-2 记录,留给宿主/事件契约会话)

- ~~`DomainCall` 只有 frame/domain/name/args:没有 script/pc~~ **P4 已解决**:
  新增可选 `script`/`pc`(内核发出的总是 `Some`);见 §9 与
  `domain-event-contracts.md` §1。yu-trace 的映射已用 `..` 兼容。
- ~~`SAVE/LOAD/FILEINFO/SOUNDINFO` 的 `SET`/`LET` 是变量引用,但
  `ArgValue` 只携带求值结果,变量身份丢失(DIV-RT-042)。~~ **B1 已解决**
  (2026-10-24):`ArgValue.reference`(白名单槽:SAVE[SET]/LOAD[LET]/
  FILEINFO[LET]/SOUNDINFO[LET]);宿主另有 `SaveHost` 同步契约(预约/刷盘/
  拉取),三条存档命令不再依赖异步事件做语义。旧 trace 缺字段按 `None` 读。
- `VmEvent::Text.id` 仍是 UTF-8 `String`;非 UTF-8 CP932 ID 保持空并
  挂号(DIV-GFX-021)。修复需要改事件字段类型,跨 crate 契约变更。

## 6. 台账索引

| 台账 | 内容 | 代码入口 | 回归 |
|---|---|---|---|
| `COV-M-01` | `_TEXT_FLAG` 读取(控制字节流) | `src/text.rs`, `src/sysvar.rs` | `fixture::text_flag_sysvar_reads_the_control_stream` |
| `COV-M-02` | `TEXT` 属性/输出模式/行尾节奏 + `TEXTINFO` | `src/vm/handlers.rs::op_text/op_textinfo/op_returncode` | `fixture::text_outputmode_*`, `textinfo_*` |
| `COV-M-03` | `WORD`/`_` 的逐字符宽度标志 | `src/text.rs::TextState::append_text` | `fixture::text_flags_*` |
| `COV-M-04` | 域事件参数名视图(SOUND/SAVE/FILE 接口基础) | `src/event.rs::VmEvent::domain_args` | `fixture::domain_args_map_slots_through_project_yscm` |
| `COV-P0-01` | 输入按下沿/文本推进键名修正 | `src/input.rs` | `input::tests::repress_after_release_restarts_at_one`, `text_advance_buttons_use_binding_names`, `fixture::one_text_line_per_frame_until_advance_input` |
| `COV-P1-01` | `_WINDOW_EXEC_CAPTION` 有界取证 | 本文 §3/§7 | `fixture::window_exec_caption_is_a_host_setter_round_trip` |
| `COV-P3-01` | 官方默认槽 `0x459e90` = 脚本执行错误(非 no-op) | `src/vm/handlers.rs`(`ScriptError`) | `fixture::default_slot_reports_once_and_yields_without_terminating` |
| `COV-P3-02` | 官方 no-op 槽 `0x4634d8` | `src/vm/handlers.rs`(`Noop`) | `fixture::noop_slots_continue_in_the_same_pump` |
| `COV-P3-03` | DIALOG 契约 + 任务挂起/回答 | `src/dialog.rs`, `src/vm.rs`(`answer_dialog`) | `fixture::dialog_*` |
| `COV-P3-04` | 剪贴板镜像 / ERROR 启用表 / 域边界 | `src/vm/handlers.rs`(`op_clipact/op_clipinfo/op_error`) | `fixture::clipact_*`, `error_sets_*`, `tray_is_a_domain_event` |
| `COV-P3-05` | HANTOZEN/ZENTOHAN 官方表 | `src/case_tables.rs`, `data/case-tables-0495.tsv` | `case_tables::tests::*`, `fixture::varact_hantozen_*` |

## 7. P0/P1 验证(2026-10-21)

### P0 · 真实语料文本路径

全量 `SYSTEM_START` 600 tick + 每 20 tick 点击: **0 条 Text 事件**;场景流
`s21 → s139` 后,`es.ScenarioScript` 在 tick 126 被 `TASK[E=0]` 暂停,
标题/UI 依赖 `GraphicsHost`/`WINDOW`/`MOUSE`,未进入文本。这是宿主缺口,
不是文本语义缺口。

为隔离文本路径,直接以真实语料标签 boot(绕过标题):

| 入口 | tick | Text | ReturnCode | bytes | flags | 结论 |
|---|---:|---:|---:|---:|---|---|
| `SCENARIO_EPILOGUE`(s129) | 30 | 1 | 1 | 12 | `222222CRPE` | 6 个全角字符 + 自动行尾 `C/R/P/E` |
| `RP_101`(s133),每 10 tick 点击 | 120 | 10 | 10 | 132 | 106 | 10 行逐行推进;每行 `'2'*n + CRPE`;第 99 tick 主线正常结束 |

`SCENARIO_EPILOGUE` 文本 = `エピローグ。`(CP932);`RP_101` 文本 =
`回想シーン１スタート / あいうえお / かきくけこ / さしすせそ / 回想シーン１終了 /
回想シーン２スタート / ＡＢＣＤＥ / ＦＧＨＩＪ / ＫＬＭＮＯ / 回想シーン２終了`。
输出模式均为 1(自动),AUTORETURN/AUTOPAGE/AUTOCLICK = 1。

**P0 发现并修复的输入 bug**:

- 松开后计数器逐帧递减;重新按下时原实现 `+= 1`,长期未按后一次短按从
  `-N` 加到 `-N+1`,永远到不了正值 ⇒ 第二次点击无法推进文本。手册
  「押された瞬間から 1 → 2 → 3」⇒ 按下沿重置为 1。
- `text_advance_requested` 原检查 `ENTER`/`CTRL`,而计数器键名是
  `KEY_ENTER`/`KEY_CTRL` ⇒ ENTER/CTRL 从未生效(只有 MOUSE_L 生效)。
- 两处修复后台账 `COV-P0-01`;fixture 第三条文本行验证长期松开后的第二次点击。

### P1 · `_WINDOW_EXEC_CAPTION`(仍挂账)

- **手册**:0.479/0.495 全部 HTML 按字节搜索无 `_WINDOW_EXEC_CAPTION`;
  只有 `basic/svariable.html` 的 `_WINDOW_EXEC_MAX/CLOSE`。older SDK
  0.292/0.466 也无命中。⇒ 三层缺①。
- **语料语义**(`es_Scenario.yst`):`IF[@_MENUCODE==30001 ||
  @_WINDOW_EXEC_CAPTION==1]` → 写回 0 → `WINDOW[NO=0 WSX=… WSY=…]`;
  注释掉的旧路径 `IF[@_WINDOW_EXEC_CAPTION==1] WINDOW[NO=0 MAXIMIZE=1]`。
  即「窗口尺寸再初始化」的一次性宿主事件标志。
- **REA**:
  - 读:`0x427704`(int getter case 0x78 → `[0x801924]`);
  - 脚本写:`0x446f70` `MOV byte [0x801924], DL`(系统变量写入路径);
  - 引擎写:`WM_SYSCOMMAND(0xA3)` → `FUN_0045a3b4` 的 case →
    `FUN_0045a9d0`(`0x45a9d0`);`0x45a9e7` 写 `[0x801924] = (char)*ecx + 1`,
    仅当 `[0x801338]==1` 且 `[0x8015e0]==0`;否则置 `[0x801504]=1` 并返回 0。
  - **未取证**:`*ecx` 的具体来源(窗口过程参数)与 `0x801338/0x8015e0`
    的语义;不足以定义完整可观测行为。
- **决策**:保持 `Unimplemented`(DIV-RT-007 已补本条);若将来放宽门禁,
  正确形态是**宿主窗口事件 setter**(像 `_WINDOW_EXEC_MAX/CLOSE`),不是
  VM 里猜一个 0/1 状态机。


## 8. 声音域接线（任务 N,2026-10-22）

> 台账：`docs/sound-ledger.md`。REA 锚点仍是 0.495 处理体地址；
> 状态列区分「事件层」与「宿主实现」。
> 口径注(AU5/B6)：§1 把 MIDI/MIDIEND/MIDIINFO 按事件层记「域事件」，本表按
> 终态记「🔵 aligned-stub」——同一事实的两侧口径，非矛盾；`CDDA` 在 `domain_of`
> 落 `Domain::Other` 而非 `Sound`（归类错位保留——B6 纪律是行为不变，只改
> 诊断归属；域归类一行修仍归 B2）。
> B6（2026-10-24）：SOUNDINFO 增列 `DATALEFT/DATARIGHT` fixed(no-manual)
> （DIV-SND-001）；`SOUND[SPEED=]` unsupported-by-official（处理体无槽 13
> 读取点）；ACM WAVE fixed(no-manual)（DIV-SND-004）；负 START/RESTART
> fixed(Likely)（DIV-SND-002）。

| 命令 | opcode | 运行触发 | 处理器 | 宿主/状态 | 说明 |
|---|---:|---:|---|---|---|
| `SOUND` | 87 | 531 | `0x44e1e8` | ✅ `yu-sound` | 层表/装载/参数写回；`SPEED`(槽 13) 官方不读 ⇒ 静默忽略(unsupported-by-official)；负 START/RESTART 延迟 fixed(Likely) |
| `SOUNDINFO` | 89 | 7,200 | `0x44f4bc` | ✅ `yu-sound` | 装宿主时走 `op_soundinfo` → `SoundHost::query` → `LET`；未装宿主保持 M-2 通用域路径。B1:`LET` 槽事件携带变量引用(query 参数面不变,DIV-RT-042)。B6:`DATALEFT/DATARIGHT` fixed(no-manual,`0x44fe3c/0x44ff58`) |
| `SOUNDEND` | 88 | 1 | `0x44f308` | ✅ `yu-sound` | 删层；随后 `SOUNDINFO[EXIST]=0` |
| `MIDI` | 67 | 0 | `0x44aa40` | 🔵 aligned-stub | 官方空桩,内核同型:域事件照发,诊断 `rt.cmd.aligned_stub`(B6) |
| `MIDIEND` | 68 | 0 | `0x44aa60` | 🔵 aligned-stub | 同上 |
| `MIDIINFO` | 69 | 0 | `0x44aa80` | 🔵 aligned-stub | 同上 |
| `CDDA` | 0 | 0 | `0x429fb0` | 🔵 aligned-stub | 官方空桩;事件行为不变(归类仍在 Other),只改诊断归属(B6) |

回归：不装宿主时 `coverage_census 600 8` 的三条计数与 M-2 一致
（531 / 7,200 / 1），诊断文案不变。


## 9. P3 收口(2026-10-23)

### 9.1 默认槽定案(DIV-RT-043)

- 0.495 分发表 `DAT_007ace20` 先以 **`0x459e90` 填满 119 槽**
  (`FUN_0045cf90` `0x45d1ae`),再逐槽覆盖。
- 默认体(`0x459e90..0x459ea4`,REA 原始字节):
  `push 1; push 0xdbbd2; call 0x465d08; add esp,8; mov eax,-1; ret`。
  `0xdbbd2` 在工程 `ysbin/yse.ybn` 里 = `「[内部エラー] スクリプト実行
  エラーが発生しました。」`。⇒ **不是 no-op、不是静默继续**:报一次全局
  错误(`[0x5e4f84]` 抑制重复)并返回 -1(本帧让出,任务不终止)。
- 另一类槽映射到 `0x4634d8`(`xor eax,eax; ret`)= **真 no-op**:
  `DEBUGLIST`/`LABEL`/`PREP`/`RETURNBREAK`/`S_INT`/`S_FLT`/`S_STR`。
- 内核:`HandlerKind::ScriptError` / `Noop` 与 `Unknown` 分离;
  `F_*`/`G_*`/`MACRO`/`S_VAR`/`_IF*`/`);`/`SYSTEMMODE`/`COMPILEMODE`/
  `RELEASEMODE`/`PROJECTFOLDER` 归默认槽。
- **静态口径 vs 运行口径**:F_* 静态 473 处(`global_f.yst` 的声明段,
  group 0..N),但入口标签都指向声明段之后,600 帧运行期命中 **0**;
  扫描器不改,两口径同时记录。

### 9.2 600 帧复跑(改动后)

| 指标 | 值 |
|---|---:|
| 命令总数 | 7,735,679 |
| 逻辑帧 | 602 |
| `LET` 未实现 | 1(`@_SAVE_PATH`) |
| `ERROR` 未实现 | 0(已实现启用表) |
| `_WINDOW_EXEC_CAPTION` 诊断 | 0(宿主 setter 状态) |
| 文本事件 | 0(无 GraphicsHost/标题流,与 M-1 同因) |

`_WINDOW_INIT_POS` 从 Unimplemented 变为 0 后,`es_GSD.yst:490` 的
`IF[@_WINDOW_INIT_POS==0]` 首次成立,首帧命令数 1,292,091 → 1,292,100
(9 条初始化命令);这是**按证据**的分支变化,语料测试金丝雀已同步钉值。

### 9.3 新增实现一览

`DIALOG`(VM 状态机)、`CLIPACT`/`CLIPINFO`(剪贴板镜像)、`ERROR`
(逐命令启用表)、`TRAY`/`YMV`/`YMVEND`(宿主域事件)、`CSV`/`PLUG`/
`ERRORINFO`(官方 no-op)、`LABEL`/`PREP`/`RETURNBREAK`/`DEBUGLIST`/
`S_*`(官方 no-op)、`F_*`/`G_*`/`S_VAR`/`MACRO`/`_IF*`/`);`/
`SYSTEMMODE`/`COMPILEMODE`/`RELEASEMODE`/`PROJECTFOLDER`(默认槽错误)、
`MATH[RND]`(MT19937 + 注入种子)、`VARACT[HANTOZEN/ZENTOHAN]`(官方表)、
`_TEXT_FLAG` 写、`_TEXT_NUM`、`_WINDOW_INIT_POS`、`_WINDOW_EXEC_CAPTION`
宿主 setter、`DomainCall.script/pc`。
