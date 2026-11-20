# DIV-RT-064 · 存档任务块 SCRIPTPOS 线格式分代:0.479 是 u16 GOSUB 计数+尾部 u8,0.495 是 u8+无尾部

- 状态: fixed(2026-11-20)
- 优先级: 高(0.47x 存档与官方**双向互读全断**)
- 发现日期 / 发现方式: 2026-11-20 / 主人实机:内核 qsave 后官方 QLOAD 报
  「スクリプト位置設定先のスクリプトファイル『**2\**script\userscript\new\01_al001.txt』
  が存在しませんでした」;反向官方 0.47x 存档内核读不了。

## 复现

- 0.47x 商业游戏内存档(容器 `0x138`)→ 官方读档报上述错误:
  路径开头的 `ku ro` 四个字符被吃掉 —— 官方从我们的串里**晚 4 字节**
  取脚本名。
- 反向:官方 0.47x 存档内核读档失败(同一任务块解析错位)。
- 0.495 语料(ERIS)不受影响:旧内核线格式恰好匹配 0.495。

## 官方行为(真值层级 1 + 2)

任务块(存档 `DNO=11`)的载荷 = yssd 值头(16B)+ **SCRIPTPOS 串**。串骨架
跨版本一致(帧宽用装载器推进量取证):

```text
[magic 8B 01..08][if 计数 2ch][IF 帧×18ch][GOSUB 计数][GOSUB 帧×20ch]
[LOOP 计数 2ch][LOOP 帧×34ch][0.47x 代:尾部 u8 2ch][line u32][pc u32]
[name_len u32][脚本名副本身节]
```

两代差异(装载器逐指令 + 官方存档逐字节双重取证):

| 代 | GOSUB 计数 | 尾部 u8 | 覆盖容器版本 | 装载器证据 |
|---|---|---|---|---|
| 旧代 | **u16(4 字符,高字节在前)** | **有** | `0x0C0`(0.450–0.466)、`0x138`(0.470–0.479(0.13)) | 0.450 `0x440515`、0.466 `0x4420bd`、0.470 `0x4428e9`、0.474 `0x448f69`、0.479 `0x449f3d/0x449f58`(均为「读两组 2 字符 + `add ecx/ebp,4`」);尾部 u8:0.479 `0x44a355`、0.466 `0x442498` |
| 新代 | u8(2 字符) | 无 | `0x1E0`(0.479(0.14)–0.495)、`0x1E1`/`0x1E6` | 0.480 `0x44fa3a` 一带、0.495 `0x4511cc`、v494(均 `add ebp,2`) |

存档样本逐字节验证:

- 官方 0.479(黒獣2 `save00001.sd` DNO=11,98B):「u16 + 尾部 u8」精确消耗,
  `line=12 pc=67 name_len=40 name=kuro2\script\userscript\new\00_op001.txt`;
  再编码与原字节**全等**。
- 官方 0.479(`save00002.sd`,131B):GOSUB 栈 2 帧,同上全等。
- 官方 0.495(SDK `data/save/save00001.sd`,容器 `0x1E0`):「u8 + 无尾部」
  精确消耗;加尾部 u8 会读出错误 name_len。旧内核格式即此代。

`0x0C0` 没有官方存档样本,尾部 u8 由 0.466 装载器 `0x442498` 的
「u8 存储 + 3×u32」读取序列取证(非样本级,已在残余登记)。

## 内核行为

`scriptpos.rs` 旧实现按 0.495 单代实现(GOSUB u8、无尾部),且由
`encode()` 全局使用:写 0.47x 工程时官方读档在脚本名处晚 4 字节起步
(`kuro` 被吃),反向同样错位。version→格式没有表,宿主也没有把存档
容器版本传给 VM。

## 修法

1. `yu-rt::scriptpos` 引入 [`ScriptPosWire`](crates/yu-rt/src/scriptpos.rs):
   `gosub_count: U8|U16` + `tail_byte: bool`,常量
   `WIRE_GOSUB_U16`(旧代)、`WIRE_GOSUB_U8`(新代);
   `encode_with_wire` / `decode_for_wire` 按代编解码。
2. **版本→格式的唯一表**在 `yu_save::profile::SaveProfile::scriptpos`
   (`profile_for` 每一行显式声明):`0x0C0`/`0x138` → `WIRE_GOSUB_U16`;
   `0x1E0`/`0x1E1`/`0x1E6`/`0x1DA`/`0x1EF` → `WIRE_GOSUB_U8`。
   加新版本 = 在 `profile_for` 加一行 + `profile::tests` 加断言。
3. `yu-engine` 开机在 `detect_save_container_version` 成功后把
   `profile.scriptpos` 注入 `Vm::set_scriptpos_wire`;`TASKINFO[SCRIPTPOS]`
   编码与 LOAD 解码都用它。
4. 解码容错:先按当前代解析(严格消耗完整个串),失败换另一代,
   最后回退旧内核布局(缺尾部 u8)——旧内核写出的档仍可读。

## 回归

- `yu-rt::scriptpos::tests::official_0479_task_blob_roundtrips_byte_exact`
  (官方 0.479 样本逐字节)
- `yu-rt::scriptpos::tests::official_0479_task_blob_with_gosub_frames_roundtrips`
- `yu-rt::scriptpos::tests::official_0495_task_blob_roundtrips_byte_exact`
  (官方 0.495 样本逐字节)
- `yu-rt::scriptpos::tests::wrong_wire_generation_is_rejected`
- `yu-save::profile::tests::observed_containers_have_expected_profiles`
  (新增 `scriptpos` 列断言)
- 既有 `scriptpos` 单测与 `yu-rt` fixture(131)全绿。

## 残余

- `0x0C0` 的尾部 u8 只有装载器读数证据、无官方存档样本;若将来拿到
  0.45x/0.46x 官方 `.sd`,补一条逐字节断言即可销案。
- 尾部 u8 的**语义**未取证(0.479 存入目标 `+0xa70`;0.495 无此字节);
  当前恒写 0,官方样本亦为 0。
