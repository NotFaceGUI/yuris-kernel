# DIV-SND-001 · 手册字段集合/顺序 vs 工程 yscm

状态:**fixed(2026-10-24 B6 收口;宿主一律按名消费,差异不再构成挂账)**

## 现象

`<sdk495>/マニュアル/YU-RIS/html/sound/soundinfo.html` 只列 19 个查询字段,
顺序为 `ID LET EXIST FILE E PLAY LOOP PAUSE LOOPCOUNT START RESTART END
VOLUME PAN PITCH TIME TOTALTIME CHANNEL BIT SAMPLE CODEC`。

官方 YSCom 0.495 编译的 ERIS `ysc.ybn`(`dump_yscm` 实测)给出 **24 槽**:

```text
0:ID 1:EXIST 2:E 3:PLAY 4:PAUSE 5:VOLUME 6:PAN 7:PITCH 8:FILE 9:FILETYPE
10:START 11:RESTART 12:END 13:LOOP 14:LOOPCOUNT 15:TIME 16:TOTALTIME
17:CHANNEL 18:BIT 19:SAMPLING 20:DATALEFT 21:DATARIGHT 22:CODEC 23:LET
```

SOUND 同样多出槽 13 `SPEED`(手册无页、语料 0 次)。

## 裁决(B6 定案)

- `yu-rt` 的 `DomainCall` 槽号**不跨版本稳定**(既有契约);宿主一律按
  `yscm` 参数名消费。
- `FILETYPE` 与 `CODEC` 是**两个字段**:内部 `+0x105`(OGG=1/WAVE=2)与
  对外 `+0x107`(WAVE=0/OGG=1/ACM=2),见 REA `0x422ab0`/`0x422e04`。
- **`DATALEFT` / `DATARIGHT`:fixed(no-manual)**。REA `0x44fe3c/0x44ff58`
  逐字节取证并实现(`yu-sound/src/state.rs::pcm_data_sample`):播放光标处
  Lock 一帧;16-bit 取 `abs(i16)`(-32768 回绕保留);8-bit 取 `|byte-0x80|`;
  CHANNEL∉{1,2}/BIT∉{8,16}/缓冲缺失/不足一帧 ⇒ -1;单声道 DATARIGHT=-1。
  **勘误**:16-bit 立体声的 DATARIGHT 官方读 `word [ptr+4]`——越过 4 字节
  锁区,是**下一帧的左声道**,不是同帧右声道(旧台账「取第二采样」对
  16-bit 情形不准确;官方越界怪癖 1:1 保留;8-bit 立体声读 `ptr[1]` 才是
  同帧右采样)。手册无页,按 2026-10-24 政策以 REA 为据实现并标注。
- **`SOUND[SPEED=]`(槽 13):unsupported-by-official**。REA 复核:处理体
  `0x44e1e8` 只读 `0x684160..0x68416c`(参数 0..12,反编译逐槽核对),
  **从不读 `0x68416d`** ⇒ 官方静默忽略 ⇒ 内核同型:宿主不建模该参数,
  不产生诊断(分发点证据:反编译无 `0x68416d` 引用)。

## 复现

```bash
YURIS_SAMPLE_ERIS_ROOT=<project-eris> \
  cargo run -q -p yu-rt --example dump_yscm -- SOUNDINFO
```
