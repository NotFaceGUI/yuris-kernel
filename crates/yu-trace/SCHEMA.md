# yu-trace 记录格式(schema 3)

> 本文件是本 crate 协议的**唯一定义**。`docs/oracle.md` 是方法论草案,冲突以本文件为准
> (差异见文末)。任何字段增删或语义变化都必须递增 `schema` 并在此留痕。
>
> v3 起协议是 **producer-neutral** 的:官方引擎采集器与内核录制器写同一种记录,
> 比较时按 `(sub, kind, frame, ordinal)` 对齐,而不是按物理行号 `seq`。

## 1. 文件形态

- UTF-8 文本,一行一条记录(LF 结尾,末行也必须有 LF)。
- 不允许 CR、空行、注释行。
- 键顺序固定;输出不经过任何无序容器。
- 文件必须非空,且第一条记录是 header。

## 2. 封套(固定六字段,顺序固定)

```json
{"schema":3,"seq":0,"frame":42,"sub":"vfs","kind":"open","payload":{...}}
```

| 字段 | 类型 | 含义 |
|---|---|---|
| `schema` | u32 | 协议版本,当前 `3`。未知版本 = `trace.schema`(Fatal,拒绝猜测)。v2 文件不迁移、直接拒绝。 |
| `seq` | u64 | 从 0 起严格连续递增,是**同一 producer 内**录制顺序的唯一序;不按时间排序。跨 producer 对齐不得使用 `seq`。 |
| `frame` | u64 \| null | 逻辑帧号;边界①在帧外(启动链)为 `null`;边界②/③必为整数。 |
| `sub` | string | `meta` / `vfs` / `vm` / `frame`;`platform` 为 ④ 保留。 |
| `kind` | string | 见 §4;必须与 `sub` 匹配。未知 kind = `trace.schema`。 |
| `payload` | object | 按 `kind` 定形,见 §4。 |

校验规则(解析时强制):

1. `seq` 必须等于物理行号减 1;
2. `seq = 0` 必须是 `sub = "meta"`, `kind = "header"`;
3. `header` 只允许出现在 `seq = 0`,且 `frame = null`;
4. `sub` 必须等于 `kind` 的固有子系统(例如 `open` 必须是 `vfs`);
5. `frame = null` 只允许 `meta` 与 `vfs`;
6. 未知字段、未知 kind、未知 sub、未知 jump/domain/level/mode 字符串 = `trace.schema`;
7. 缺字段、类型不符 = `trace.parse`;
8. ④ `platform` 子系统的记录在 schema 3 中仍被拒绝(平台后端未定义);
9. ③ `frame_description` 的封套 `frame` 与 payload `frame` 必须相等。

## 3. 通用值编码

| 概念 | 编码 |
|---|---|
| 字节串(路径、CP932 文本) | JSON 字符串,每字节映射为 U+0000..U+00FF;`0x00..0x1F`、`0x7F..0xFF` 以 `\u00xx` 写出。读回只接受 ≤ U+00FF 的字符。 |
| 浮点 | JSON 数字,最短往返表示(`{:?}`);`NaN`/`±Infinity` 写成字符串 `"NaN"` / `"Infinity"` / `"-Infinity"`。 |
| 整数 | JSON 整数;`i64` 可为负,`u64` 不为负。 |
| 有序表 | JSON 数组;顺序是语义的一部分,不排序、不去重。 |
| SHA-256 | 64 位小写十六进制字符串。格式不符 = `trace.parse`,内容不符 = `trace.schema`。 |

字节串编码与真实 Unicode 文本在书写形式上可区分:非 ASCII 字节一律 `\u00xx`,
而真实 Unicode 字符原样写出。

## 4. kind / payload

### 4.1 `meta` / `header`(`seq = 0`)

```json
{"codec":"cp932",
 "producer":{"kind":"kernel","name":"yu-trace","version":"0.1.0","sample_sha256":null},
 "tables":["ysc: v495 (105 命令)","..."],
 "boot":{"entry":"SYSTEM_START","fps":60,"start_ms":0,"frame_ms":16,
         "script_key":"d36fac96","debug_mode":false,
         "argv":{"len":0,"sha256":"…"},
         "host":{"screen_color":32,"media_readonly":false,"os_sound":true,
                 "dx_mode":0,"paths":{"len":0,"sha256":"…"}}},
 "input":{"events":2,"sha256":"…"},
 "clock":{"mode":"tick_at","now_ms":[0,16]},
 "kernel":"yu-trace 0.1.0"}
```

| 字段 | 类型 | 来源 |
|---|---|---|
| `codec` | string | 调用方注入的 `yu_vfs::Codec`(`cp932` / `gbk`) |
| `producer.kind` | `"kernel"` \| `"official"` | 记录生产者族 |
| `producer.name` | string | 内核名或官方采集器名 |
| `producer.version` | string | 内核版本;official 侧是**采集器版本** |
| `producer.sample_sha256` | string \| null | 样本身份摘要(官方数据包/脚本集合);两边都存在时参与跨 producer 比较 |
| `tables` | string[] | `yu_rt::Vm::table_provenance()`,只含相对路径/表版本 |
| `boot.entry` | string | `BootConfig::entry_label` |
| `boot.fps` | u32 | `BootConfig::fps` |
| `boot.start_ms` | u32 | `BootConfig::start_ms` |
| `boot.frame_ms` | u32 | `BootConfig::frame_ms` |
| `boot.script_key` | string | `BootConfig::script_key`,8 位小写十六进制(字节序即数组顺序) |
| `boot.debug_mode` | bool | `BootConfig::debug_mode`(`@_DEBUGMODE`) |
| `boot.argv.len` / `.sha256` | u64 / string | 命令行**摘要**(值可能含绝对路径,不入录制内容),算法见 §5 |
| `boot.host.screen_color` | u32 | `BootConfig::screen_color`(`@_SCREEN_COLOR`) |
| `boot.host.media_readonly` | bool | `BootConfig::media_readonly`(`@_MEDIA_READONLY`) |
| `boot.host.os_sound` | bool | `BootConfig::os_sound`(`@_OS_SOUND`) |
| `boot.host.dx_mode` | u32 | `BootConfig::dx_mode`(`@_DXMODE`) |
| `boot.host.paths.len` / `.sha256` | u64 / string | 宿主路径变量**摘要**,算法见 §5 |
| `input.events` | u64 | `InputTimeline::len()`(显式设定的帧数) |
| `input.sha256` | string | 见 §5;只覆盖 VM 实际消费的逻辑帧 |
| `clock.mode` | string | `none` / `tick` / `tick_at` / `mixed` |
| `clock.now_ms` | (u32\|null)[] | tick_at 的显式时钟序列;`mixed` 中 `null` = 该次用 `tick()` |
| `kernel` | string | 人类可读的内核标识;**不参与跨 producer diff** |

`clock` 的强制关系:

| mode | now_ms |
|---|---|
| `none` | 必须为空(没有 tick) |
| `tick` | 必须为空(全部使用内部时钟) |
| `tick_at` | 非空,且每项都是整数 |
| `mixed` | 非空,且同时含有 `null` 与整数 |

**volatile / 跨 producer 规则**:

- 不参与跨 producer 语义比较:`kernel`、`tables`、`producer.name`、
  `producer.version`、`clock`、以及 official 侧不存在的 boot 字段。
- 参与比较:`codec`、`boot` 的语义字段、`input`、双方都非 null 的
  `producer.sample_sha256`。
- 上述选择由 diff 的显式投影实现(§6),不是容差;任何被选中的字段仍按精确值比较。

禁止项:绝对路径、主机名、墙钟、线程/进程标识、随机数。`header` 与其他记录
用同一个 `seq` 序;需要补头时调用 `TraceLog::prepend_header` 并整体重编号。

### 4.2 `vfs` 子系统(边界①)

#### `kind = "open"`

```json
{"source":"ypf","key":"ysbin/ysc.ybn","bytes":11156,"sha256":"…","diag":null}
```

| 字段 | 类型 | 含义 |
|---|---|---|
| `source` | string | 内层数据源的 `source_name()` |
| `key` | bytes | 归一后的查找键(不是原始输入) |
| `bytes` | u64 \| null | 成功时的字节数;失败为 `null` |
| `sha256` | string \| null | 成功时的内容 SHA-256(64 位小写十六进制);失败为 `null` |
| `diag` | string \| null | 失败时的稳定错误码(如 `vfs.missing`);成功为 `null` |

失败记录不得携带 `bytes`/`sha256`;成功记录两者必填(schema 强制)。

#### `kind = "exists"`

```json
{"source":"ypf","key":"ysbin/ysl.ybn","exists":true}
```

#### `kind = "list"`

```json
{"source":"ypf","key":"ysbin",
 "entries":[{"name":"ysc.ybn","kind":"file","size":11156},
            {"name":"sub","kind":"dir","size":null}],
 "diag":null}
```

| 字段 | 类型 | 含义 |
|---|---|---|
| `entries[].name` | bytes | 最后一级组件(归一后) |
| `entries[].kind` | `"file"` \| `"dir"` | 目录项种类 |
| `entries[].size` | u64 \| null | 文件为字节数;目录必须为 `null` |
| `diag` | string \| null | 失败时的稳定错误码;失败时 `entries` 为空数组 |

目录项保持数据源返回的顺序(实现上为名字字节升序)。

### 4.3 `vm` 子系统(边界②)

`frame` 一律为整数。载荷与 `yu_rt::VmEvent` 变体一一对应,由本 crate 的
穷尽 `match` 生成;`yu-rt` 新增变体时本 crate 编译失败(强制 schema 评审)。

#### `frame_start` / `frame_end`

```json
{"payload":{}}
```

#### `kind = "command"`

```json
{"script":29,"pc":107,"name":"TEXT",
 "args":[{"slot":0,"value":{"t":"str","v":"…"}}]}
```

`args[].value` 的 `t` 为 `int` / `flt` / `str`;`v` 按 §3 编码。

#### `kind = "jump"`

```json
{"script":29,"pc":107,"to_script":30,"to_pc":3,"jump":"gosub"}
```

`jump` 枚举:`go` `gosub` `return` `if_false` `else` `loop_back` `loop_break`
`loop_continue` `if_break` `if_continue` `task_start`。

#### `kind = "task_spawn"`

```json
{"id":"SYSTEM","script":29,"pc":0,"enabled":true,"main":true}
```

#### `kind = "task_finish"`

```json
{"id":"SYSTEM"}
```

#### `kind = "wait"`

```json
{"task":"SYSTEM","frames":1,"time_ms":null}
```

`frames` 与 `time_ms` 至少一个非 `null`(schema 强制)。

#### `kind = "text"`

```json
{"id":"","bytes":"…"}
```

`bytes` 为 CP932 原始字节(§3 字节串编码)。

#### `kind = "return_code"`

```json
{"line":1}
```

#### `kind = "fps"`

```json
{"fps":60,"skip":false}
```

#### `kind = "system_call"`

```json
{"name":"SYSTEM","args":[]}
```

#### `kind = "domain_call"`

```json
{"domain":"graphics","name":"CG","args":[]}
```

`domain` 枚举:`graphics` `sound` `input` `file` `save` `menu` `window` `movie`
`flash` `emote` `system` `other`。

#### `kind = "end"`

```json
{"restart":false}
```

#### `kind = "diag"`

```json
{"code":"rt.cmd.unknown","detail":"…"}
```

### 4.4 `frame` 子系统(边界③,schema 3 实现)

#### `kind = "frame_description"`

```json
{"frame":42,"level":"event","display_sha256":"…",
 "pass_count":1,"op_count":1234,"executable_ops":1200,"skipped_ops":34,
 "display":{"passes":[…],"layers":[…]}}
```

| 字段 | 类型 | 含义 |
|---|---|---|
| `frame` | u64 | 必须等于封套 `frame` |
| `level` | `"event"` \| `"screen"` | 事件级(帧语义层)或屏幕级(官方截图摘要) |
| `display_sha256` | string | **规范 display JSON 文本**的 SHA-256(无 display 时由调用方提供) |
| `pass_count` | u32 | 渲染 pass 数 |
| `op_count` | u64 | 总操作数 |
| `executable_ops` | u64 | 可执行操作数 |
| `skipped_ops` | u64 | 跳过操作数 |
| `display` | JSON \| null | 完整规范化 frame JSON;`null` = 摘要级记录 |

规则:

1. `display` 非 null 时,解析器必须重新规范序列化并校验
   `display_sha256 == SHA-256(规范文本)`;不一致 = `trace.schema`。
2. `display` 为 null 时,`display_sha256` 必填(摘要级),格式错误 = `trace.parse`。
3. 规范化只去掉 JSON 空白,**保持对象键序与数组顺序**;同一调用方输入两次写入
   逐字节一致。
4. 本 crate 不解释 display 的内部结构;与 `yu-gfx` 的字段解耦,由调用方传入。
5. 屏幕级记录(`level = "screen"`)只证明"画面摘要",不能替代事件级 L1。

### 4.5 保留(M3,仍拒绝)

| sub | kind | 用途 |
|---|---|---|
| `platform` | `platform_call` | ④ 后端呈现/音频/输入调用 |

解析器遇到 `platform_call` 直接返回 `trace.schema`,不做任何猜测性解析。

## 5. 摘要算法(argv / 宿主路径 / 输入时间线)

### 5.1 命令行 `argv`

规范字节序,逐项:`len:u64 LE + bytes`;`sha256` 覆盖全部项。

### 5.2 宿主路径 `HostPaths`

固定字段顺序 `system/save/appdata/personal/project`,每项:
`tag:u8`(0..4),`present:u8`(0/1),present 时追加 `len:u64 LE + bytes`。
`len` 字段 = `Some` 的条数(0..=5)。

### 5.3 输入时间线

`input.sha256` 覆盖 `[first, first + frames)` 每帧的**有效**输入(不是时间线显式帧数)。
新建 `Vm` 的首个逻辑帧是 **1**(`tick`/`tick_at` 先 `advance(scheduler.frame() + 1)`),
因此录制时 `first = 1`;非新建 VM 的回放用 `input_summary_range(timeline, first, frames)`。

`VmTrace::finish_with_timeline` 只覆盖 VM 实际消费的逻辑帧
(`VmTrace::consumed_frames()` = 调度器已推进到的帧号),不会多算一个未消费帧。
回放校验(`verify_input`)同时比较 `input.events` 与 `input.sha256`:前者要求调用方提供
**同一显式设定集合**的时间线,后者要求同一有效输入序列。

```text
repeat for frame in 0..frames:
  frame:u64 LE
  cursor.x:i64 LE
  cursor.y:i64 LE
  count:u64 LE
  repeat count 次: len:u64 LE, name_bytes
```

`input.events` 另记 `InputTimeline::len()`,用于区分「空时间线」与「有设定但全为空帧」。
摘要不含墙钟与主机信息;同一时间线 + 同一帧窗口 ⇒ 同一摘要。

## 6. diff 语义

### 6.1 原始严格 diff(`diff_text` / `diff_logs`)

- 先对两份文本各自做完整协议校验(非法 JSON → `trace.parse`;未知 schema/kind/字段 →
  `trace.schema`;末尾缺 LF 或空文件 → `trace.truncated`)。
- 然后**逐物理行比较原始字节**;某行字节不同即在该行内解析并定位首个语义分歧。
- 首个分歧报告 `{line, path, expected, actual}`:

| 字段 | 含义 |
|---|---|
| `line` | 1 起算的物理行号(header 为 1) |
| `path` | RFC 6901 JSON Pointer;`""` 表示整条记录(空白/键序差异无法定位到单键) |
| `expected` / `actual` | 该位置的值;缺失字段一侧为 `null` |

- 不做容差、归一化、模糊匹配;唯一例外是 header 中显式 `"volatile":true` 的字段——
  schema 3 **没有**此类字段。

### 6.2 逻辑对齐 diff(`diff_aligned` / `diff_aligned_logs`)

跨 producer 时 `seq` 没有可比性。逻辑键固定为:

```text
(sub, kind, frame, ordinal)
```

`ordinal` = 同一 `(sub, kind, frame)` 在应用投影后第几次出现(0 起)。对齐后:

1. header 按显式策略比较:`Ignore` / `CodecSample` / `Semantic` / `Strict`;
2. 记录按期望侧顺序逐键匹配,payload 内用 JSON Pointer 定位首个分歧;
3. 期望缺失/实际多余记录也报告为分歧(路径 `""`);
4. 报告 `{key, expected_line, actual_line, path, expected, actual, context}`;
   `context` 是期望侧前后各 N 条逻辑记录及两侧物理行号;
5. 投影支持按 `sub` / `kind` / `frame` 范围过滤;设置帧范围时 `frame = null`
   的启动链记录不参与;
6. 不排序、不去重、不做容差;跨 producer 对齐失败也必须原样报告。

## 7. 错误码

| 码 | 严重级 | 触发 |
|---|---|---|
| `trace.io` | Fatal | 文件读写失败 |
| `trace.parse` | Fatal | 非法 JSON、缺字段、字段类型不符、CR、空行、坏 hash 格式 |
| `trace.schema` | Fatal | 未知 schema/sub/kind/字段、seq 不连续、header 位置错、kind 与载荷不一致、保留的 `platform_call`、display 与摘要不一致 |
| `trace.truncated` | Fatal | 空文件、末尾缺 LF、行被截断(结构不完整) |
| `trace.length` | Fatal | 两份文件各自合法但记录条数不同 |
| `trace.replay` | Fatal | 回放时间线/argv/宿主路径/时钟序列/数据源编码与录制时不一致 |
| `trace.diff` | Fatal | 首个内容分歧(原始:行/Pointer;对齐:逻辑键/路径/期望/实际/上下文) |
| `trace.sample.missing` | Info | 环境 gate 缺样本,显式跳过并计数 |

## 8. 与 `docs/oracle.md` 草案的差异

| 项 | 草案 | 本实现 | 原因 |
|---|---|---|---|
| 封套 | `{frame, sub, kind, payload}` | `{schema, seq, frame, sub, kind, payload}` | `seq` 是同一 producer 的唯一序,必须有;`schema` 支持拒绝未知版本 |
| header | 未定义为记录 | `seq=0` 的 `meta/header` 记录 | diff 必须先比 header,且不能带外 |
| `sub` 取值 | `vm/graph/sound/input/save` | `meta/vfs/vm/frame`(+ 保留 `platform`) | ③ 已实装为帧描述;④ 等平台后端 |
| 路径/字节 | 未规定 | 每字节 U+00XX,`\u00xx` 转义 | 与 `VmEvent::Text` 的 CP932 字节一致,逐字节往返 |
| 浮点 | 未规定 | 最短往返 + 非有限值字符串 | 不丢精度且是合法 JSON |
| diff | 未规定粒度 | 原始逐行 + 逻辑对齐两套;显式 header 投影 | 跨 producer 不能按物理行号对齐 |
| 官方采集器 | 同一 schema | 本 crate 不实现采集器;`tools/` 采集器写同 schema | 任务书 §1:采集器为独立工具 |

## 9. 版本历史

| schema | 日期 | 变更 |
|---|---|---|
| 1 | 2026-10-19 | 初版:边界①(vfs open/exists/list)、边界②(全部 13 个 `VmEvent` 变体)、header、严格 diff |
| 2 | 2026-10-19 | 审查修订:header 补全启动配置(`script_key`/`debug_mode`/`argv` 摘要)与宿主注入项(`screen_color`/`media_readonly`/`os_sound`/`dx_mode`/`paths` 摘要),使"用 header 重跑"可闭合;输入摘要改为覆盖 VM 实际消费的帧 `[1, 1+N)`;`trace.length`、`trace.replay` 单列;`vfs/list` 失败态强制 `entries` 为空 |
| 3 | 2026-10-20 | producer-neutral:header 增加 `producer`(kernel/official/采集器版本/样本 hash)与 `clock`(tick_at now_ms 序列);③ `frame/frame_description` 实现(display JSON/摘要、内容寻址、level);新增逻辑对齐 diff `(sub,kind,frame,ordinal)` 与 header 投影;回放校验数据源编码、aux 帧句柄共享、输入摘要只覆盖实际消费帧 |
