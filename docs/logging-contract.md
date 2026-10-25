# 日志契约(Q1 定稿 · 2026-10-25)

> 单页契约:控制台行的唯一格式、级别词、频道表、颜色开关与 JSONL 行型。
> 实现唯一落点 `crates/yu-log/src/console.rs`(格式化/频道表/颜色解析)与
> `crates/yu-log/src/facade.rs`(门面桥);本文与其同步改。
> 色表事实参照:旧线 `<repo-old>/crates/yuris-log/src/level.rs:71..76`
> (2026-10-25 核对,只取数值;未搬代码)。

## 1. 控制台行格式(唯一)

所有**日志性质**的 stderr 行经同一格式化器:

```text
[  +12.345s] WARN  [gfx] gfx.imageop.region.partial @ s66 pc6028 x2: region partly outside source (:/W0/SPL401)
[   +0.102s] INFO  [host] log file %TEMP%/yu-cli-desktop.jsonl (facade level warn(default))
```

- 段 1:相对进程启动秒 `[{secs:>+9.3}s]`(测试未 `init_process_start` 时恒 `+0.000`);
  **着色时四段分档**:时间域暗灰 `[90m`;级别词加粗级别色
  `[1;{级别色}m`(粗体 = 与行体的亮度差);`[频道]` 按域独立色调
  (`channel_color`:vm=青/gfx=品红/sound=蓝/save=亮青/host=亮黄/
  log-facade=灰…,与级别色无关);行体全亮级别色;
  三段各自复位,多行行体逐行包色;
- 段 2:级别词 `{:<5}`;
- 段 3:`[频道]`;
- 段 4:行体 = `{码} @ {位置} x{次数}: {详情}`(位置/次数缺省时省略段)。

**位置显示**(S1):`{script} pc{pc}`(gfx 侧 `s66`;VM 侧为脚本路径)、
数据 `path+0xoff`、表 `table name`;多位置前 8 个逗号分隔,超出 `+N`。

## 2. 级别词(单一词表,两族来源)

| 词 | 来源 | 色(ANSI) |
|---|---|---|
| `FATAL` | `Severity::Fatal` | `1;37;41`(白底红) |
| `ERROR` | 门面 `log::Level::Error` / 宿主 `host_fatal!`(fail/panic 冻结) | `1;31`(亮红) |
| `WARN` | `Severity::Recoverable` / 门面 Warn / 宿主 Warn | `33`(黄) |
| `UNIMP` | `Severity::Unimplemented` | `33`(黄) |
| `INFO` | `Severity::Info` / 门面 Info / 宿主 Info | `32`(绿) |
| `DEBUG` / `TRACE` | 门面 Debug/Trace | `36`(青) / `90`(灰) |

不引入第二套级别集合(主人裁决 2026-10-25):生命周期行不新增级别,
由 `[host]` 等频道标识;`Severity::Info` 之外的细分不进词表。

## 3. 频道表(码表契约;新增必须进 `console::CHANNELS` 并过单测)

| 码前缀 | 频道 |
|---|---|
| `vfs.` | `[vfs]` |
| `fmt.` | `[fmt]` |
| `table.` | `[table]` |
| `rt.` | `[vm]` |
| `gfx.` | `[gfx]` |
| `snd.` | `[sound]` |
| `sv.` | `[save]` |
| `inp.` | `[inp]` |
| `platform.` | `[platform]` |
| `log.` | `[log]` |
| `host.` | `[host]` |
| (其他/宿主生命周期行) | 显式传入:`[host]` `[serve]` `[run]` `[dev]` `[case]`;门面行固定 `[log-facade]`;未登记码前缀兜底 `[core]` |

## 4. 可见性门槛(默认可见,主人裁决 2026-10-25)

| 级别 | 控制台默认 | 说明 |
|---|---|---|
| Fatal | 恒显示(红) | |
| Unimplemented / Recoverable | 显示(黄),同 key 聚合、带位置与计数 | 「没法排查」主诉的解 |
| Info(生命周期) | 显示(绿) | `--log-level quiet` 压掉(含末尾汇总) |
| 门面 Debug/Trace | 默认关 | `YU_LOG`/`--log-level` 维持旧语义 |
| 门面 Warn | 显示(`[log-facade]`) | 维持 |

- `--log-level quiet|error|warn|info|debug|trace`(全局):quiet/error/warn
  压内核 Info;info(缺省)/debug/trace 全显;debug/trace 同时放大门面级别。
- headless `run`:live 行不打,末尾打**诊断汇总**(频次降序,同受门槛);
  `--diag-summary off` 关闭。
- 桌面:observe 后同步控制台行(与 egui 面板同一次 drain,两路消费)。
- 聚合规则:**同 key**(码+位置+详情)命中只 `count += 1`,不重格式化;
  级别过滤在格式化**之前**。

## 5. 颜色开关

优先级:显式 `--color on|off` > `CLICOLOR_FORCE=1` > `NO_COLOR`(非空)⇒ off >
auto(stderr 是否终端)。

- auto 关闭时必须在 JSONL 留一行原因(「color auto→off: …;`--color on`
  可强制」)——tyn 实机「控制台日志全是白的」的教训,不允许静默无色;
- 多行消息逐行包色、空行不包、每行 `\x1b[0m` 复位;
- **stdout 无 ANSI**:`output::line/raw` 两个 stdout 出口无色无前缀;
  机器可读产品输出(case/report JSON、bench 表)永不经日志通道。
  守卫:`crates/yu-cli/tests/logging_guards.rs`(ANSI 字面量只许出现在
  `yu-log/src/console.rs`)。

## 6. JSONL 行型(单写者)

一次运行 = 一个现场(每轮截断;desktop/run `--log-file`/serve `--log-file`
同口径)。**一个文件一个句柄**(`JsonlFile` = `Arc<Mutex<File>>`),两种行共存:

```json
{"sub":"diag","code":"…","severity":"…","boundary":"…","where":{…},"detail":"…","causes":[]}
{"sub":"log","level":"warn","target":"…","file":…,"line":…,"message":"…"}
```

- `sub:"diag"` = `Diag::to_json_line` schema 前置一个 sub 键;
  `Diag::from_json_line` 忽略未知键,**往返契约不变**(归档:schema/往返
  契约与 yu-trace 取证通道不碰);
- 宿主生命周期行落 `sub:"log"`(level 小写)。

## 7. 宿主 `[频道]` 行(非 Diag)

生命周期/降级通知走 stderr,经 `Console::emit_host`(统一格式 + JSONL)。
桌面宿主宏(`host_line!`=INFO / `host_error!`=WARN / `host_fatal!`=ERROR)
经 `install_host_console`(进程级注入槽,`run()` 最先安装;宿主运维层
元数据,内核语义不读取)走同一 Console;槽未安装(极早期/嵌入/测试)
退化为纯 `[host] …` stderr 行——**凡 stderr 行必有频道或级别**,
stdout 只留产品输出。诊断消息里的路径一律 `{:?}` 调试格式
(空白/NUL/非法字符必须现形,不许显示成「空路径」)。

## 8. 守卫(提交前手跑清单,与 `case --all` 同级)

```bash
cargo test -p yu-cli --test logging_guards   # 三合一:终端宏零命中 /
                                            # ANSI 只在 console.rs /
                                            # silent_fallback_audit(S3 白名单)
grep -rn 'println!\|eprintln!\|dbg!\|print!' crates/*/src --include=*.rs | grep -v '^\s*//\|//!'   # 应零命中
grep -rn '\\x1b\|\\u{1b}' crates/*/src --include=*.rs | grep -v yu-log                            # 应零命中
grep -rn 'output::error(format!' crates/*/src --include=*.rs                                      # 首段均为 [频道]
```
