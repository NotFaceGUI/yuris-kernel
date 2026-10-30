# `yu-rt/examples` 收敛清单(2026-10-23)

> 只出清单,**不删文件**(主人裁决)。当前 45 个 example。
> 判据:是否仍被 `docs/`、`crates/yu-rt/docs/`、测试引用;是否是可复用的
> 证据工具;是否只是一次性取证脚本。
>
> 复核命令:
> ```bash
> for f in $(ls crates/yu-rt/examples | sed 's/\.rs$//'); do
>   echo "$(grep -rl "\b$f\b" docs crates/yu-rt/docs crates/yu-rt/tests | wc -l) $f";
> done | sort -n
> ```

## A. 保留(证据/回归工具,建议加 README 一行)

| 文件 | 理由 |
|---|---|
| `coverage_census.rs` | 命令覆盖普查(5 处引用);性能/覆盖率基线 |
| `run_eris.rs` | 官方 ERIS 启动链回归(3 处引用) |
| `gfx_census.rs` | gfx 命令/图层普查(P2 取证) |
| `dump_yscm.rs` / `dump_ycd.rs` | 命令表/定义表取证(各 1–2 处引用) |
| `find_var_ref.rs` | 变量引用扫描;DIV-RT-003 取证工具(本会话扩了槽位初值输出) |
| `audit_sysvar_manual.rs` | 系统变量手册对账(P3 证据) |
| `version_probe.rs` | 版本/ABI 探测 |
| `e2e_ypf.rs` / `extract_ypf.rs` / `cmp_eris_pack.rs` | YPF 数据入口/打包对照 |

## B. 合并后删除(同类一次性工具)

| 现有 | 建议合并到 |
|---|---|
| `dump_one` / `dump_words` / `dump_str_arrays` / `dump_abi_range` / `dump_codes` / `dump_script_paths` / `dump_ysv_one` | `dump_table <kind> [args]`(kind = one/words/strarr/abi/codes/paths/ysv) |
| `find_negidx` / `find_prefix` / `find_rc_payload` / `find_str` / `find_synth` / `find_task_tint` / `find_ycd` / `find_var_ref` | `find_ref <mode>`(var/bytes/prefix/negidx/task) |
| `negidx_detail` / `negidx_seq` | `find_ref negidx --detail/--seq` |
| `t_rows` / `t_tddef` / `track_t` / `who_fills_t` | `trace_t <mode>` |
| `trace_if` / `trace_taskinfo` / `trace_eris` | `yu-trace` 的 record/replay/diff 已有正式入口;保留一个最小 repro |
| `probe_varinfo` / `dbg_varinfo` | `find_var_ref --slot`(槽位/声明/初值) |
| `dbg_code_binding` | `dump_codes --binding` |

## C. 可直接删除候选(一次性现场;删除前先 grep `PROGRESS.md`)

| 文件 | 说明 |
|---|---|
| `check_dims.rs` | 早期视口/尺寸核对 |
| `boot_events.rs` | 启动事件一次性 dump(现由 `yu-trace` 覆盖) |
| `text_scenario.rs` | 文本场景手工回放(现由 `yu-gfx` 测试覆盖) |
| `last_dialog.rs` | DIALOG 取证现场(P3 已收口) |
| `diag_census.rs` | 诊断码普查(P1 面板已覆盖) |
| `bench_eris.rs` | 早期基准;`yu-cli bench` 取代 |
| `t_rows`/`t_tddef`/`track_t`/`who_fills_t` | 见 B;若合并落地则删 |

## D. 保留但标注“一次取证,勿当 API”

- `audit_sysvar_manual.rs`、`version_probe.rs` 依赖外部 SDK 路径
  (`YURIS_SAMPLE_SDK0479/0495`);缺样本时必须显式报错/跳过计数,
  不要改成静默空跑(与 `AGENTS.md` §2 一致)。

## D2. 样本环境变量一览(测试/探针共用)

缺样本的用例必须显式报错或显式跳过并计数;`YURIS_REQUIRE_SAMPLES=1` 时
禁止跳过(直接失败)。

| 变量 | 指向 | 用途 |
|---|---|---|
| `YURIS_SAMPLE_ERIS_YSBIN` | `<project-eris>/data/ysbin` | `yu-rt` 表/脚本一致性对拍 |
| `YURIS_SAMPLE_ERIS_ROOT` | `<project-eris>` | 存档 golden(`save/*.sd`)、工程级用例 |
| `YURIS_SAMPLE_SDK0479` / `_SDK0495` | SDK 安装目录 | 系统变量 ABI / 命令表跨版本核对 |
| `YURIS_SAMPLE_PAC481` | 商业包根(`pac/` 多包布局) | `yu-engine` 包布局断言;`yu-format` 存档块压缩层往返(0.481 = Snappy) |

## E. 不动的宿主证据工具

`crates/yu-platform/host-desktop/examples/{scenario_probe,serve_probe}.rs`
不在本次 `yu-rt/examples` 清单内,但同属证据工具:
`scenario_probe` 新增 `--hash-csv`(逐帧像素 SHA,性能改动像素门禁用),
`serve_probe` 继续做 `/ws` 帧/输入回放。
