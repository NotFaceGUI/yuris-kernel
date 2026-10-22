# DIV-SND-001 · 手册字段集合/顺序 vs 工程 yscm

状态：**open（按名消费，已规避）**

## 现象

`<sdk495>/マニュアル/YU-RIS/html/sound/soundinfo.html` 只列 19 个查询字段，
顺序为 `ID LET EXIST FILE E PLAY LOOP PAUSE LOOPCOUNT START RESTART END
VOLUME PAN PITCH TIME TOTALTIME CHANNEL BIT SAMPLE CODEC`。

官方 YSCom 0.495 编译的 ERIS `ysc.ybn`（`dump_yscm` 实测）给出 **24 槽**：

```text
0:ID 1:EXIST 2:E 3:PLAY 4:PAUSE 5:VOLUME 6:PAN 7:PITCH 8:FILE 9:FILETYPE
10:START 11:RESTART 12:END 13:LOOP 14:LOOPCOUNT 15:TIME 16:TOTALTIME
17:CHANNEL 18:BIT 19:SAMPLING 20:DATALEFT 21:DATARIGHT 22:CODEC 23:LET
```

SOUND 同样多出槽 13 `SPEED`（手册无页、语料 0 次）。

## 裁决

- `yu-rt` 的 `DomainCall` 槽号**不跨版本稳定**（既有契约）；宿主一律按
  `yscm` 参数名消费。
- `FILETYPE` 与 `CODEC` 是**两个字段**：内部 `+0x105`（OGG=1/WAVE=2）与
  对外 `+0x107`（WAVE=0/OGG=1/ACM=2），见 REA `0x422ab0`/`0x422e04`。
- `SPEED` 无手册页、语料 0 次 ⇒ Unimplemented（`snd.command.unimplemented`）。
- `DATALEFT` / `DATARIGHT` 无手册页，但 REA `0x44fe3c/0x44ff58` 已逐行取证
  （见台账 N0-12）。三层证据仍缺手册层，按任务书 §0.1 保持 Unimplemented
  （`snd.query.unsupported`），不实现。

## 复现

```bash
YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
  cargo run -q -p yu-rt --example dump_yscm -- SOUNDINFO
```
