# todo-format-generic · 旧代格式剩余面

> 快照:2026-11-14。已完成的部分见 `PROGRESS.md` 成果 `FMT-GENERIC`;
> 本文件只列**尚未闭合**的项,按“先样本、后代码”排序。
>
> 样本一律用占位符,不写真实游戏名/路径:
> `<sample-v2>` = 200<version<300 的 `ysbin/*.ybn` 包;
> `<sample-wz>` = 命中 WZ 标记的 `.ypf`;
> `<sample-v500>` = version 500/555 的 `.ypf`;
> `<sample-v222>` = version 0xDE 的 `.ypf`;
> `<sample-ycg>` = 含 `file_type=8`/`.ycg` 的 `.ypf`。

## P0 · 样本获取(所有代码项的前置)

| ID | 项 | 现状 / 阻塞 | 动作 | 验收 |
|---|---|---|---|---|
| T0.1 | YSTB V2 样本 | 无真实 200<v<300 语料;SDK 0.292 `YSCom` 需日文区域 + 版本特化 mapping,直接跑 exit 0 不产出 | 在日文环境/特化启动器下编译 0.292 `sample`;或引入 `<sample-v2>` | `ysbin` 下至少 1 个 `YSTB` version∈(200,300),且 `yst/ysv/ysl` 同代 |
| T0.2 | WZ YPF 样本 | 本机全部 `.ypf` 扫描 0 命中;WZ 反变换只有合成测试 | 找到 `<sample-wz>`(首 dword 低字节 `0x5A`),或确认某作品使用 WZ | 包内至少 1 条 `compressed=1` 载荷经 WZ 反变换后 zlib/Snappy 闭合 |
| T0.3 | v500/v555 YPF 样本 | V500 长度表已实现候选,无真包;YPF-Manager 用恒等表、yuri/YPF_Tool 用交换表,互相矛盾 | 引入 `<sample-v500>`;或仅以结构闭合验证现有 V500 候选 | 名字哈希 100% 命中,索引恰好闭合 |
| T0.4 | v222(0xDE)样本 | Script-Editor 称每条 +8 字节;无样本,未加 `Meta26` | 引入 `<sample-v222>` | 条目名/偏移/长度闭合,确认 8 字节归属 |
| T0.5 | YCG 样本 | 本地包扫描 0 命中;纹理扩展名无 `.ycg` | 引入 `<sample-ycg>`(至少 1 张) | 能解出 `(w,h,rgba)`,与官方/GARbro 对照 |

## P1 · 代码(有样本即可落)

| ID | 项 | 现状 | 动作 | 验收 |
|---|---|---|---|---|
| T1.1 | YSTB V2 解析 | `crates/yu-format/src/ystb.rs:103` 对 200<v<300 直接 `Unimplemented` | 按 `<sample-v2>` 的段索引/异或/窗口布局实现 V2 分支;保留 V5 路径 | 全语料 decode + re-encode 逐字节;错误码区分 `fmt.ybn.key`/`fmt.ybn.args` |
| T1.2 | YCG 解码 | `crates/yu-render/core/src/texture.rs:1758` 的 `EXTS` 无 `.ycg`;`file_type=8` 无消费者 | 在纹理层加 YCG 解码/注册;必要时单独 crate | 样本逐像素与对照一致;缺样本显式 `Unsupported` |
| T1.3 | YPF `Meta26`(v222) | `crates/yu-vfs/src/ypf.rs:383` 只有 `Meta18/Meta22` | 加 `Meta26`(18 字节 + 8 字节尾部)及候选;字段语义按 `<sample-v222>` 定 | 该样本闭合;0.495/v473 回归不变 |
| T1.4 | V500 长度表裁决 | `candidate_profiles` 已含 `V500`;无真包 | 用 `<sample-v500>` 决定恒等/交换表;删掉错误候选或保留为后备 | 名字哈希全命中;工具冲突有结论 |
| T1.5 | WZ 真包验证 | `wz_unscramble` 已实现,仅合成测试 | 用 `<sample-wz>` 验证:哈希是加密前还是加密后、是否整包/部分条目 | 真实包可读;哈希顺序修正;测试从 synthetic 升为 sample |
| T1.6 | YPF Snappy 真包验证 | zlib→Snappy fallback 已实现;无 YPF Snappy 真包 | 用 `<sample-v500>` 或 v555 样本验证 `compressed=1` 的 Snappy 分支 | 真实包解压长度闭合 |

## P2 · 宿主/工程面

| ID | 项 | 现状 | 动作 | 验收 |
|---|---|---|---|---|
| T2.1 | 内嵌 YPF 自动挂载 | `YpfSource::open` 已支持 `MZ→YSER→YPF`;`Project::discover` 只扫 `.ypf` | 决定是否扫 `.exe`;建议只在显式 `--data <exe>` 启用,避免误挂 installer | 显式路径可 `run`;发现策略有文档 |
| T2.2 | 旧 YPF 方言文档 | `docs/formats/ypf.md` 仍是 0.495 单布局口径(flag 0/2 等已过时) | 改为 profile 表:Meta18/22、V000/V004/V010/V500、CRC/Adler/Murmur、Snappy/WZ、内嵌 | 文档与实际代码/样本一致 |
| T2.3 | v408 内嵌样本回归 | 已有真实 installer 样本(28 条),只做 env 门控测试 | 加入 `docs/divergences` 证据锚点;如需可做 CI 可选 oracle | 样本缺失显式 skip;有样本时 28/28 |

## 明确关闭(不是 TODO)

- NUL-XOR 名字方言:无官方证据,保持 `Unimplemented`(DIV-002)。
- 0x24 索引起点:旧线自洽不等于官方,保持拒绝。
- YSTB 自定义 XOR key:`yu-engine/src/keys.rs` 已支持投票/校验。
- YPF V004/V010 长度表:本轮已实现并有合成测试。
- EXE 内嵌 YPF 读取:本轮已实现并有 v408 真样本。
- YSSD v474 偏移表垃圾槽:本轮已按块数顺序解析,官方 0.495 roundtrip 仍逐字节一致。

## 验证命令模板

```bash
export TEMP="<tmp>"; TMP="$TEMP"
export YURIS_SAMPLE_LEGACY_YPF="<sample-v473>"
export YURIS_SAMPLE_EMBEDDED_YPF="<sample-embedded-v408>"
export YURIS_SAMPLE_LEGACY_SAVE="<sample-save-dir>"
export YURIS_SAMPLE_V2="<sample-v2>"
export YURIS_SAMPLE_WZ="<sample-wz>"
export YURIS_SAMPLE_V500="<sample-v500>"
export YURIS_SAMPLE_YCG="<sample-ycg>"

cargo test -p yu-vfs -p yu-format -p yu-save -p yu-gfx
cargo clippy -p yu-vfs -p yu-format --all-targets
```

新增样本时:先写 env 门控 oracle,断言“索引闭合 + 名字哈希全命中 + 载荷长度闭合”,
再动解析器;禁止用样本名/路径写死进代码。
