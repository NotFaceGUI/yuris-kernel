# yu-rt 覆盖矩阵(M-1 普查 × M-2 实现)

> 本表由 `examples/coverage_census` 的实测输出生成,不是手抄估计。
> 台账行 `COV-M-xx` 与代码注释互查;证据规则见 `docs/command-provenance.md`。

## 0. 口径与复现

- 语料:`data.ypf`(31,501,753 字节),FNV-1a64 = `64d15de36b4fff30`;
  静态扫描 136 个 `yst`、36,974 组、80,102 窗口。
- 运行:`SYSTEM_START` 固定时钟,`tick_at(f*16)` 共 600 次,602 个逻辑帧;
  首个非空 tick / 首个可见帧(Graphics/Window/Text)都是 0;
  命令 7,046,350 次;事件丢弃 0;主线未结束。
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
| `LET` | vm | 8136 | `variable/let.html` | `0x4468e0` | 🔁 部分(2 次未实现) | 主路径实现;未实现路径见 §2 |
| `IF` | vm | 3326 | `control/if.html` | `0x4462d0` | ✅ 实现 | 保持 |
| `IFEND` | vm | 3326 | `control/ifend.html` | `0x446518` | ✅ 实现 | 保持 |
| `RETURN` | vm | 1750 | `control/return.html` | `0x44c0fc` | ✅ 实现 | 保持 |
| `INT` | vm | 1428 | `variable/int.html` | `0x44661c` | ✅ 实现 | 保持 |
| `ELSE` | vm | 887 | `control/else.html` | `0x4404f0` | ✅ 实现 | 保持 |
| `IFBLEND` | vm | 887 | `control/if.html` | `0x4463c8` | ✅ 实现 | 保持 |
| `S_INT` | vm | 767 | `variable/s_int.html` | `0x4634d8` | ✅ 实现 | 本 600 帧未触发;静态语料有 767 处 |
| `CG` | graphics | 534 | `cg/cg.html` | `0x42a514` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `LOOPEND` | vm | 522 | `control/loopend.html` | `0x4481d8` | ✅ 实现 | 保持 |
| `LOOP` | vm | 522 | `control/loop.html` | `0x447ef0` | ✅ 实现 | 保持 |
| `WORD` | vm | 464 | `basic/basic.html` | `0x458d44` | ✅ 实现 | 本 600 帧未触发;静态语料有 464 处;COV-M-03 M-2:每字符写 '1'/'2' 到 _TEXT_FLAG(REA 0x458d44) |
| `STR` | vm | 431 | `variable/str.html` | `0x4500d8` | ✅ 实现 | 保持 |
| `RETURNCODE` | vm | 396 | — | `0x44c390` | ✅ 实现 | 本 600 帧未触发;静态语料有 396 处;COV-M-02 M-2:自动模式追加 C/R/P,恒追加 E;仍为让出点(REA 0x44c390) |
| `F_INT` | vm | 337 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `VARACT` | vm | 310 | `variable/varact.html` | `0x453fa8` | ✅ 实现 | 保持 |
| `SAVE` | save | 290 | `file/save.html` | `0x44c45c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `LOAD` | save | 289 | `file/load.html` | `0x44708c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `CGACT` | graphics | 274 | `cg/cgact.html` | `0x42c4a4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `END` | system | 205 | `control/end.html` | `0x441334` | ✅ 实现 | 本 600 帧未触发;静态语料有 205 处 |
| `GO` | vm | 195 | `control/go.html` | `0x445830` | ✅ 实现 | 保持 |
| `FLT` | vm | 173 | `variable/flt.html` | `0x4442ec` | ✅ 实现 | 保持 |
| `LOOPBREAK` | vm | 159 | `control/loopbreak.html` | `0x44808c` | ✅ 实现 | 保持 |
| `S_STR` | vm | 144 | `variable/s_str.html` | `0x4634d8` | ✅ 实现 | 本 600 帧未触发;静态语料有 144 处 |
| `VARINFO` | vm | 119 | `variable/varinfo.html` | `0x456050` | ✅ 实现 | 保持 |
| `CGEND` | graphics | 100 | `cg/cgend.html` | `0x43d334` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `CGINFO` | graphics | 100 | `cg/cginfo.html` | `0x43d664` | 🔁 部分(1598 次未实现) | 主路径实现;未实现路径见 §2 |
| `WAIT` | vm | 95 | `control/wait.html` | `0x456ae4` | ✅ 实现 | 保持 |
| `F_FLT` | vm | 72 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `F_STR` | vm | 64 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `FONT` | graphics | 42 | `font/font.html` | `0x444428` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `LOOPCONTINUE` | vm | 40 | `control/loopcontinue.html` | `0x448144` | ✅ 实现 | 保持 |
| `SOUND` | sound | 34 | `sound/sound.html` | `0x44e1e8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `_` | vm | 33 | `basic/basic.html` | `0x4567c0` | ✅ 实现 | 本 600 帧未触发;静态语料有 33 处;COV-M-03 M-2:式值追加文本并按字符写 '1'/'2'(REA 0x4567c0) |
| `WINDOWINFO` | window | 31 | `window/windowinfo.html` | `0x4586ec` | 🔁 部分(129 次未实现) | 主路径实现;未实现路径见 §2 |
| `WINDOW` | window | 31 | `window/window.html` | `0x456b70` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SOUNDINFO` | sound | 25 | `sound/soundinfo.html` | `0x44f4bc` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FILEINFO` | file | 22 | `file/fileinfo.html` | `0x44286c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MATH` | vm | 21 | `math/math.html` | `0x4482c4` | ✅ 实现 | 保持 |
| `FILEACT` | file | 20 | `file/fileact.html` | `0x441370` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASKINFO` | vm | 19 | `control/taskinfo.html` | `0x4523fc` | ✅ 实现 | 保持 |
| `DIALOG` | system | 17 | `other/dialog.html` | `0x43fc74` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `S_FLT` | vm | 17 | `variable/s_flt.html` | `0x4634d8` | ✅ 实现 | 本 600 帧未触发;静态语料有 17 处 |
| `EMOTE` | emote | 13 | — | `0x4405a4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASK` | vm | 10 | `control/task.html` | `0x4506f4` | ✅ 实现 | 保持 |
| `FONTINFO` | graphics | 8 | `font/fontinfo.html` | `0x444cfc` | 🔁 部分(3 次未实现) | 主路径实现;未实现路径见 §2 |
| `MOVIE` | movie | 6 | `movie/movie.html` | `0x44b134` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SYSTEMINFO` | system | 5 | — | `0x4505e4` | ✅ 实现 | 保持 |
| `SAVEEND` | save | 5 | `file/saveend.html` | `0x44e1a0` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `DEBUGLIST` | system | 5 | — | `0x4634d8` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `IFBREAK` | vm | 5 | `control/ifbreak.html` | `0x446408` | ✅ 实现 | 本 600 帧未触发;静态语料有 5 处 |
| `MOUSE` | system | 4 | `system/mouse.html` | `0x44aaa0` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MENU` | menu | 4 | `menu/menu.html` | `0x449df8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MOVIEINFO` | movie | 4 | `movie/movieinfo.html` | `0x44ba2c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `YMVINFO` | vm | 4 | — | `0x459b98` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `EMOTEINFO` | emote | 3 | — | `0x441014` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `YMV` | vm | 3 | — | `0x458ee0` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `SYSTEM` | system | 2 | — | `0x45023c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FPS` | system | 2 | `system/fps.html` | `0x4457a8` | ✅ 实现 | 保持 |
| `INPUT` | system | 2 | `system/input.html` | `0x446564` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SOUNDEND` | sound | 2 | `sound/soundend.html` | `0x44f308` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TEXT` | vm | 2 | `text/text.html` | `0x453448` | ✅ 实现 | COV-M-02 M-2 重写:OUTPUTMODE/WAIT/CLICK/RETURN/PAGE/CLICKRETURN/CLICKPAGE/CLEAR/AUTO*;CLICK 自动模式让出(REA 0x453448) |
| `MOVIEEND` | movie | 2 | `movie/movieend.html` | `0x44b9d8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `TASKEND` | vm | 2 | `control/taskend.html` | `0x4521dc` | ✅ 实现 | 本 600 帧未触发;静态语料有 2 处 |
| `TRAY` | system | 2 | `other/tray.html` | `0x453a4c` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `YMVEND` | vm | 2 | — | `0x459a64` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `LABELINFO` | vm | 1 | `other/labelinfo.html` | `0x44674c` | ✅ 实现 | 保持 |
| `ERROR` | system | 1 | — | `0x46358c` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `CLIPACT` | graphics | 1 | `clip/clipact.html` | `0x43e2d0` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `FLASH` | flash | 1 | `flash/flash.html` | `0x443814` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FLASHEND` | flash | 1 | `flash/flashend.html` | `0x443e64` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `FLASHINFO` | flash | 1 | `flash/flashinfo.html` | `0x443eb4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MATH3D` | vm | 1 | — | `0x448900` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `MENUINFO` | menu | 1 | `menu/menuinfo.html` | `0x44a6f4` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `);` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `CDDA` | system | 0 | — | `0x429fb0` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `CLIPINFO` | graphics | 0 | `clip/clipinfo.html` | `0x43e410` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `COMPILEMODE` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `CSV` | vm | 0 | — | `0x43e640` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `CSVINFO` | vm | 0 | — | `0x43e660` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `ERRORINFO` | vm | 0 | — | `0x46356c` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `F_VAR` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `G_FLT` | vm | 0 | `variable/g_flt.html` | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_FLT2` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_FLT3` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_FLT4` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_INT` | vm | 0 | `variable/g_int.html` | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_INT2` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_INT3` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_INT4` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_STR` | vm | 0 | `variable/g_str.html` | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_STR2` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_STR3` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_STR4` | vm | 0 | — | `0x459e90(默认)` | ➖ 声明类 | 不按命令执行;初值/数组经 `VARACT[INIT]`(DIV-RT-024) |
| `G_VAR` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `IFCONTINUE` | vm | 0 | `control/ifcontinue.html` | `0x446490` | ✅ 实现 | 语料未使用;单测覆盖 |
| `LABEL` | vm | 0 | `other/label.html` | `0x4634d8` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `MACRO` | vm | 0 | `other/macro.html` | `0x459e90(默认)` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `MENUACT` | menu | 0 | `menu/menuact.html` | `0x44a41c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MENUEND` | menu | 0 | `menu/menuend.html` | `0x44a5d0` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MIDI` | sound | 0 | — | `0x44aa40` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MIDIEND` | sound | 0 | — | `0x44aa60` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `MIDIINFO` | sound | 0 | — | `0x44aa80` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `PLUG` | vm | 0 | — | `0x44bd10` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `PREP` | vm | 0 | — | `0x4634d8` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `PROJECTFOLDER` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `REGINFO` | vm | 0 | `reg/reginfo.html` | `0x44bd30` | ⛔ 未实现 | 手册有页但语义未取证 ⇒ 挂档 |
| `RELEASEMODE` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `RETURNBREAK` | vm | 0 | — | `0x4634d8` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `SAVEINFO` | save | 0 | — | `0x44e1c8` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `SYSTEMMODE` | system | 0 | `projectconfig/systemmode.html` | `0x459e90(默认)` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `S_VAR` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `TEXTINFO` | vm | 0 | `text/textinfo.html` | `0x4538cc` | ✅ 实现 | 语料未使用;M-2 按手册 + REA 实现,单测覆盖(COV-M-02) |
| `VAR` | vm | 0 | — | `0x453f88` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `WINDOWEND` | window | 0 | `window/windowend.html` | `0x45866c` | ➖ 域事件 | 只发 `DomainCall`;子系统语义归宿主(契约见 `domain-event-contracts.md`) |
| `_IFDEF` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `_IFEND` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |
| `_IFNDEF` | vm | 0 | — | `0x459e90(默认)` | ⛔ 未取证 | 三层缺①(手册无页)⇒ 挂档 |

## 2. 600 帧运行期未实现/未接线(按触发次数)

| 命令 | 触发次数 | 处理器 | 归因/处置 |
|---|---:|---|---|
| `CG` | 80620 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `SOUNDINFO` | 7200 | 域事件:sound | 域事件已发;查询/写回需宿主状态机 |
| `CGEND` | 5713 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `CGINFO` | 1598 | 已实现 | 未安装 `GraphicsHost`(宿主查询面;INFO 不是 VM 语义) |
| `FILEINFO` | 1136 | 域事件:file | 域事件已发;查询/写回需宿主状态机 |
| `CGACT` | 631 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `SOUND` | 531 | 域事件:sound | 域事件已发;查询/写回需宿主状态机 |
| `WINDOWINFO` | 129 | 已实现 | 未安装 `GraphicsHost`(宿主查询面;INFO 不是 VM 语义) |
| `SAVE` | 16 | 域事件:save | 域事件已发;查询/写回需宿主状态机 |
| `EMOTE` | 12 | 域事件:emote | 域事件已发;查询/写回需宿主状态机 |
| `WINDOW` | 9 | 域事件:window | 域事件已发;查询/写回需宿主状态机 |
| `SYSTEM` | 6 | 域事件:system | 域事件已发;查询/写回需宿主状态机 |
| `FONT` | 5 | 域事件:graphics | 域事件已发;查询/写回需宿主状态机 |
| `FILEACT` | 3 | 域事件:file | 域事件已发;查询/写回需宿主状态机 |
| `FONTINFO` | 3 | 已实现 | 未安装 `GraphicsHost`(宿主查询面;INFO 不是 VM 语义) |
| `SAVEEND` | 3 | 域事件:save | 域事件已发;查询/写回需宿主状态机 |
| `LET` | 2 | 已实现 | 2 次:`@_SAVE_PATH` 宿主未注入 1 次 + `_TEXT_FLAG` 写入未取证 1 次 |
| `MOUSE` | 1 | 域事件:system | 域事件已发;查询/写回需宿主状态机 |
| `INPUT` | 1 | 域事件:system | 域事件已发;查询/写回需宿主状态机 |
| `SOUNDEND` | 1 | 域事件:sound | 域事件已发;查询/写回需宿主状态机 |
| `ERROR` | 1 | 无处理器 | 手册无页;分发表 opcode 15 走默认处理体 `0x459e90` ⇒ 语义未取证 |

## 3. 系统变量缺口(运行期高频)

| 变量 | 600 帧诊断 | 手册页 | REA | 状态 | 决策 |
|---|---:|---|---|---|---|
| `_WINDOW_EXEC_CAPTION` | 600 次(每帧 `IF`) | —(0.479/0.495 字节级均无) | 读 `0x427704`(case 0x78 → `[0x801924]`);脚本写 `0x446f70`;引擎 `WM_SYSCOMMAND(0xA3)` 路径 `FUN_0045a9d0` → `0x45a9e7` | ⛔ 未取证 | 三层仍缺①(手册无页)。语料语义(es_Scenario):一次性「窗口尺寸再初始化」标志,处理完写回 0;注释掉的旧路径是 `WINDOW[MAXIMIZE=1]`。属宿主窗口事件,不是 VM 语义 ⇒ 保持挂号(DIV-RT-007);若将来放宽门禁,只能做宿主 setter,不得在 VM 猜 |
| `_TEXT_FLAG` 读 | 120 次(M-2 前) | `basic/svariable.html` | `0x446b93`(id `0x7b` → `[state+0x3c]`) | ✅ 已实现 | `COV-M-01`;返回控制字节流,字节表见 `src/text.rs::flag` |
| `_TEXT_FLAG` 写 | 1 次 | 同上(○ 可赋值) | 写入路径未复核 | ⛔ 未实现 | 读取已实现;写入语义(是否重建文本/标志)未取证,保持挂号(DIV-RT-032) |
| `_TEXT_NUM` | 0 | — | `0x4276e3`(装载期全局 `0x80174c`) | ⛔ 未实现 | 手册无页;引擎取值不是文本缓冲计数,语义未取证 |
| `_WINDOW_INIT_POS` | 1 次 | — | `0x4276f3`(int getter case 0x79 → `[0x80133c]`) | ⛔ 未取证 | 手册无页;DIV-RT-007 |
| `_SAVE_PATH` | 1 次 | `basic/svariable.html` | — | ⛔ 宿主注入 | 路径由宿主给值;示例未注入(DIV-RT-010) |

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
- `_TEXT_FLAG`:`provenance` 记「⛔ 挂账(DIV-RT-032)」;读取已按 REA
  解除挂账,写入仍挂。
- `LET` 的运行时未实现从 122 次降到 2 次(读 `_TEXT_FLAG` 不再挂号)。

## 5. 已知契约缺口(M-2 记录,留给宿主/事件契约会话)

- `DomainCall` 只有 frame/domain/name/args:**没有 script/pc**;宿主无法
  把域命令定位回脚本。新增字段会破坏既有构造与模式匹配,本轮不改。
- `SAVE/LOAD/FILEINFO/SOUNDINFO` 的 `SET`/`LET` 是变量引用,但
  `ArgValue` 只携带求值结果,变量身份丢失(DIV-RT-042)。
  宿主可用 `VmEvent::domain_args(&yscm)` 取参数名,但拿不到引用目标。
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
| `COV-P1-01` | `_WINDOW_EXEC_CAPTION` 有界取证(未实现) | 本文 §3/§7 | — |

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

