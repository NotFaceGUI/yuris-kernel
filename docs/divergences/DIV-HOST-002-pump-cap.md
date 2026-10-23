# DIV-HOST-002 · 可选 `--pump-cap`(实验性,默认关)

## 现象/动机

桌面宿主在 2026-10-23 的性能会话里引入了 `--pump-cap N`:
每个任务每次泵最多执行 N 条命令,超出则让出、下一帧从保存的 PC/栈续跑。
目标是给**任意脚本突发**(打開設定、场景切换等)一个通用兜底,避免单帧
几百 ms。

## 实测(必须保留)

语料 `<project-eris>`(官方 0.495 编译),宿主脚本
`target/tmp/perf/settings-reopen.json`(打开設定→戻る→再打开):

```text
# 默认(不设 cap,官方同构无限泵)
frame 300 engine_tick ≈ 264–270ms,无 Fatal

# --pump-cap 100000
frame 23:Fatal rt.label @ data\script\eris\button\es_button.yst:SYSTEM@2377 GOSUB
         main task ended;进程提前退出

# --pump-cap 500000
frame 9:Fatal rt.label @ 同一位置(es_button@2377 GOSUB)
        main task ended;frames=7 即退出
```

两次都在**同一个 GOSUB** 处失败,说明失败不是“某条命令被截断”,而是
任务在任意边界被抢占后,脚本依赖的**帧内任务交错/初始化顺序**被破坏。
脚本不是为可抢占执行设计的。

## 结论

- **不能默认开启**;`yu-desktop` 默认 `--pump-cap` 关闭,行为 = 引擎同构
  无限泵(官方语义)。
- 该开关只保留给诊断/失控对拍;日志会显式打印 EXPERIMENTAL 警告。
- **通用的正确修复是解释器吞吐**,不是抢占:
  预解码窗口、常量折叠、执行位表/命令缓存优化(见 `docs/perf-report.md` §2.5)
  让同一段脚本从 410–480ms 降到 264–270ms,且语义/像素门禁全绿。
- 若将来要重新考虑抢占,必须先证明脚本在任意命令边界可恢复
  (至少需要官方 trace 或脚本级安全点标注);当前证据是反例。

## 复现

```bash
yu-desktop <project-eris> --frames 760 \
  --script target/tmp/perf/settings-reopen.json --pump-cap 100000 \
  --log-file target/tmp/perf/cap100k.jsonl
# 期望(当前结论):日志出现 rt.label Fatal,main_finished=true
```
