# DIV-RT-084 · `SCRIPTPOS` 的 `pc` 线上是 code 段字节偏移

> 状态:已修复(2026-11-28)。影响 `TASKINFO[SCRIPTPOS]` / `TASK
> [SCRIPTPOS]` 与存档交换(内核档给官方读、官方档给内核读)。

## 现象

0.289 商业样本(`<0.289 商业>`)的官方 exe 读内核写出的 `save0000N.sd`
后:先恢复出立绘场景,但没有对话框,随后崩溃(Windows 事件 1000:
`0xc0000005`、faulting module `unknown` offset 0;minidump 的 `EIP=0`)。
内核自身读写自家档正常,所以问题只在**跨引擎交换**暴露。

## 证据

`%LOCALAPPDATA%\CrashDumps\<0.289 商业>.exe.<pid>.dmp`: 

```text
ExceptionCode = 0xc0000005, ExceptionAddress = 0x0
EIP = 0x0   (faulting module unknown, offset 0)
```

官方存档(`save/save00001.sd`,官方 exe 写出,DNO=10 = 文本式
`SCRIPTPOS`):

```text
IF=0 GOSUB=0 LOOP=0 text_line=135 pc=1698
script = <0.289 商业 s110>   (我们的 s110)
```

用 `yu-rt` 装载 s110 对照:

| 量 | 值 |
|---|---|
| `s110.code_offset_at(61)` | **1698** |
| `s110.source_position(61)` | 138 |
| DNO=4 末行台词所属组 | `[61] WORD line=138`(与 DNO10 的 `text_line=135` 相邻) |

⇒ 官方的 `pc` 是**下一组在 code 段里的字节偏移**,不是组下标。
内核旧实现把执行期 `ExecState.pc`(组下标)原样写进线上字段,同一份内核
档里 `pc=211` 恰好等于组下标 211(其 code 偏移是 5210),官方按字节偏移
从指令中段起跳,取到非法 opcode/函数指针 → `call [0]`。

## 修复

在**唯一线格式边界**换算(结构语义,无版本分支):

- 写盘/`TASKINFO[SCRIPTPOS]`(`encode_scriptpos`):组下标 →
  `ScriptFile::code_offset_at(pc)`。
- 读盘/`TASK[SCRIPTPOS]`(`decode_scriptpos_compat` → `scriptpos_wire_to_pc`):
  code 偏移 → `ScriptFile::pc_covering_code_offset(offset)`(偏移不在指令
  起点时取覆盖它的组)。
- IF/GOSUB/LOOP 帧位同样换算:GOSUB 帧按该帧自己的 `return_script`
  取脚本映射。
- 旧内核 8 字节 `[sid][pc]` blob 的 `pc` 本来就是组下标,兼容路径保持原样。
- 非 V2(无 `code_offsets`)保持旧行为。

判据是**脚本是否有 code 偏移表**(V2 有,V5 无),不按引擎版本号分支。

## 影响

- 0.29x / 0.25x 等 V2 剧本:首次可以做到内核档 ↔ 官方档双向恢复执行位。
- 以前"内核读官方档"虽不崩溃,但 `pc` 被当成组下标使用,恢复位置其实是
  错的(表面能进场景,执行到的是别的指令)。
- V5(0.4xx/0.5xx)的 `pc` 单位没有同等证据,本次未改;留给后续
  REA/官方存档取证。

## 测试注意(2026-11-28 复测)

- 存档里的 SCRIPTPOS 值来自**变量快照**,不是 SAVE 时现算:
  es_saveload `SAVE DNO=10 SET=$050C`;读档时 `LOAD DNO=10 LET=$0B15`
  后 `TASK ID=$050A SCRIPTPOS=$0B15`,再把 `$0B15` 抄回 `$050C`
  (`LET $050C=$0B15`)。
- 因此**载入旧内核档后立即存档**,`$050C` 仍是旧档里的组下标 blob,
  新存档原样带走它;必须在场景里推进至少一行(s31 `line 920`
  的 `TASKINFO ID=$050A SCRIPTPOS=1 LET=$050C` 重新生成)后保存,
  写出的才是 code 偏移。
- 运行时探针(当前代码,240/1200 tick):`$050C` 依次为
  s68 `pc=102`=`code_offset_at(5)`、s109 `pc=108`=`code_offset_at(2)`,
  证明编码换算生效。
- 旧档里的组下标 blob 被当作 code 偏移读取时会落到"覆盖该偏移的组",
  不会崩但与旧位置不同;这类档只适合读一次后推进一行再另存。

## 验证

```text
cargo test -p yu-rt --test fixture task_scrip_pos        → 2 passed
cargo test -p yu-rt --test fixture                       → 166 passed / 7 ignored
cargo test -p yu-rt --test corpus_v2                     → 3 passed
cargo test -p yu-rt --test consistency(带 ERIS env)      → 4 passed
cargo build --workspace --all-targets                    → 通过
```

实机交换:重建 `yu-cli.exe` → 复制到 `<games>/his/` → 内核存档 →
官方 exe 读取(待主人验收)。
