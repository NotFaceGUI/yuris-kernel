# v555 vs 0.495 命令差异与同名异构取证

- 最后核对:2026-11-21
- 范围:`ysc.ybn` 运行时命令表(名称、槽位、参数表)与已取证的处理器语义
- 样本:0.495 = `<side-repo>/workbench/project-eris`;555 = `<games-555>/<v555-2>`
- 结论:**同名不等于同 ABI**。555 相对 0.495 有 5 条新增、2 条删除、14 条同名参数表变化;另有 8 组同名同参数但语义不同。所有 555 语义必须按 `SemanticProfile`/精确 555 分派,禁止按槽号或按 0.495 表硬编码。

---

## 1. 证据与复现

### 1.1 证据源

| ID | 证据 | 用途 | 可信度 |
|---|---|---|---|
| E1 | 0.495 工程 `ysc.ybn` 参数表现场探针 | 495 名称/槽位/参数序列 | 观测 |
| E2 | `<v555-2>` 工程 `ysc.ybn` 参数表现场探针 | 555 名称/槽位/参数序列 | 观测 |
| E3 | `docs/formats/v555-command-handlers-all.tsv` | 6 个 555 构建 × 121 槽;opcode、argc、class、handler 地址 | 观测(合并表) |
| E4 | `docs/formats/v555-commercial.md` §6.3 | 独立脚本 `ysc_diff2.py` 的逐序列对齐结果 | 观测(交叉验证 E1/E2) |
| E5 | 0.495/555 处理器反编译锚点 | 同名异构语义 | 观测(地址见 §4) |
| E6 | `crates/yu-rt/src/{semantics.rs,other/}` 与测试 | 当前内核实现/门控状态 | 实现事实 |

E1/E2 与 E4 的 14 条参数变化一致;E3 的 handler 地址用于复查语义差异。**E4 的部分运行时结论已过时**(见 §7 勘误),但 §6.3 的纯表结构对比仍有效。

### 1.2 复现命令

0.495 全表(该入口有 `data.ypf`,可直接 `dump_yscm`):

```bash
export YURIS_SAMPLE_ERIS_ROOT="<side-repo>/workbench/project-eris"
cargo run -q -p yu-rt --example dump_yscm > /tmp/yscm495.txt
```

555 单命令参数(555 发行包是 `pac/*.ypf`,用 `Project::discover` 的探针逐名查询):

```bash
G="<games-555>/<v555-2>"
cargo run -q -p yu-engine --example probe_cmd_params -- "$G" \
  CG CGACT CGINFO CSVINFO EMOTE ERROR FILEACT LOAD MATH SAVE TEXT TEXTINFO VARINFO SYSTEMMODE
```

555 全表以 E3 的 TSV 为准(121 槽;含每个 build 的 handler 地址)。若需现场复核某个槽位:

```bash
cargo run -q -p yu-engine --example probe_cmd_params -- "$G" <命令名>
```

### 1.3 证据等级标记

- **观测**:探针/反汇编直接读到。
- **推导**:两个独立证据源一致(如 E1/E2 与 E4)。
- **未取证**:明确写出,禁止在实现里猜。

---

## 2. 命令集与 opcode 位移

### 2.1 增减

| 变化 | 0.495 | 555 | 证据 |
|---|---|---|---|
| 替换 | `CDDA`(idx 0) | `ALIAS`(idx 0) | E1/E2/E3 |
| 删除 | `EMOTEINFO`(idx 13) | — | E1/E2/E3 |
| 新增 | — | `MOVIE2` / `MOVIE2END` / `MOVIE2INFO`(idx 73–75) | E2/E3 |
| 新增 | — | `PRODUCT`(idx 116) | E2/E3 |
| 补洞 | `);`(idx 114,旧表残留名) | `_IFNDEF`(idx 114) | E1/E2 |
| 净变化 | 119 条 | 121 条 | E1/E2 |

注意:`PRODUCT` 不是 555 首次出现。0.481/0.494 商业表已有 `PRODUCT`;0.495 SDK 表没有;555 重新纳入。证据:E4 §6.3 注 + 历史样本表普查。

### 2.2 opcode 位移(禁止硬编码)

| 命令 | 0.495 | 555 |
|---|---:|---:|
| `GO` | 43 | 42 |
| `GOSUB` | 44 | 43 |
| `TEXT` | 96 | 98 |
| `RETURNCODE` | 79 | 81 |
| `WORD` | 106 | 108 |
| `PRODUCT` | — | 116 |

证据:E1/E2;E4 §6.2。内核现状已按命令名解析参数与目标,但此表用于审计任何残留的数字 opcode/槽号常量。

---

## 3. 同名但参数表不同(14 条)

argc 与槽位序列均以 E1/E2 为准;E4 §6.3 独立复核一致。

| 命令 | 0.495→555 argc | 555 变化(槽位) |
|---|---:|---|
| `CG` | 45→58 | 7–8 由 `SX,SY` 改为 `SD,MD`;9–10 重新放 `SX,SY`;11–21 新增 `SLX,SLY,SCX,SCY,RZ,RLX,RLY,RCX,RCY,SIP,RIP`;`TSX` 起整体后移 13 槽 |
| `CGACT` | 70→71 | `RECTPAINT3` 之后插入 `GRADATION`(555 idx 61);`WHIRLPOOL` 起后移 1 槽 |
| `CGINFO` | 33→36 | 删 `FX,FY`(495 idx 15–16);`FBX` 起前移 2;`LINT2` 之后新增 `SEARCH,NUM,NAME,LSET,NO`(555 idx 28–32);`LET,SET,SET2` 后移 |
| `CSVINFO` | 4→3 | 删 `DATA`(495 idx 2);`LET` 由 3→2 |
| `EMOTE` | 24→18 | 删 `SMOOTH,MESH,QUE,LABEL,STOP,RATIO,DATA`;增 `LIST`;`IDNO` flag 0→1;`PLAY` kind 4→3 |
| `ERROR` | 107→108 | 在 idx 2 插入 `COM_ALIAS`;其后所有 `COM_*` 数值整体 +1 |
| `FILEACT` | 17→18 | 在 idx 15 插入 `WEBP`;`CGCASH/SOUNDCASH` 后移 1 槽 |
| `LOAD` | 17→18 | 在 idx 1 插入 `MEM`;`DNO` 起整体后移 1 槽;`DNO` kind 16→2 |
| `MATH` | 22→23 | 末尾追加 `CALC`(555 idx 22);旧槽不动 |
| `SAVE` | 17→18 | 在 idx 1 插入 `MEM`;其后整体后移 1 槽 |
| `TEXT` | 20→3 | 只保留 `CLEAR`;新增 `FILE,LET`;旧 20 槽的 `ID/OUTPUTMODE/8×按钮/WAIT/RETURN/PAGE/CLICKRETURN/CLICKPAGE/CLICK/AUTO*` 全删 |
| `TEXTINFO` | 10→0 | 10 个参数全删,命令保留为空签名 |
| `VARINFO` | 21→22 | idx 15 插入 `STRFIRST`;`SJISCODE` 起后移 1;删 `UTF8CODE`;末尾追加 `NO2` |
| `SYSTEMMODE` | 36→33 | idx 9 插入 `WEBP`;删除 495 的 4 个无名占位槽(13–16);命名参数净增 1 |

### 3.1 槽位移风险

以下三条最容易被按槽号读取的代码写错:

1. `CG`:槽 7/8 在 495 是 `SX/SY`,555 是 `SD/MD`;`SX/SY` 移到 9/10。
2. `ERROR`:`COM_ALIAS` 插入 idx 2,所有 `COM_*` 常量值后移一位。
3. `LOAD/SAVE`:`MEM` 插入 idx 1,后续槽全部后移。

处理要求:参数一律按 `cmd.param_names` 的名字解析;禁止 `if slot == N` 之类的跨版本常量。`VARINFO/VARACT` 的历史静默失效正是这一族。

---

## 4. 同名同参数但语义不同(核心同名异构)

| 命令 | 0.495 语义 | 555 语义 | 证据 |
|---|---|---|---|
| `TEXT` | 文本属性设置;`ID/OUTPUTMODE/按钮/WAIT/RETURN/PAGE/CLICK*/AUTO*` | 读场景文本文件到 `LET` 字符串数组;`FILE/LET/CLEAR`;行尾加 `0x0D`,末尾两个空元素 | 0.495 handler `0x453448`;555 `0x452888`(b3c02)/`0x452a0c`(b02a);参数表见 §3;语料 `es_scenario_load.yst` s60 |
| `TEXTINFO` | 10 参数查询;缺 `LET` 报 `CMD_SLOT` | 0 参数空处理体;`0x452d54` 只装载参数后 `return 0`,任何调用 no-op | 0.495 `0x4538cc`;555 `0x452bd0`/`0x452d54`;E3 argc 0 |
| `GOSUB` / `RETURN` | 单组 `args`/`ret` 数组 | 三组 `args_int/flt/str`、`ret_int/flt/str`;整数引用参数可传槽号(`RefIntParamMode::SlotId`) | 参数名相同;任务书 §3.3;`other/gosub.rs`;555 `0x4428cc/0x442a40`、`0x44b418/0x44b59c` |
| `VARINFO[SJISCODE]` | 处理体不读该槽(惰性参数) | 读 `SET` 字符串首字符;单字节=字节值,双字节=`(lead<<8)｜trail` | 0.495 `0x456050..0x456ae4` 对槽零引用;555 `0x455200` 的 `0x4554a9`、`0x455b9c` |
| `VARINFO[STRFIRST]` | 无 | 在 `SET` 字符串数组 `[NO,NO2]` 闭区间内找字典序最小元素,写 `LET` | 555 `0x455200`;语料前后写 `$a[1]`/`$a[2]` 自证 NO2 为闭区间终点 |
| `FILEINFO[CGFILE]` | 槽存在;**已取证:真读该槽** | 按 `CG[FILE]` 的图像扩展名序在 VFS 内探测存在性(BT 按钮 `_off/_over/_on/_onover/_na`) | 两代表都有 `CGFILE`(E1/E2);0.495 `0x44286c`(kind 2 → `ebx=3`,`0x44298a` → `0x442c7f` 扩展名探测);555 `0x43eb34`/`b02a 0x43eca8`;语料 `es_buttonque.yst:341` |
| `CGINFO[LINT2]` | 槽存在;类型码 3;旧语义 | 直接回读图层 `+0x60` 按钮号(`CG` 处理体写 `+0x58/+0x60`);类型码 1 | 555 `CGINFO 0x43b090`、`CG 0x423870`;`+0x58/+0x60` 证据见 truth §11.3;E4 §6.3 |
| `SAVE/LOAD[MEM]` | 无 `MEM` | `MEM=n` 是内存存档槽:写/读 `DAT_008670c0[n]`、容量 `DAT_008678a0[n]`;`MEM=99999` 走 `FUN_0044d82c` 0..499 打包分支 | 555 `SAVE 0x44b6f0`/`LOAD 0x444654`;E4 §16.10;当前实现见 §5 |
| `CG` | 槽 7/8 = `SX/SY` | 槽 7/8 = `SD/MD`;`SX/SY` 在 9/10 | E1/E2;E3 handler `0x423870` |
| `ERROR` | `COM_CG` 从 idx 2 开始 | `COM_ALIAS` 占 idx 2,所有 `COM_*` 值后移一位 | E1/E2;E3 handler `0x469958` |
| `EMOTE` / `EMOTEINFO` | E-mote ABI 24 参数 | 18 参数重写;`EMOTEINFO` 命令删除 | E3 `EMOTE 0x43d40c`;E4 §6.3;E-mote 仍 `deferred(master)` |

### 4.1 重要勘误:`FILEINFO[CGFILE]`

旧审计理由写作「旧 ysc 的 FILEINFO 无 `CGFILE`」,本次现场探针证明**0.495 表里已经有 `CGFILE` 槽**(`FOLDER,FILE,CGFILE,SOUNDFILE,EXIST,SIZE,RELATION,LET`)。

因此:

- 不能再用「参数表里有没有 `CGFILE`」来判断 555;
- **2026-11-23 已取证**:0.495 `0x44286c` 对 `CGFILE`(kind 2)置 `ebx=3`,EXIST 分支 `0x44298a` 跳 `0x442c7f`,先试原路径再按 `.png/.bmp/.jpg/.gif/.psb` 拼扩展名探测 —— 两代都读该槽;
- 当前内核 `crates/yu-rt/src/vm/handlers/file_io.rs:371` 的 `yscm.version == 555` 门控**没有语义依据**,属于待删除的特化;
- v555 的扩展名表在 `.psb` 之后还有 `.webp`,当前 `cg_file_exists` 缺这一候选;详见 `docs/formats/v555-semantic-census-2026-11-23.md` §2.3。

---

## 5. 当前内核实现状态

| 项 | 当前位置 | 状态 |
|---|---|---|
| `TEXT[FILE]` / `TEXTINFO` 空签名 | `crates/yu-rt/src/other/handlers/text.rs` | 已迁入独立运行面;`SemanticProfile::text/textinfo` 门控;共享 `text.rs` 恢复 c1549eb |
| `GOSUB/RETURN` 三组帧 | `crates/yu-rt/src/other/gosub.rs` | 已实现;含任务基帧修复与 fixture |
| `VARINFO[SJISCODE/STRFIRST/NO2]` | `crates/yu-rt/src/semantics.rs` + `vm/handlers/variables.rs` | 已实现;按语义形态门控 |
| `CGINFO[LINT2]` | `crates/yu-gfx/src/state.rs` | LINT2 回读已实现;`SEARCH/NUM/NAME/LSET/NO` 未实现(truth 记 31,405 条 unsupported 待分类) |
| `FILEINFO[CGFILE]` | 共享 `vm/handlers/file_io.rs:371` | 已实现探测;已删除 `yscm.version == 555` 门控,扩展名链末尾通用追加 `.webp`(DIV-RT-070) |
| `SAVE/LOAD[MEM]` | 共享 `vm/handlers/file_io.rs:719/984` + `yu-save` | 已实现内存槽;`MEM=99999` 的 0..499 打包已实现(DIV-RT-071);打包缓冲官方无直接消费者,宿主保留字节 |
| `PRODUCT` | `crates/yu-rt/src/other/handlers/product.rs` | 已实现部分 COM;`COM=21000/10201/10205` 仍 `Unimplemented` |
| `MOVIE2/MOVIE2END/MOVIE2INFO` | — | 未路由/未实现;静态语料 0 次使用(E4 普查) |
| `ALIAS` | 555 共享空操作槽 | 已按 555 默认槽清单处理 |
| `CDDA` / `EMOTEINFO` | — | 555 表不存在;旧 handler 不得假设可调用 |
| `EMOTE` | — | `deferred(master)`;不实现 |
| `CG`/`ERROR` 位移 | handler 按名解析 | 需持续审计;禁止槽号常量 |

---

## 6. 后续模型复查清单

按顺序执行;每项都给出可重复命令/地址,复查后把结果追加到本文件末尾的「复查记录」。

### 6.1 表结构复查

- [ ] 重新生成 0.495 参数表,与 §3 的 14 条逐槽对比。
  - 命令:`YURIS_SAMPLE_ERIS_ROOT=... cargo run -q -p yu-rt --example dump_yscm`
  - 预期:14 条命令的 argc/槽序列与 §3 一致;`CDDA/EMOTEINFO` 在 495 存在,555 不存在。
- [ ] 重新跑 555 参数表:
  - 命令:`probe_cmd_params "<games-555>/<v555-2>" <命令名>`
  - 预期:`TEXT`=3、`TEXTINFO`=0、`LOAD/SAVE`=18、`VARINFO`=22、`CGINFO`=36。
- [ ] 核对 E3 TSV 的 handler 地址与 §4 表一致;若 TSV 更新,以新地址为准并更新本文件。

### 6.2 语义复查(逐条反汇编)

- [ ] `TEXT`:0.495 `0x453448` vs 555 `0x452888/0x452a0c`;确认 555 只做 `FILE/LET/CLEAR` 与整文件切行。
- [ ] `TEXTINFO`:555 `0x452bd0/0x452d54` 参数表为 0;确认空处理体 `return 0`。
- [ ] `GOSUB/RETURN`:555 `0x442a40/0x44b59c`;确认三组参数窗口与 `PINT/PFLT/PSTR`、`RINT/RFLT/RSTR` 的落位。
- [ ] `VARINFO`:555 `0x455200` 的 `0x4554a9`(SJISCODE 首字符)、`0x455b9c`(`SHL 8 + trail`)、`STRFIRST`;0.495 `0x456050..0x456ae4` 对 `SJISCODE` 零引用。
- [x] `FILEINFO[CGFILE]`:反汇编 0.495 `0x44286c`,确认它**读** `CGFILE` 槽(kind 2 → `ebx=3` → `0x442c7f` 扩展名探测);v555 同槽走 `FUN_0043f770`,扩展名表含 `.webp`。2026-11-23 闭合,见 `v555-semantic-census-2026-11-23.md` §2.3。
- [ ] `CGINFO[LINT2]`:555 `CG` `0x423870` 写 `+0x58/+0x60`;`CGINFO 0x43b090` 回读;确认 0.495 的 `FX/FY` 删除与类型码 3→1。
- [x] `SAVE/LOAD[MEM]`:555 `0x44b6f0/0x444654`;确认 `DAT_008670c0[n]`/`DAT_008678a0[n]` 与 `MEM=99999` 的 `FUN_0044d82c`。`MEM=99999` 已按 `0x44b874` → `FUN_0044d82c` 实现(DIV-RT-071);`LOAD` 无 99999 分支。
- [ ] `CG` 槽 7/8/9/10:确认处理器按参数名取值,不存在槽号常量。
- [ ] `ERROR` `COM_*`:确认所有 COM 常量由 ysc 参数名/表值解析,不存在硬编码数值。
- [ ] `MOVIE2*`:E3 地址 `0x44a6d8/0x44ad64/0x44ad88`;当前 0 静态使用,未实现可接受,但需记录为 `Unimplemented` 而非静默 no-op。
- [ ] `EMOTE`:复查仍为 `deferred(master)`;不得因表结构变化启用。

### 6.3 回归门槛

- [ ] `cargo test -p yu-rt --lib`
- [ ] `cargo test -p yu-rt --test fixture`
- [ ] `cargo test -p yu-engine --lib patch`
- [ ] `cargo test -p yu-gfx --test info_query`
- [ ] `cargo test -p yu-save`(需 `YURIS_SAMPLE_ERIS_ROOT`/`YURIS_SAMPLE_SDK0495`)
- [ ] 555 启动链 60 帧 + 1500 帧探针:无 Fatal/unimplemented;确认 `\CSEL` 之后的推进点。

### 6.4 文档一致性

- [ ] 若修改本文件任何结论,同步更新 `docs/formats/v555-truth-2026-11-21.md` 对应小节。
- [ ] 若更新 E3 TSV,必须同时更新 §2/§3/§4 的地址与 argc。
- [ ] 新发现的同名异构项按同样格式追加,必须带 0.495/555 双地址或双证据。

---

## 7. 勘误与已知冲突

1. **`FILEINFO[CGFILE]`**:旧审计「0.495 表无 CGFILE」错误;E1 实测两代都有该槽。见 §4.1。
2. **`v555-commercial.md` §7.3 的 `--patch gbk`**:已被 `v555-truth-2026-11-21.md` §11.7 取代。`<v555-2>` 的 ybn UI/场景真值是 Big5;语言由游戏内 `ES.LANG.SET` 选择,不能用 exe 静态 lead 表或 GBK 自动匹配替代。命令差异文档不依赖该编码结论,但复查运行时不要沿用旧命令。
3. **`v555-commercial.md` §7.3 的历史首错点**(`es_scenario_load.yst pc6` 槽引用 Fatal):属于旧实现阶段;当前启动链已能到 `s60 pc1/2/3 TEXT` 与 `es_select2.yst pc130`。复查时以最新 trace 为准。
4. **`PRODUCT` 的「新增」**:相对 0.495 SDK 表是新增;相对 0.481/0.494 商业表是重新纳入,不要写成 555 首创。
5. **`SYSTEMMODE` 的 argc 口径**:495 表含 4 个无名占位槽,555 删除;按「命名参数」比较是 32→33,按 argc 是 36→33。复查时必须同时看 argc 与命名序列。

---

## 8. 复查记录

> 后续模型每次复查追加一条:日期 / 证据 ID / 命令 / 结果 / 是否修改结论。

- 2026-11-21:首次建立。E1/E2/E3/E4 一致确认 14 条参数表变化;E5 确认 8 组同名异构;发现 `FILEINFO[CGFILE]` 旧审计理由错误,记入 §4.1/§7。
- 2026-11-23:语义普查(见 `docs/formats/v555-semantic-census-2026-11-23.md`)。确认 §4 的 8 组不完整(缺 `RETURNCODE`,已由 DIV-RT-069 补);闭合 `FILEINFO[CGFILE]` 证据空洞(0.495 `0x44286c` 读该槽);新增 `TEXT[CLEAR]` 时机、`WORD`/`_` 逐字符表删除、`LET` 系统变量写入、`FONT NAME`、`WAIT` 边界等结论。0.495 与 v555 处理体归一化反汇编相似度:15 条命令 1.000 一致,差异集中在 text/查询/文件/任务边界。
- 2026-11-23(续):`_`/`WORD` 折空格/上限与 v555 `TEXT[CLEAR]` 时机已修(DIV-RT-072/073);`FONT` 的 NAME/NO/BNO 复核为 `yu-gfx` 已实现(8 样式槽 + `yu-font` 请求),VM 数值镜像另议。
