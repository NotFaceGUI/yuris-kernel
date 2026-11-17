# 低版本(2xx/3xx)支持计划

> **落地状态(2026-11-17)**:V2 格式/运行核/密钥/标签/VM 语义均已实现,
> 0.292 与 0.255 均跑到标题;验证见 `PROGRESS.md` 成果 L-1,
> 布局与勘误见 `docs/divergences/DIV-007/009/010..016`。本文保留为技术基线。
>
> **计数勘误(2026-11-17 实测)**:0.255 语料是 **96** 个 YSTB(非 97);
> 非零控制流目标 0.292 = **2,590**、0.255 = **2,279**(计划里的 4,412/3,897
> 按全部控制窗口/其它口径统计)。以语料实测与 DIV-007 为准。
>
> 快照: 2026-11-17。范围 = 让内核读取并运行官方 0.292 / 0.255 的 ERIS 示例，
> 并为 0.3xx–0.4xx 的版本差异留出回归位。本文只定计划，不改生产代码。
>
> 样本占位符：
> - `<sample-0292>` = 官方 0.292 SDK 的 `sample` 工程（本轮已由官方 YSCom 编译）
> - `<corpus-0292>` = 上述编译产物（119 个 `.ybn`，其中 112 个 YSTB）
> - `<sample-0255>` = 参考仓库 `yuris_decompiler` 的 `example-files/v255.ypf`（官方 0.255 示例）
> - `<corpus-0255>` = 从 `<sample-0255>` 解出的 `ysbin`（103 个 `.ybn`，其中 97 个 YSTB）
> - `<official-engine-0292>` = 官方 0.292 `yu-ris.exe`

## 1. 目标与边界

**目标**

1. 内核能完整读取 `<corpus-0292>` 与 `<corpus-0255>`：头、表、脚本、控制流目标、密钥自检。
2. `yu-cli run` 能把 0.292 示例跑到标题画面；再以 0.255 作为更老语料回归。
3. 所有格式层读写保持字节级往返；无证据处继续显式失败，不猜。

**边界**

- 参考实现的最低公开版本是 0.200；**低于 0.200 不在计划内**（无语料、无格式证据）。
- 5xx / 商业魔改版不在本轮；它们只作为“版本号不可信、必须按结构裁决”的反例。
- V2 文本改写（翻译回写）不是本轮前置；先保证只读运行，回写能力随 `Segments` 往返一起保留。

## 2. 本轮新增证据（已实测）

### 2.1 官方 0.292 示例已被编译并可被官方引擎启动

- 官方 0.292 `YSCom.exe` 可由 32 位宿主按映射协议驱动；64 位宿主创建的命名映射会被 32 位编译器以
  `Access Denied` 拒绝，**宿主必须 32 位**。
- 编译产物：119 个 `.ybn` / 1,099,058 字节；`YSTB version = 292`。
- `<official-engine-0292>` 使用这批产物能起到标题窗口（`ERIS Ver0.92 [デバッグモード]`）。
- 0.292 映射2 条目布局：`{u32 nameSize, name[nameSize], u32 fileSize, data}`；载荷为**明文**，
  不能按 V5 的旧假设做整文件 XOR（整文件 XOR 会把 `YSCF`/`YSTB` 魔数打坏）。

### 2.2 V2 脚本结构已在两个官方语料上闭合

用独立脚本按以下布局走查：

```text
+0x00  "YSTB"
+0x04  u32 version
+0x08  u32 code_size
+0x0C  u32 expr_size
+0x10  u32 expr_offset == 0x20 + code_size
+0x14  u32[3] padding（0）
+0x20  code[code_size]     ← XOR，密钥从下标 0 重新开始
+...   expr[expr_size]     ← XOR，密钥从下标 0 重新开始
```

指令与参数：

```text
code 段逐条:
  u8  op            // ysc/ycd 命令表下标
  u8  argc
  u32 line
  RETURNCODE: 4 字节（version == 290 时 8 字节）
  其它: argc × 12 字节 {u16 id, u8 typ, u8 aop, u32 expr_size, u32 expr_offset}
```

结果：

| 语料 | 版本 | 脚本数 | 指令数 | 结构闭合 | 控制流目标 |
|---|---:|---:|---:|---|---|
| `<corpus-0292>` | 292 | 112 | 20,117 | 全部 | 4,412 个 IF/ELSE/LOOP 目标**全部是 code 段字节偏移** |
| `<corpus-0255>` | 255 | 97 | 16,351 | 全部 | 3,897 个目标**全部是 code 段字节偏移** |

- `YSLB` 的 `code_position` 在 V2 也是 code 段字节偏移：0.292 的 1,189 个标签、0.255 的 1,026 个标签
  全部落在指令起始地址上。
- **opcode 不能硬编码**：0.292 的 `RETURNCODE=0x38`、`WORD=0x54`；0.255 的 `RETURNCODE=0x36`、
  `WORD=0x51`。必须按工程 `ysc.ybn` 的命令名解析。开源文本工具里的 `0x38`/`0x54` 只是特定版本的
  经验常量，不能作为格式事实。

### 2.3 现有表解析器已能覆盖大部分 2xx 表

用本仓库 `yu-format` 直接解析两套语料：

| 表 | 0.292 | 0.255 | 结论 |
|---|---|---|---|
| `ysc.ybn` | 91 命令 | 88 命令 | 已闭合 |
| `ysv.ybn` | 2,393 条，no-flag | 2,101 条，no-flag | 已闭合 |
| `ysl.ybn` | 1,189 标签 | 1,026 标签 | 已闭合（偏移语义待转换） |
| `yst_list.ybn` | 116 条，old16 | 100 条，old16 | 已闭合 |
| `yst.ybn` | var=3422 | var=3032 | 已闭合 |
| `yse.ybn` | 122 条 | 119 条 | 已闭合 |
| `yscfg.ybn` | **失败** | **失败** | 唯一需要新增布局的表 |

### 2.4 YSCF 布局族（实测字节）

| 版本 | 文件长 | 画面宽 | 画面高 | 标题长 | 标题 | 说明 |
|---|---:|---:|---:|---:|---|---|
| 0.255 | 62 | +0x14 | +0x18 | +0x30 | +0x32 | 短布局，无后续 slots |
| 0.292 | 78 | +0x14 | +0x18 | +0x40 | +0x42 | 2xx 布局 |
| 0.459/0.466 | 78 | +0x10 | +0x14 | +0x40 | +0x42 | 现有 `Old` |
| 0.479/0.495 | 90 | +0x10 | +0x14 | +0x4C | +0x4E | 现有 `New` |

0.292 的 `+0x10` 是额外的零字段；宽度/高度整体后移 4 字节。0.255 的固定区更短，标题直接落在
`+0x30`。候选必须按“宽高范围 + 标题长度 + 文件尾精确闭合”裁决，不能只看版本号。

### 2.5 YPF 低版本：0.255 包已能被本仓库挂载

- 本仓库 `YpfSource` 能打开 `<sample-0255>` 并读出 `ysbin/*`；首错不是 YPF，而是 YSCF 布局。
- 参考实现给出的 YPF 版本轴：
  - 名字 XOR：默认 0xFF，290 为 0x40，500 为 0x36；
  - 长度表：`<500` 用 V000 族，500 用交换表；
  - 哈希：`<265` 无哈希，265–463 用 CRC32 名字 + Adler32 负载，464+ 用 MurmurHash2；
  - 偏移宽度：`<470` u32，470+ u64；
  - `header_size` 语义：`<300` 不含 32 字节头，`>=300` 含。
- 现有 `candidate_profiles()` 已覆盖 V000/V004/V010/V500 长度表、Meta18/Meta22、CRC/Adler/Murmur；
  **缺“无哈希”族**（`<265`）。0.255 样本可作该族的第一个 oracle。

## 3. 开源项目调研（扩大后的清单）

| 项目 | 语言/许可 | 低版本能力 | 可用点 / 结论 |
|---|---|---|---|
| `shimamura-sakura/yuris_decompiler` | Python / **MIT** | 0.247–0.494；V2/V5；YPF、YSCM、YSCF 除外、YSER、YSTD、YSLB、YSTL、YSVR；含官方 0.255 样本 | **首选参考**。完整 decompile/recompile，0.255/0.488/0.494 实测；算法可按 MIT 采用并致谢 |
| `shimamura-sakura/yuri` | Python / **无许可** | 0.200–0.500；编译器 + 反编译器；YCD 450–494；`yuri.md` 技术说明 | 版本差异、表达式 tyq、特殊命令、商业魔改说明的**事实来源**；不复制代码 |
| `ZQF-ReVN/RxYuris` | C++ / GPL | V2 文本、YSCM、YSTL、YSCom Loader | V2 头/指令/参数布局的第一手对照；已有采用记录 |
| `Aionfatedio/YU-RIS-Script-Editor` | Python / **无许可** | V2/V5 文本、密钥/编码自检 | V2 的 `argc*12+6` 扫描与 `0x38`/`0x54` 经验常量；只作事实参考 |
| `yy487/VisualNoveltools` | Python / **无许可** | V2/V5 文本、追加式回写、中文流程 | 独立确认 V2 头与 `op=0x54` 文本经验；事实参考 |
| `alessandrocaetanob/vnforge` | C# / GPL 混合 | **仅 V5**；YPF 2xx–5xx 版本矩阵、密钥表示法 | 明确不实现 V2；YPF 版本轴/键字节序说明很有价值，事实参考 |
| `UserUnknownFactor/GARbro2` | C# / MIT | 有 V2 支路 | **V2 头字段读法与官方样本不符**（把 code/expr 当成 count/code/arg/res/off）；只作反例，不能当权威 |
| `bfloat16/vnlens` | Rust / **无许可** | 仅 V5（300–555） | 密钥候选：全量结构 + argc 统计 + 多候选拒绝；V2 无实现 |
| `1F1E33-float32/Tools`、`crskycode/YuRis_Tool` | Python/C# / 无许可 | 仅 V5 | 命令/表达式语义与 V5 流；不是 V2 来源 |
| `fengberd/YuRISTools` | C# / MIT | 声明 234–490，实际按 V5 读 | 只作 V5/YPF 对照；不能证明 V2 |
| `CodeSpoof/extYuRis`、`regomne/chinesize` | Go / 无许可 | 仅 V5 | 文本/密钥/打包流程；V5 |
| `arcusmaximus/VNTranslationTools` | C# / MIT | YU-RIS 笔记（V5） | 表达式 VM opcode、表头、YSLB 字段的原始笔记；事实来源 |
| `TheVNConnoisseur/YU-RIS_Guide` | 归档 / 无许可 | 81 个官方 SDK | ABI 表与 `YSCom.ycd` 的来源（已采用） |

搜索范围：GitHub repos/code（多语言、多关键词）、Gitee、GitLab、npm、crates.io、PyPI、Sourcegraph/grep.app
（后两者被防火墙/验证页拦截）。结论：**V2 的可靠实现集中在 `yuris_decompiler` 与 `yuri`**；
其余项目要么只做 V5，要么在 V2 上有明显误读。Gitee/GitLab/包注册表没有发现新的 YU-RIS 解析器。

## 4. 版本差异矩阵（实现时必须按结构裁决）

| 项 | `<290` | `290` | `291–299` | `300–469` | `470–480` | `481+` |
|---|---|---|---|---|---|---|
| YSTB 布局 | V2 | V2，RETURNCODE 8B | V2，RETURNCODE 4B | V5 | V5 | V5 |
| 默认 XOR 键 | 07 B4 02 4A | D3 6F AC 96 | D3 6F AC 96 | 同左 | 同左 | 同左 |
| YSLB `code_position` | 字节偏移 | 字节偏移 | 字节偏移 | 指令下标 | 下标 | 下标 |
| YSTL 记录尾 | 16B | 16B | 16B | 16B | 20B（470+） | 20B |
| YSVR 记录头 | 7B | 7B | 7B | 7B | 7B | 8B（481+） |
| YSCF 候选 | V255（0.255） | V292/短布局 | V292 | Old（宽度 +0x10） | Old/New | New |
| YPF 名字哈希 | 无（<265）/CRC+Adler（265+） | CRC+Adler | CRC+Adler | Murmur（464+） | u64 offset（470+） | 同左 |

> 商业版可能用任意版本号承载旧布局（参考 `yuri.md`：490 用 pre-481 布局、554 也用旧布局）。
> 因此所有布局选择都先结构闭合，再考虑版本号提示；多候选通过时按版本提示/精确闭合裁决，无法裁决则失败。

## 5. 实施计划

### P0 · 证据与夹具（0.5 天）

- 把 0.292 编译配方（32 位宿主、映射1/2、条目布局、明文载荷）写入 `docs/` 证据页。
- 约定环境变量：
  - `YURIS_SAMPLE_V2_292` = `<corpus-0292>`；
  - `YURIS_SAMPLE_V2_255` = `<corpus-0255>`；
  - `YURIS_SAMPLE_V2_YPF` = `<sample-0255>`。
- 缺少样本的测试显式 skip 并计数，禁止空跑通过。
- 更新 `docs/references/open-source-tools.md`：新增本文 §3 的项目、许可与“仅事实参考/可算法采用”状态。

### P1 · 格式层（2–3 天）

1. `crates/yu-format/src/ystb.rs`
   - 新增 V2 分支：32 字节头、code/expr 两段、6 字节指令头、12 字节参数；
   - `RETURNCODE` 按 `CommandTable` 名字识别：版本 290 读 8 字节，其余读 4 字节；
   - `IF/ELSE/LOOP` 的第二/第三参数 `expr_size` 是 **code 段字节偏移**，解码时转成组下标；
     目标不在指令起始地址 = Fatal；
   - `WORD` 的表达式数据是裸字符串；`_` 仍按表达式处理；
   - 合成 V5 视图供运行核使用：`inst`（count×4）、`args_index`（count 窗口×12）、
     `args_data = expr`、`line_numbers = 各行号`；同时保留 V2 原始 code/expr/padding/trailing，
     供 `encrypt` 逐字节回写；
   - `Segments` 增加布局判别（V5/V2）与 `encrypt` 分支；V2 只保证只读/原字节回写，
     文本修改暂不承诺。
2. `crates/yu-format/src/yscf.rs`
   - 布局候选：`V255`（宽/高 +0x14/+0x18，标题 +0x30）、`V292`（+0x14/+0x18，标题 +0x40）、
     现有 `Old`/`New`；
   - 字段读取改为布局驱动的安全切片；短布局缺失区按 0/默认值；
   - 选择规则：宽高范围、标题长度、文件尾闭合；多候选成功 = 不确定 → 失败（或按版本提示唯一裁决）。
3. `crates/yu-engine/src/keys.rs`
   - V2 头解析与结构校验：`0x20 + code_size == expr_offset`、两段在文件内、指令流恰好闭合、
     参数引用不越界、opcode 在 `ysc` 表内；
   - 候选键：0、默认 `D3 6F AC 96`、旧键 `07 B4 02 4A`、文件 `+0x2C` 读数（RxYuris 路线）、
     首参数 offset/行号启发式；多个候选同时通过 → 失败；
   - 明确 `--script-key` 是**字节序列十六进制**（0.292 用 `d36fac96`，0.255 用 `07b4024a`）。
4. `crates/yu-format/src/yslb.rs`
   - 线格式不变；文档写明 `<300` 的 `field_b` 是偏移；运行核负责转换。
5. 表解析回归
   - 为 0.292/0.255 的 `ysc/ysv/ysl/yst_list/yst/yse` 增加 env 门控样本测试；
   - YSCF 两套新布局各一条单测 + 两套真样本。

**验收**：两套语料全部表解析通过；`Segments::decrypt` 对 112/97 个 V2 脚本全部闭合；
V2 原字节回写逐字节一致；现有 0.495 语料回归不变。

### P2 · 运行核投影（2–4 天）

1. `crates/yu-rt/src/tables.rs`
   - `ScriptFile::parse` / `Segments::decrypt` 传入 `CommandTable`（V2 识别 RETURNCODE 必需）；
   - 合成视图 → `GroupRec/WindowRec/pool/part4`；`GroupRec.param = 0`（V2 无 gosub_npar）；
   - 控制窗口的 `length` 已是组下标；`offset` 保留原 target expr 偏移（VM 不消费）；
   - 为 V2 建立 `code_offset -> pc` 映射（`ScriptFile` 暴露查询）。
2. `YSLB` 标签转换
   - `<300` 时把 `field_b` 偏移转成组下标。标签表在脚本之前解析，建议：
     - 启动时对 V2 按 `yst_list` 逐脚本读 code 段、只走指令头建立偏移集合（不完整解码），
       转换全部标签后再释放；或
     - 惰性解析：首次跳转时确保目标脚本已装载并转换。优先选前者（V2 语料小、失败早、实现简单）。
   - 目标偏移不在指令起始地址 = `fmt.*` Fatal。
3. 启动链
   - 0.292 工程（`data/ysbin`）headless 跑到首个可绘制帧；逐项收口表/配置/标签/ABI 诊断；
   - 0.255 包（YPF）同样跑到首个可绘制帧。
4. 配置/ABI
   - YSCF 800×600、扩展名优先序缺省值；
   - `SysVarTable::select_for` 已含 0.255/0.292 表；验证 `ysv` 指纹选表与实际 `YSD`/`ysv` 一致；
   - 失败时保留显式诊断，不静默降级。

**验收**：`yu-cli run <0292 proj> --mode headless --frames N` 无格式 Fatal，出首帧；
0.255 同样；`--json`/截图与官方引擎标题画面对照（尺寸/标题/首帧非空）。

### P3 · VM 语义（3–7 天，按实际缺口）

1. 表达式 tyq
   - V2：`0x23` = 旧式 `$@`（数值转字符串）；V300：`0x23` = `&$`、`0x60` = `&@`；
   - 现有 `VarSpace::from_prefix` 把 `0x23/0x60` 当旧变量空间，必须按脚本版本分支；
     先以 `yuris_decompiler` 的 `InsTyqV200/V300` 为待验证事实，再用官方引擎行为确认。
2. `RETURNCODE`
   - 0.255/0.292 是 4 字节参数（无 size/offset）；290 是 8 字节（含 size）；
   - 确认 VM 对“返回值”的取法；无法确认时显式失败，不把 0 当值。
3. `GOSUB/RETURN`
   - V2 无 `gosub_npar`；检查现有 handler 是否只依赖窗口序列；
   - 与官方 0.292 的 `es_*.yst` 行为对照。
4. 命令覆盖
   - 从两套语料的实际执行路径收集“未实现/降级”诊断（不要只静态比对 handler 名称；
     `CG`/`SAVE`/`MENU` 等走域处理，不在 `HandlerKind` 表里）；
   - 逐条补实现，证据锚点 = 官方 0.292 引擎地址/手册页；手册无页时按 `fixed(no-manual)` 记账。
5. 变量作用域
   - 核对 `kind` 1/2/3 与 `scope_ex` 1..3：参考实现为 1=Global、2=Static、3=Folder；
     本仓库当前注释把 2/3 写反但运行过滤可能是对的（1|3 全局、2 脚本局部）；
     以 0.292/0.255 实际运行验证并修正注释/测试。

**验收**：0.292 从标题进入一段官方示例流程（日志/画面/事件与官方引擎一致到可判定粒度）；
0.255 至少到标题；所有未实现点有台账，不以 `Unimplemented` 作终态。

### P4 · YPF 2xx（1–2 天）

- `candidate_profiles()` 增加“无哈希”族（`<265`）：Meta18，名字/负载哈希字段为 0 或忽略；
- 名字 XOR 的版本提示：默认 0xFF、290 = 0x40、500 = 0x36；保留全 256 key 兜底但必须结构闭合；
- 用 `<sample-0255>` 做 `YURIS_SAMPLE_V2_YPF` oracle：条目数/名字/偏移/长度闭合；
- 如需 0.292 打包形态，再补 v2xx 真包；不把“散盘可用”当成“旧包已支持”。

**验收**：0.255 YPF 全量条目读取 + 至少一个条目字节与参考解包一致；现有 0.495/v473 回归不变。

### P5 · 收口（1–2 天）

- `DIV-007`、`DIV-009` 改为 fixed，记录 V2 布局、密钥、控制目标、opcode 名称解析；
- 如需，新增 `DIV-FMT-*`：YSCF 布局族、YSLB 偏移、V200 tyq、RETURNCODE 宽度；
- `PROGRESS.md` 按“成果”格式追加验证命令与原始结果；
- `docs/references/open-source-tools.md` 与 README 致谢按实际采用更新；
- 全量回归：`cargo test -p yu-format -p yu-rt -p yu-engine -p yu-vfs`，0.495 语料逐字节不变。

## 6. 测试矩阵

| 层 | 用例 | 样本/断言 |
|---|---|---|
| 单元 | V2 头/指令/RETURNCODE/IF-ELSE-LOOP 目标 | 合成字节；目标非指令起点必失败 |
| 单元 | YSCF 四布局 | 合成 + 0.255/0.292/0.466/0.479/0.495 头 |
| 单元 | YSLB 偏移→pc | 合成脚本；目标越界/非起点失败 |
| 语料 | `<corpus-0292>` 112 脚本 | 头闭合、命令数、参数引用、控制目标、原字节回写 |
| 语料 | `<corpus-0255>` 97 脚本 | 同上；旧键 07 B4 02 4A |
| 语料 | 表解析 | ysc/ysv/ysl/yst_list/yst/yse 全量闭合 |
| 集成 | 0.292 headless | 首帧非空、无 `fmt.*` Fatal、配置 800×600 |
| 集成 | 0.255 YPF | YPF 条目读取 + 首帧 |
| 对照 | 官方 0.292 | 标题/首帧/关键命令日志 |
| 负例 | 错键、截断、目标非边界、RETURNCODE 宽度错、YSCF 多候选 | 显式 Fatal；不产出垃圾 |

## 7. 风险与裁决点

1. **RETURNCODE 4 字节的返回值语义**：0.255/0.292 没有 size 字段；参考实现说 `expr_size` 是返回值，
   但只对 290 的 8 字节形态成立。需要官方引擎行为/手册裁决；宁可显式失败。
2. **V200 tyq 0x23**：参考说 `$@`（数值转字符串），本仓库按旧变量空间处理；必须按版本分支并用
   官方执行结果验证。仅 22/31 处出现，可能不影响标题，但会影响剧本。
3. **opcode 硬编码陷阱**：`RETURNCODE/WORD` 的 opcode 随 ysc 表变化；禁止按 0x38/0x54 写死。
4. **YSCF 短布局**：0.255 的 62 字节布局字段不全；缺省扩展名顺序/开关要确认引擎默认值。
5. **标签/控制流偏移转换**：V2 的 YSLB 与 IF/LOOP 目标都是 code 段偏移；转换必须在启动/解码期
   完成，否则 VM 会把偏移当组下标，产生“看似能跑”的错位。
6. **YPF `<265` 无哈希族**：现有 profile 要求全量名字哈希命中；不补该族会拒绝 0.255 之前的包。
7. **商业魔改版本号**：490/554 可能承载旧布局；选择必须结构优先，不能 `version < 300` 一刀切。
8. **命令覆盖**：ERIS 0.292/0.255 的旧命令/宏可能缺实现；以实际 boot/trace 为准，逐条台账。

## 8. 建议顺序与产出

1. **先 P1 + P2（只做 0.292）**：目标 = 0.292 示例在我们的内核出首帧。这是最短闭环，
   也能验证 V2 解码/标签/控制流三件事。
2. 再用 0.255 回归 P1/P2（旧键、62 字节 YSCF），确认不是“292 特化”。
3. 之后按 P3 的实际诊断缺口决定投入；不要在没有 boot 证据前先补旧命令。
4. P4 与 P5 在 0.292/0.255 都能起标题后收口。

预计：P0 0.5 天，P1 2–3 天，P2 2–4 天，P3 3–7 天（取决于缺口），P4 1–2 天，P5 1–2 天。

## 9. 参考与许可

- **可算法采用**：`shimamura-sakura/yuris_decompiler`（MIT，需在文件头/README 致谢）。
- **仅事实参考（不复制代码）**：`shimamura-sakura/yuri`（无许可）、`YU-RIS-Script-Editor`（无许可）、
  `yy487/VisualNoveltools`（无许可）、`bfloat16/vnlens`（无许可）。
- **算法/事实（已有采用记录）**：RxYuris（GPL）、`YU-RIS_Guide`（官方 SDK 产物）。
- **反例/不作为权威**：GARbro2 V2 支路（头字段与官方样本不符）。
- 所有采用处按仓库纪律在文件头注明来源，并在 `docs/references/open-source-tools.md` 登记许可状态。
