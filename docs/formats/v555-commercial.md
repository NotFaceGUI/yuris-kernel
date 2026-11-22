# v555 商业分发版:普查、差异与运行基线(2026-11-19)

> 范围 = 本机 4 个引擎版本资源为 **0.555.0.0** 的商业发行版,外加 0.495 官方 ERIS 语料作对照。
> 本文只记录**已实测的事实**;每条给出复现命令或字节锚点。推断项显式标 `Inferred`,未知项标 `Unknown`。
>
> 样本占位符(真名/路径对照只留在 `AGENTS.md`,不入库):
>
> | 占位符 | 引擎 | YPF | 说明 |
> |---|---|---|---|
> | `<v555-1>` | 0.555 | 500 | 2016 日文作;有散盘 `ysbin/` 汉化叠层 |
> | `<v555-2>` | 0.555 | 500 | 2021-11 多语作;脚本在 `pac/bn.ypf` |
> | `<v555-3>` | 0.555 | 500 | 2023-05 多语作;脚本在 `pac/bn.ypf` |
> | `<v555-4>` | 0.555 | 500 | 2022-12 多语作;脚本在 `pac/bn.ypf` |
> | `<v479-x>` / `<v481-x>` / `<v488-x>` / `<v494-x>` | 0.479 / 0.481 / 0.488 / 0.494 | 479/481/487/494 | 密钥规律的跨版本对照 |
>
> 本机取证产物(不入库):`<side-repo>/workbench/tmp/probe555/`
> (`survey.json`、`<v555-1>_ysbin/`、`<v555-2>_bn/`、`639a8c2c/`、`937657dd/` 等)。

---

## 0. 结论速览

| 项 | 结论 | 证据 / 状态 |
|---|---|---|
| 引擎版本 | 4 样本全部 `ProductVersion = 0, 555, 0, 0` | PE 版本资源;`survey.json` |
| 包版本 | **v555 引擎发行版使用 YPF v500 包**,不是 555 | 每个 `.ypf` 头 `+0x04 = 500` |
| YPF 方言 | `hashed-len`;`yu-vfs` 已可读,条目数/解压闭合 | `version_probe` / `extract_ypf` |
| 七表版本 | YSCM/YSLB/YSVR/YSTD/YSTL/YSCF/YSER **全部申报 555**,现有解析器全部通过 | `version_probe` |
| 脚本容器 | YSTB v555 = V5 四区段布局;4 套语料 **1,554 文件 / 508,641 条指令**结构闭合 | 独立结构走查 |
| 脚本密钥 | **key = CRC32(exe 内“作品 ID”串)大端**;4/4 v555 命中,另 4 个 0.479–0.494 样本交叉命中 | 见 §5;自动探测 `rx-yuris:header` |
| 系统变量 ABI | **实测 1000 槽**:0..547 = 0.495 的 548 个具名项;548..999 = 0.495 就已存在的 452 个 `ty=0` 保留槽;作品全局从 1000 起。0.495 SDK 与 6 个 555 样本在 0..999 上逐字段 0 差异 | §16.2;`v555-sysvar-abi-measured.tsv` |
| 命令表跨样本 | 6 个 555 样本的 `ysc.ybn` **字节完全相同**(SHA-256 `7ee03ebb…`),121 命令 / 1113 参数;尾部消息/键表与 0.495 相同 | §16.1/§16.3 |
| 存档容器 | 0x1e0(480),与 0.495 官方一致 | `yu-engine::save` 启动探测 |
| 运行 | 4 样本均可 boot 到首帧;`<v555-1>` 240 帧 **0 个 VM 诊断** | `yu-cli run` |
| 多语发行版 | `<v555-2/3/4>` 必须 `--patch gbk` 启动(CP932 会在 `_tw.txt` 处 fatal) | 用户口径 + 实测 |
| 首错点 | `<v555-2/3/4>` 在 `es_scenario_load.yst pc6` 的 **`TEXT[FILE=…, LET=…]`** 处 `rt.expr.malformed` | 见 §7 |
| TEXT 555 语义 | **已 RE**:读文本文件 → 按行写入 LET 字符串数组(行尾 CR;末尾空元素);FILE 不补扩展名 | 处理体 `0x452a0c`;YSVR `$B34..$B39` = `ty=3, dims=[70000]` |
| 未实现命令 | `PRODUCT` 全 COM 分支已 RE(拖放文件/文件对话框/模式位/渲染路径/调试口);启动期 `COM=0x1D` 初值 0 | `0x459670`;分支表见 §7.3 |
| 独立剧本系统 | **555 剧情正文是独立文本文件**(`scenario/*.txt`、`sc_cn_scenario/*.txt`、`sc_tw_scenario/*.txt`),由包内 `es_scenario_lexer/load/parser/text` + 宏脚本解释;`TEXT[FILE]` 读入行数组 | §13;`s56–s76` |
| 顶部 UI 条 | 当前内核启动过渡期显示的 `LOCK/DEFAULT/Q.SAVE…` 是 `メイン画面設計` 的 GAMEMAIN 按钮条,**不是** `:/W1/DBG`;官方同版本窗口化运行无 | §14;frame 180 display JSON + 官方截图 |
| 本轮最大差异 | **TEXT 签名从 495 的 `ID/OUTPUTMODE/…` 变为 555 的 `FILE/LET/CLEAR`**;文本资源外置到 `scenario/*.txt`/`sc_cn_`/`sc_tw_` | YSCM 参数表 + 语料 + §13 |

---

## 1. 样本与识别

### 1.1 引擎版本资源

| 样本 | 主 exe 大小 | PE 时间戳 | ProductVersion | FileDescription |
|---|---:|---|---|---|
| `<v555-1>` | 3,522,560 | 2016-10-13 | 0, 555, 0, 0 | `""` |
| `<v555-2>` | 6,496,256 | 2021-11-27 | 0, 555, 0, 0 | `<patch-source>`(第三方重打包痕迹) |
| `<v555-3>` | 2,115,072 | 2023-05-03 | 0, 555, 0, 0 | `<patch-source>` |
| `<v555-4>` | 2,115,072 | 2022-12-28 | 0, 555, 0, 0 | `<patch-source>` |

- 四者均为 32 位 PE(`Machine=0x14c`)、Subsystem=2、Linker 6.0。
- `<v555-2>` 的 exe 含额外节(`.ierdata/.xfudata/.imrdata/.ZDFX`),`.rsrc` 也更大;`.text` 与 `<v555-3/4>` 同量级(≈1.53MB)。
- `<v555-1>` 的 `.text` 1,534,976 B;0.495 官方引擎 1,616,384 B。
- **告警**:三份样本的版本资源被第三方改写(`<patch-source>`),不能当作原始发行版哈希;代码/数据行为仍需以官方引擎 trace 为准(真值层级第 1 层)。

### 1.2 文件结构

| 样本 | 脚本包 | 其它包 | 散盘叠层 |
|---|---|---|---|
| `<v555-1>` | `pac/ysbin.ypf`(620 条, v500) | `bgm/cg1/cg2/cgsys/fc/se/st/vo1/vo2` + `update1/2` | 根 `ysbin/`(只有 yscfg + yst,汉化补丁;loose-first 覆盖包) |
| `<v555-2>` | `pac/bn.ypf`(320 条, v500) | `bgm/cg/cgsys/sc/se/vo` + `update1` | 无 |
| `<v555-3>` | `pac/bn.ypf`(321 条, v500) | 同上 | 无 |
| `<v555-4>` | `pac/bn.ypf`(321 条, v500) | 同上 | 无 |

- `pac/sc.ypf` 存 **场景文本资源**(`scenario/*.txt`,198 条),不是脚本二进制。
- `pac/*.ypf` 之外还有伪装成 `.ypf` 的 MPEG-PS 影片;`Project` 扫描时按魔数跳过并计数(既有行为)。
- 工程入口仍遵守官方规则:根/`pac/*.ypf` 扫描,`update1..9` 作为增量层;`data.ypf` 只是“根目录单包”特例。

---

## 2. YPF:555 引擎仍读 v500 包

| 包 | `+0x04` 版本 | 条目 | 索引字节 | 方言 |
|---|---:|---:|---:|---|
| `<v555-1>/pac/ysbin.ypf` | 500 | 620 | 27,904 | hashed-len |
| `<v555-2>/pac/bn.ypf` | 500 | 320 | 14,404 | hashed-len |
| `<v555-3>/pac/bn.ypf` | 500 | 321 | 14,449 | hashed-len |
| `<v555-4>/pac/bn.ypf` | 500 | 321 | 14,449 | hashed-len |

复现:

```bash
yu-cli 或 <repo>/target/debug/examples/version_probe <pack.ypf>
# 输出: ypf 包版本 500, 方言 hashed-len, N 条目
```

结论:

- **不存在“v555 包版本”**:555 是引擎/脚本格式版本,发行包仍写 500。
- 现有 `yu-vfs` 的 v500 方言(交换长度表 + Murmur/名字 XOR 0x36 族)足以读取这 4 个包;本轮未发现需要新增的 YPF 分支。
- `update1/2` 挂载顺序在 `<v555-1>` 上实测可用(10 基础包 + 2 增量层)。

---

## 3. 七张表:版本 555,布局未变

### 3.1 版本/计数

| 表 | `<v555-1>` | `<v555-2>` | `<v555-3>` | `<v555-4>` | 0.495 ERIS |
|---|---:|---:|---:|---:|---:|
| YSCM 命令 | 121 | 121 | 121 | 121 | 119 |
| YSLB 标签 | 4,507 | 4,204 | 4,232 | 4,199 | 1,941 |
| YSVR 变量记录 | 3,094 | 3,373 | 3,375 | 3,375 | 2,568 |
| YSTD word08 | 6,341 | 7,659 | 7,665 | 7,656 | 4,600 |
| YSTD word0C | 58,584 | 0 | 0 | 0 | 396 |
| YSTL 清单 | 619 | 320 | 321 | 321 | 141 |
| YSER 消息 | 123 | 123 | 123 | 123 | 119 |
| YSCF 字节 | 119 | 94 | 104 | 99 | 90 |

- 四份 `ysc.ybn` **字节完全相同**(sha256 `7ee03ebb…`),说明这 4 个发行版共用同一张 555 命令表。
- 四份 `yse.ybn` 也相同(sha256 `391a353c…`)。
- `yscfg.ybn` 布局与 0.495 的 `New` 族一致:固定区 0x4C,`+0x4C u16 标题长度`,`+0x4E` CP932 标题;文件长度差只是标题长度。
- `ysl.ybn` 仍是 `+0x0C` 起 0x400 字节索引块 + `0x0D+name` 记录;`dump_script` 已在 `<v555-1>` 上解析 `SYSTEM_START` → `s176 pc0`。
- `yst_list.ybn` 仍是 `New20`(20 字节尾);四套语料全部闭合。
- `ysv.ybn` 仍是 `WithFlag`(8 字节头);`abi_match` 全量解析闭合。

### 3.2 YSTD 两个 u32 的语义(本轮闭合)

`yst.ybn` 只有 16 字节:`YSTD + version + u32 + u32`。用 5 份语料做交叉验证:

| 语料 | YSTD.word08 | Σ YSTL.var_count | YSVR kind==1 条数 | 和 | YSTD.word0C | Σ YSTL.text_count |
|---|---:|---:|---:|---:|---:|---:|
| `<v555-1>` | 6,341 | 5,001 | 1,340 | **6,341** | 58,584 | **58,584** |
| `<v555-2>` | 7,659 | 6,430 | 1,229 | **7,659** | 0 | **0** |
| `<v555-3>` | 7,665 | 6,437 | 1,228 | **7,665** | 0 | **0** |
| `<v555-4>` | 7,656 | 6,427 | 1,229 | **7,656** | 0 | **0** |
| 0.495 ERIS | 4,600 | 3,433 | 1,167 | **4,600** | 396 | **396** |

结论(Confirmed,数据自证):

- `YSTD +0x08` = **总变量声明数** = Σ(YSTL 每脚本 `var_count`) + YSVR 里 `kind==1` 的条数。
- `YSTD +0x0C` = **总内嵌文本数** = Σ(YSTL 每脚本 `text_count`)。
- 另两条现成不变量也成立:Σ(YSTL.label_count) = YSLB 条数;YSTL 条数 = 清单脚本数。

> 意义:三份样本的 `text_count` 总和为 **0**,说明 555 的新发行模式把剧情文本**外置**到了 `scenario/*.txt`;这正是 `TEXT[FILE]` 存在的直接证据。

---

## 4. YSTB v555:V5 四区段,1554 文件全闭合

- 头版本 `+0x04 = 555`;`count` 为迭代上界;`s1 == count×4`、`s4 == count×4`、`s2 % 12 == 0`。
- 四区段各自从密钥下标 0 开始异或(与 0.495 相同)。
- 独立走查结果:

| 语料 | 脚本文件 | 指令总数 | 结构失败文件 |
|---|---:|---:|---:|
| `<v555-1>` | 613 | 272,716 | 0 |
| `<v555-2>` | 313 | 78,025 | 0 |
| `<v555-3>` | 314 | 79,796 | 0 |
| `<v555-4>` | 314 | 78,104 | 0 |
| **合计** | **1,554** | **508,641** | **0** |

- 载荷标签除 495 的 `M/H/V/L/v` 外,还观测到 `B/I/W/F`;是否 555 新增还是 495 语料未覆盖,需继续取样(Unknown)。本轮 4 套语料的运行未因这些标签报错。
- 结论:**v555 没有改动 YSTB 容器布局**,只改了版本号、命令表与文本机制。

---

## 5. 密钥:CRC32(作品 ID),不是协议常量

### 5.1 规律

引擎在 exe 的同一张常量表里存放 `"pac\0" + <ID 串>`:

- 0.495 官方:`"pac\0Yu-ris\0"` → `CRC32("Yu-ris") = D36FAC96`;
- 商业构建把 ID 槽替换成作品 ID → 密钥随之改变;
- 引擎异或时按 **CRC 值的大端字节** 取 4 字节(与 `docs/formats/ypf.md` / `key.rs` 既有结论一致)。

### 5.2 跨版本验证(9/9)

| 样本 | 引擎 | exe 内 ID 槽 | CRC32(BE) = 实测密钥 |
|---|---|---|---|
| `<v479-x>` | 0.479 | `Yu-ris` | `D36FAC96` |
| `<v481-x>` | 0.481 | `<pid-481>` | `6424E8DD` |
| `<v488-x>` | 0.488 | `<pid-488>` | `605E414A` |
| `<v494-x>` | 0.494 | `<pid-494>` | `605E414A` |
| `<v555-1>` | 0.555 | `<pid-555-1>` | `9C28430C` |
| `<v555-2>` | 0.555 | `<pid-555-2>` | `7FF08DAF` |
| `<v555-3>` | 0.555 | `<pid-555-3>` | `F469D13E` |
| `<v555-4>` | 0.555 | `<pid-555-4>` | `4CD5B65B` |
| `<v555-pc>` | 0.555 | `<v555-pc>` | `6732A887` |

> `pid-*` 实际字符串与文件路径对照见 `AGENTS.md`;本机取证记录在
> `<side-repo>/workbench/tmp/probe555/survey.json` 与临时脚本输出。
> `<v555-pc>` 是 2026-11-19 新增的第 6 个 555 样本,CRC32(`<v555-pc>`)
> = `6732A887`;用该钥可解析其 YSTB 脚本(探针跨 46 个脚本组扫描通过;
> `yst_list` 中纯声明条目没有 `.ybn`,探针需按 `has_script` 跳过)。

验证方法:

1. 在 exe 中定位 `pac\0`,读紧随其后的 NUL 结尾 ASCII 串;
2. `CRC32(该串)` 取大端 4 字节;
3. 与 YSTB 首参数窗口的 `offset` 字段(明文 0 ⇒ 密文即密钥)比对;
4. 用 `Σargc×12 == s2` 与 `opcode < 命令数` 做结构自检。

5 个 v555 样本(含 <v555-pc>)全部通过;另 4 个 0.479–0.494 样本同样命中,说明这条规律**不是 555 特有**。

### 5.3 现有实现的取钥路线

`yu-engine/src/keys.rs` 的顺序:

1. `--script-key` 显式;
2. V5 首窗口自举(RxYuris 路线)→ 本轮 4 样本全部命中,日志 `source=rx-yuris:header`;
3. 参数窗口投票(Script-Editor 路线);
4. 默认 `D36FAC96`;
5. exe 可打印串 CRC32 扫描(本仓库补充)。

因此 **无需新增代码就能取到 4 个 555 样本的钥**;但建议后续把“exe 内 `pac` 邻接 ID”做成带名字的诊断来源(便于审计为何选此钥)。

---

## 6. 命令表 495 → 555

### 6.1 命令集增减

| 变化 | 495 | 555 |
|---|---|---|
| 替换 | `CDDA`(idx 0) | `ALIAS`(idx 0) |
| 删除 | `EMOTEINFO`(idx 13) | — |
| 新增 | — | `MOVIE2` / `MOVIE2END` / `MOVIE2INFO`(idx 73–75) |
| 替换 | `);`(idx 114,疑似旧表残留名) | `PRODUCT`(idx 116) |

净增 2 条:119 → 121。

### 6.2 opcode 位移(必须按名解析,禁止硬编码)

| 命令 | 495 | 555 |
|---|---:|---:|
| `GO` | 43 | 42 |
| `GOSUB` | 44 | 43 |
| `TEXT` | 96 | 98 |
| `RETURNCODE` | 79 | 81 |
| `WORD` | 106 | 108 |
| `PRODUCT` | — | 116 |

内核现有的“按 ysc 名字解析参数/输出目标”策略在 555 上仍然成立;本轮 4 套语料运行未出现 opcode 误派。

### 6.3 参数表变化的 14 条命令(精确序列对齐,`ysc_diff2.py`)

`CG` `CGACT` `CGINFO` `CSVINFO` `EMOTE` `ERROR` `FILEACT` `LOAD` `MATH` `SAVE` `TEXT` `TEXTINFO` `VARINFO` `SYSTEMMODE`

| 命令 | 495 argc | 555 argc | 555 实际变化 |
|---|---:|---:|---|
| `CG` | 45 | 58 | 插入 `SD, MD`(索引 7–8)与 `SLX, SLY, SCX, SCY, RZ, RLX, RLY, RCX, RCY, SIP, RIP`(索引 11–21);旋转/仿射/裁剪/采样参数组 |
| `CGACT` | 70 | 71 | 插入 `GRADATION`(索引 61) |
| `CGINFO` | 33 | 36 | 删除 `FX, FY`;新增 `SEARCH, NUM, NAME, LSET, NO`;`LINT/LINT2` 类型码 3→1 |
| `CSVINFO` | 4 | 3 | 删除 `DATA` |
| `EMOTE` | 24 | 18 | 删 `SMOOTH, MESH, QUE, LABEL, STOP, RATIO, DATA`;增 `LIST`;`IDNO` flag 0→1;`PLAY` kind 4→3 |
| `ERROR` | 107 | 108 | 插入 `COM_ALIAS`(索引 2);`COM_PRODUCT` 在 0.495 已存在 |
| `FILEACT` | 17 | 18 | 新增 `WEBP`(索引 15) |
| `LOAD` | 17 | 18 | 新增 `MEM`(索引 1);`DNO` kind 16→2 |
| `MATH` | 22 | 23 | 新增 `CALC` |
| `SAVE` | 17 | 18 | 新增 `MEM`(索引 1) |
| `TEXT` | 20 | 3 | 保留 `CLEAR`;删 `ID/OUTPUTMODE/8×按钮/WAIT/RETURN/PAGE/CLICKRETURN/CLICKPAGE/CLICK/AUTO*`;新增 `FILE, LET` |
| `TEXTINFO` | 10 | 0 | 全部参数删除(命令保留为空签名) |
| `VARINFO` | 21 | 22 | 新增 `STRFIRST`;删除 `UTF8CODE`;新增 `NO2` |
| `SYSTEMMODE` | 36 | 33 | 新增 `WEBP`(索引 9);删除 4 个无名占位参数(495 的 13–16) |

关键语义变化:

- `TEXT` 从“文本属性设置”变成“读取外置文本到 LET 字符串数组”;555 仍保留 `CLEAR`
  (0.495 也有),删掉的是文本窗口/按钮/自动换行一族。
- `TEXTINFO` 在 555 变成 **0 参数**;现有 5 个可读 555 样本均未执行该命令,
  所以“空签名”不会立即造成行为差异,但内核不能再假设它有 `LET`。
- `LOAD/SAVE[MEM]` 是 555 新面中使用最广的一个:5 个可读样本每作
  `LOAD[MEM]` 193–210 次、`SAVE[MEM]` 2 次;当前内核把 `MEM` 静默忽略。
- `CG` 的 13 个新参数在 5 个可读样本中 **0 次使用**;`CGINFO` 新增查询、
  `MATH[CALC]`、`FILEACT[WEBP]`、`EMOTE[LIST]`、`ALIAS`、`MOVIE2*` 同样 0 次。
  这些属于“表里有、语料未用”,不应抢在主线前实现。
- `EMOTE` 是 E-mote 家族;555 的 24→18 重写说明 E-mote ABI 也变了。
  本轮 5 个可读样本(非 E-ris)都不执行 `EMOTE`;<game-c> 取证包不含脚本,
  无法验证其参数用法。E-mote 仍按主人暂缓口径(`deferred(master)`)。
- `PRODUCT` 不是“555 新命令”:0.481 / 0.494 的商业命令表已有 `PRODUCT`,
  0.495 SDK 表没有;555 重新纳入。命令表按名分派,不能按 0.495 表硬编码。

> 静态使用普查(`probe_param555`,5 个可读样本):真正用到的 555 新面只有
> `TEXT[FILE/LET]`(仅样本)、`PRODUCT`、`LOAD/SAVE[MEM]`、
> `VARINFO[STRFIRST/NO2]`(<v555-1>/样本)、`CGACT[GRADATION]`(<v555-1>/样本)。
> <v555-pc> 只用到 `PRODUCT`、`LOAD/SAVE[MEM]`、`TEXT[CLEAR]`。

---

## 7. 运行时普查

### 7.1 启动与首帧

| 样本 | 钥来源 | 首帧 | 缺失资产 | 结论 |
|---|---|---|---:|---|
| `<v555-1>` | `9C 28 43 0C` / rx-yuris | 1280×720 | 0 | boot 成功 |
| `<v555-2>` | `7F F0 8D AF` / rx-yuris | 1280×720 | 0 | 需 `--patch gbk` |
| `<v555-3>` | `F4 69 D1 3E` / rx-yuris | 1280×720 | 0 | 需 `--patch gbk` |
| `<v555-4>` | `4C D5 B6 5B` / rx-yuris | 1280×720 | 0 | 需 `--patch gbk` |

- 0.495 ERIS 对照:钥 `D36FAC96`,存档容器 `0x1e0`。
- 4 个 555 样本的存档容器探测同样为 `0x1e0`(480),**存档格式未随引擎升到 555 而改变**。

### 7.2 `<v555-1>`:240 帧无 VM 诊断

```text
frames ticked: 240 (stop: frame limit, limit 240)
frame: 240 viewport: 1280x720
诊断:仅 gfx.info.missing ×56(ERIS 按钮脚本的 UI 目标缺失,非 VM 错误)
无 rt.* / fmt.* / table.* 诊断
```

### 7.3 三份样本:GBK + 首错点

- 不用 `--patch gbk` 时:
  `FATAL rt.expr.malformed @ data\script\userdefine\…_tw.txt pc7: 字符串字面量未闭合`
  —— CP932 把汉化文本的引号配对读坏。
- 加 `--patch gbk` 后,该 fatal 消失;新的首错点为:

```text
FATAL [vm] rt.expr.malformed @ data\script\eris\scenario\es_scenario_load.yst pc6:
  求值结果是被引用的变量槽($2871),不是值
```

三份样本只是变量槽号不同(`$2871` / `$2875`),位置与形态完全一致。

#### `TEXT[FILE, LET, CLEAR]`(Confirmed,555 处理体 RE)

语料形态(`es_scenario_load.yst`,三份样本同一结构):

```text
pc0  STR  $B1 = <scenario 名>
pc1  TEXT[FILE=$B1,          LET=$B4]
pc2  TEXT[FILE="sc_cn_"+$B1, LET=$B5]
pc3  TEXT[FILE="sc_tw_"+$B1, LET=$B6]
...
pc6  TEXT[FILE=$B2,          LET=$B7]
pc7  TEXT[FILE="sc_cn_"+$B2, LET=$B8]
pc8  TEXT[FILE="sc_tw_"+$B2, LET=$B9]
...
pc20 GOSUB es._strlen($B4, …)      ← 对 LET 数组求字符串长度
pc21 GOSUB es.SCENARIO.CRPREPLACE  ← 对 LET 数组做换行替换
```

**处理体位置**(`<v555-3>` 的 exe,sha256 `9bcd33cf…`):

> 语义取自 `<v555-3>` 的 555 构建;其余三个样本未逐作反编译,但它们共用同一张 `ysc.ybn`(sha256 `7ee03ebb…`),按同一逻辑实现后再用运行验收。

- 命令分发表基址 `0x78b1a0`(按 opcode 索引;装载器用 `mov dword [base+4*i], handler` 逐项填充);
- `TEXT` = opcode 98 → 处理体 **`0x452a0c`**;
- `TEXTINFO` = opcode 99 → `0x452d54`(29 字节;555 参数表为空 ⇒ 只做参数装载后返回 0);
- 对照 0.495:`TEXT` = opcode 96 → `0x453448`,`TEXTINFO` = 97 → `0x4538cc`,表基址 `0x7ace20`。

**语义**(处理体 + 辅助函数):

1. `FILE`(字符串):复制到栈缓冲 → `FUN_00464efc` 把 ASCII `A..Z` 降为小写 → `/` 全部换成 `\` → `FUN_0040bcdd` 按官方文件源顺序打开(**不自动补扩展名**,`FILE` 必须是完整资源名)。
2. 读入文件字节,追加 CRLF;按 CRLF / CR / LF 切行;双字节前导字节按 `DAT_0059b0a0` 表成对复制。
3. 每行末尾写入 `0x0D` 后,调用 `FUN_0045bb60` 把该行写入 **LET 指向的字符串数组** 的元素 `0..N-1`;循环结束后对空缓冲再调用 `FUN_0045bb60` 两次(末行之后的空行 + 终止空元素)。元素索引上限取自变量记录 `+0x04`(与数组长度对应);越界即停止。
4. `LET` 必须是字符串变量(type 3)。语料实证:`$B34..$B39` 在 YSVR 里是 `ty=3, dims=[70000]`,正好对应场景文本行数上限。
5. `CLEAR=1`:置 `DAT_008726f4+0x15 = 1` 并调用 `FUN_00469f9c` 清空系统文本缓冲(0x2000×2) —— 与 495 的 `TEXT[CLEAR]` 同义,**不清 LET 数组**。
6. 文件打开失败:处理体直接返回 -1(该分支未调用错误号);`LET` 不是字符串变量或索引越界:`FUN_0045bb60` 走官方错误号 `0x18b14`。两者都不终止脚本(调用方靠 `sc_cn_`/`sc_tw_` 回退)。

**辅助函数锚点**:

| 地址 | 作用 |
|---|---|
| `0x45bb60` | 把一行字符串写入 LET 字符串数组的元素(索引在 EDX) |
| `0x45bc60` | 把数值写入 LET/PINT/PFLT 目标(产品/调试输出用) |
| `0x40bcdd` | 官方文件源顺序打开文件 |
| `0x464efc` | ASCII 小写化 |
| `0x469f9c` | 清理系统文本缓冲(CLEAR) |
| `0x4254cd` | 当前指令参数装载(填 `DAT_00662620[]` 槽表) |

**对内核的实现含义**:`op_text` 需要按参数名识别 555 形态(`FILE`+`LET`),把 `LET` 当**左值**取变量,按上述切行规则逐行写数组元素;文本按原始字节保存(编码由 `--patch`/显示层决定),不需要在 TEXT 内解码。

当前内核仍按 495 形态处理:`op_text` 先对所有窗口做 `eval_window`,把 `LET` 的 `v …` 左值当值求,于是首错点报 `rt.expr.malformed: 求值结果是被引用的变量槽($2871),不是值`。实现 555 分支即可消除该 fatal。

#### `PRODUCT`(Confirmed,555 处理体 RE)

- 参数:`COM`(checker 1)+ `LET/PINT1..10/PFLT1..10/PSTR1..10`(32 项)。
- 处理体:opcode 116 → **`0x459670`**(4241 B;<v555-4> 555 exe)。
- 调用点(555 ysc 全脚本扫描;本轮新增 `probe_scan_cmd`):
  - `es_first`/`es_scenario`:`PRODUCT[80]` → `PRODUCT[81]` → `PRODUCT[80]`;
  - `es_debug` `es.InitLog`:`PRODUCT[51]`、`PRODUCT[52, $…]`;
  - `es_scenario_filelist`:`PRODUCT[29, LET=@…]`、`PRODUCT[21, PSTR=$…]`、`PRODUCT[24, PSTR=$…]`;
  - `es_scenario_load`:`PRODUCT[23, PINT=@…, PINT2=@…, LET=@…]`;
  - 菜单/其他:`PRODUCT[10201, …]`、`PRODUCT[21000, …]` 等。
- **COM 分支(逐条 RE,`0x459670`)**:

| COM | 官方分支 | 语义 |
|---|---|---|
| `0x15` (21) | `FUN_0045a785` | 把拖放/命令行文件枚举进 `DAT_00872394`(路径表)/`DAT_00872398`(计数),是 `_DROPFILE*` 的数据源 |
| `0x16` (22) | `FUN_0045a9dc` + `FUN_0045bc60` | 文件/字符串工具,结果写 LET |
| `0x17` (23) | `FUN_0045bcf8` + `FUN_0045bc60` | 文本/列表工具,结果写 LET(`es_scenario_load` 使用) |
| `0x18` (24) | `FUN_0045bd68` | 逐条扫描 `DAT_008726fc[0..DAT_00872700]`,读取并解码文件内混淆串、按 9 字节哈希过滤后追加到拖放表;调试打印 `YS_DROPFILE_NUM = %d` |
| `0x19` (25) | `GetSaveFileNameA`(BMP 过滤器,Flags `0x180e`) | 保存 BMP 文件对话框 |
| `0x1A` (26) | `FUN_0045c0b0` + `FUN_0045bb60` | 打开文件 helper,结果写 LET |
| `0x1B` (27) | `DAT_0087242c = 1` | 置内部标志 |
| `0x1C` (28) | `DAT_0087242c = 0` | 清内部标志 |
| `0x1D` (29) | `FUN_0045bc60` | 把模式位 `DAT_00872410` 写入 LET(BSS 初值 0) |
| `0x1E` (30) | `FUN_00409114(0,"COM:30 Deleted.")` | 空桩 |
| `0x1F` (31) | `FUN_00409114(0,"COM:31 Deleted.")` | 空桩 |
| `0x20` (32) | `SetCurrentDirectoryA`(`/`→`\`) | 切宿主当前目录 |
| `0x21` (33) | `FUN_0045c148` | 路径工具 |
| `0x22` (34) | `FUN_0045c1d0` + `FUN_0045bc60` | 路径/字符串工具,写 LET |
| `0x24`/`0x25`/`0x26` | `FUN_0045c4f2` + `c218`/`c278`/`c294`;`0x26` 再写 LET | 文件/字符串工具 |
| `0x33` (51) | `FUN_004696c0` | `es.InitLog` 调用;实现未展开(疑日志初始化) |
| `0x34` (52) | `FUN_0045c2ec("debuglog.txt",1,…)` | 写调试日志 |
| `0x50` (80) | `DAT_0087215c = 1` | 置渲染路径标志 |
| `0x51` (81) | `DAT_0087215c = 0` | 清渲染路径标志 |
| `0x64` (100) | `ShowWindow(hwnd,0)` | 隐藏窗口 |
| `0x6E` (110) | `ShowWindow(hwnd,5)` | 显示窗口 |
| `0x78` (120) | `ShellExecuteA("Open","IExplore",PSTR,…)` | 打开 IE |
| `0x2776..0x2779` | `FUN_0045c334/c3b4/c3d0/c3e4`(末项写 LET) | 文件工具 |
| `0x27D9` (10201) | `DialogBoxParamA("IDD_YSC")`;`-1` 时返回 -1 | YSC 调试对话框 |
| `0x27DD` (10205) | `FUN_0045bb60` | 写 LET |
| `0x4E25`/`0x4E26` | `SendMessageA(DAT_00872d80,7/8,0,0)` | 消息 |
| `0x4E2B`/`0x4E2C` | `DAT_008723b8 = 1/0` | 标志 |
| `0x4E2D`/`0x4E2E`/`0x4E35` | `FUN_0045c3fc/c44c`;`0x4E35` 写 LET | 工具 |
| `21000` (0x5208) | `FUN_0045c4f2` + `FUN_0045c540` + `FUN_0045bb60` | 文件/列表工具,写 LET |
| `1001` (0x3E9) | <v555-4> 处理体链中无此分支 | <v555-pc> 观测 5 次;落空返回 0,需 PC exe RE 确认 |
| `0x5227` (21031) | `GetWindowsDirectoryA` + `FUN_0045bb60` | 取 Windows 目录写 LET |
| 其他 | 落空,返回 0 | 未取证 |

- **5 个可读样本实际观测到的 COM**(`probe_scan_cmd PRODUCT`):

| 样本 | COM(次数) |
|---|---|
| <v555-1> | `34×3, 80×2, 90×2, 51, 52, 81, 10201, 10205, 21000` |
| <v555-2>/2/3 | `34×6, 20011×6, 20012×6, 22×3, 10201×2, 10205×2, 80×2, 21000×2, 51, 52, 20013, 20014, 29, 21, 24, 23, 81, 12` |
| <v555-pc> | `1001×6, 10201, 10205` |

  其中 **12(0x0C)、90(0x5A)、1001(0x3E9) 在<v555-4> 处理体链中没有分支**
  (落空返回 0);它们是作品/插件钩子还是别处消费,尚无可见行为证据。
  其余值都能在上表找到分支,或属于已取证的空桩。
- **`DAT_0087215c` 不是调试 UI 开关**:写入点只有 `0x459670` 的 `0x50/0x51`;
  读取点包括 `0x404909`、`0x407079`、`0x4070c5`、`0x40730e`、`0x407334`、
  `0x423ed2`、`0x45f2c2`、`0x45f4c1`、`0x45f56d`、`0x45fa6d`、`0x45fb30`。
  其中 `0x4075e0` 调用 D3D 设备对象 vtable `+0xc`,并把返回值与 `0x88760869`
  (`D3DERR_DEVICELOST`)比较 ⇒ 该标志是**渲染路径(D3D/非 D3D)开关**,
  不是 `es.DBG.INIT` 的门。
- **`DAT_00872410` 是另一个字节**(`0x1D` 写入 LET 的“模式位”),BSS 初值 0;
  `0x45bd68`(COM `0x18`)也读它决定是否拼接前缀。
- 运行表现(未改代码的当前内核):`rt.cmd.unimplemented: PRODUCT`
  (`es_scenario_filelist` 各 1 次);官方这些分支正常返回 0,所以它
  **不是** fatal,但会让场景文件枚举走空分支。
- 最小兼容面(仅取证结论,待主人批准后再实现):`0x1D → LET=0`、
  `0x15/0x18 → _DROPFILE_NUM=0/_DROPFILE=空`(宿主无拖放)、
  `0x50/0x51/0x33/0x34` 安全 no-op;其余 COM 显式挂号但不得 fatal。

#### `LOAD[MEM]` / `SAVE[MEM]`

- `<v555-1>` 静态使用 208 组;`LOAD[MEM=@…, DNO=@…, LET=@…]`,MEM 是表达式(变量),不是文件字符串。
- `<v555-2>` 同名 201 组。
- 当前 `op_load` 只识别 `FILE/DNO/LET/ID/IDNO/G_*`;**`MEM` 落入默认分支被忽略**。运行 240 帧未报错,说明它至少没有触发 fatal,但语义是否缺失 Unknown。
- `SAVE[MEM]` 2 组,同理。

#### 其它新面

- `VARINFO[STRFIRST]` / `VARINFO[NO2]`:各 2 组(`s81 pc1319`、`s102 pc291`)。
- `CGACT[GRADATION]`:1 组(`s9 pc106`,`ID` + `GRADATION=1` + `SET/SET2`)。
- `_DROPFILE_NUM` / `_DROPFILE`:运行日志中的 `rt.sysvar.unimplemented` 属 **0.495 既有缺口**(`DIV-RT-007`),不是 555 新增;0495 ABI 表本身包含这两个名字。

### 7.4 系统变量 ABI

`abi_match` 结果(4/4):

```text
ysv: version=555 records=3094..3375
table: 548 项 (sysvar-abi-0495-a626e40a.tsv)
appended ids (>=表长): 548..1339 / 548..1228 / …
MATCH
```

- 555 的 `ysv` 在 0.495 的 548 项前缀上**逐项相等**;表外 id(>=548)在 ysv 里是 `kind==1` 的声明。
- 本批脚本没有引用到新增系统变量名:运行时的 sysvar 未实现诊断只有既有的 `_DROPFILE*`(0495 表内 id 14/15,DIV-RT-007)。
- `Unknown`:表外 id 里是否夹带 555 新增系统变量(需要 555 的 ycd/名表;目前没有)。
- 结论:**这 4 个样本继续使用 0495 族 ABI 即可**;若后续作品引用 548+ 的系统变量名,再按该作的 ycd 或 RE 建新族。

### 7.5 555 处理体分发表(RE 锚点)

| 引擎 | 表基址 | `TEXT` | `TEXTINFO` | `PRODUCT` |
|---|---|---|---|---|
| 0.495 官方 | `0x7ace20` | opcode 96 → `0x453448` | 97 → `0x4538cc` | — |
| 0.555(`<v555-3>`) | `0x78b1a0` | opcode 98 → `0x452a0c` | 99 → `0x452d54` | 116 → `0x459670` |

- 分发表由装载器以 `mov dword [base+4*opcode], handler` 逐项填充;未覆盖项保持默认
  (`0x423168` / 0.495 的 `0x4634d8`)。表基址随每个 exe 构建变化,opcode→处理体的语义按 `ysc.ybn` 名对齐。
- **自证**:每个处理体入口把 opcode 常量传给参数装载器 —— `0x452a0c` 传 `0x62`(98)、
  `0x452d54` 传 `0x63`(99)、`0x459670` 传 `0x74`(116);0.495 的 `0x453448` 传 `0x60`(96)、
  `0x4538cc` 传 `0x61`(97)。因此表基址与 opcode 索引互相印证。
- Ghidra 不会为这类“只被数据表间接引用”的地址自动建函数;本轮用
  `disassemble + createFunction` 后反编译(`DecompAddrs.java`)。
- 这条表也解释了 495→555 的 opcode 位移必须按名解析:引擎自己的处理体表就是按新顺序重新编译的。

---

## 8. “同”与“不同”的边界

**未变(本轮实测)**

- YPF 容器格式(版本仍 500,方言 hashed-len);
- YSCM/YSLB/YSVR/YSTD/YSTL/YSCF/YSER 的二进制布局与解析方式;
- YSTB V5 四区段布局、四段独立异或;
- 存档容器版本 0x1e0;
- 系统变量 ABI 前缀(0495 族);
- 参数/命令的“按名解析”策略。

**变化**

- 表版本号统一 555;
- 命令表 119 → 121(ALIAS/MOVIE2* / PRODUCT,去 CDDA/EMOTEINFO);
- 14 条命令的参数表变化;
- `TEXT` 从“属性设置”变成“加载外置文本到变量”;
- `PRODUCT`、`LOAD/SAVE[MEM]`、`VARINFO[STRFIRST/NO2]`、`CGACT[GRADATION]` 等新面;
- 三份新作把剧情文本外置(`YSTD.text_count=0`),文本资源在 `sc.ypf/scenario/*.txt`;
- 多语发行版需要 GBK 文本路线(`--patch gbk`)。

---

## 9. 下一步(按证据成本排序)

> 以下是**当时的实现计划**。按主人 2026-11-19 口径,取证轮只做文档;
> 后续已获准实现 P0 前三项,落点与验证见 **§17**(2026-11-19)。
> 本节保留作验收口径;仍未实现的是 `MEM=99999`、`PRODUCT` 文件/对话框分支、
> `VARINFO[STRFIRST/NO2]`(§17.4)。禁止用改游戏脚本的大补丁绕过(§13/§14)。

### P0 — 解锁 555 主线

1. **`LOAD/SAVE[MEM]`**(5 个可读样本中使用最广:每作 `LOAD[MEM]` 193–210 次、`SAVE[MEM]` 2 次)
   - 语义已 RE(§16.10):`MEM` 是内存存档槽号;`SAVE[MEM=n]` 写入
     `DAT_008670c0[n]`/容量 `DAT_008678a0[n]`,`LOAD[MEM=n]` 从同一槽读
     (`DAT_00868860[n]` 偏移);容器仍是 `YSSD`/0x1e0,与文件存档同格式;
   - 当前 `op_load` 只按 `FILE/DNO/...` 走,`MEM` 被静默忽略,必须补齐;
   - 实现落点:`yu-save` 增加内存槽表,复用文件存档的序列化/反序列化;
     `MEM=99999` 的 `SAVE` 特殊分支待补 RE;
   - 验收:存档/读档 UI 的 MEM 调用不再被忽略,变量内容与官方一致。
2. **`TEXT[FILE, LET, CLEAR]`**(语义已由 RE 闭合,可直接实现;样本主线)
   - `op_text` 按参数名分派:出现 `FILE`/`LET` 且无 `ID` ⇒ 555 分支;
   - `FILE`:求值为字符串 → ASCII 小写化 → `/`→`\` → 走内核既有文件源打开(不补扩展名);
   - 读字节 → 追加 CRLF → 按 CRLF/CR/LF 切行 → 每行末尾加 `0x0D` → 写入 `LET` 字符串数组元素 `0..N-1` → 末尾再写一个空元素;
   - `LET`:用 `eval_lvalue_window` 取左值,校验目标为字符串数组(YSVR `ty=3, dims=[N]`);越界即停;
   - `CLEAR=1`:复用 495 的文本缓冲清理。
   - 验收:三份样本越过 `es_scenario_load.yst pc6`;`$B4..$B9` 元素 0 与文件首行逐字节一致(含行尾 CR)。
3. **`PRODUCT` + `_DROPFILE*`**
   - `COM=0x1D → LET=0`(与 BSS 初值一致);`0x15/0x18` 在宿主无拖放时
     使 `_DROPFILE_NUM=0`/`_DROPFILE=""`;`0x50/0x51/0x33/0x34` 安全 no-op;
     其余 COM 显式保留未实现日志(含 PC 的 `COM=1001`,在<v555-4> 处理体中落空)。
   - 验收:`es_scenario_filelist.yst pc8` 不再 `rt.cmd.unimplemented`;
     `_DROPFILE*` 不再 `rt.sysvar.unimplemented`;场景列表走正常分支。
4. **系统变量 ABI 台账**
   - 采用实测 1000 槽表(§16.2 / `v555-sysvar-abi-measured.tsv`):
     0..547 具名、548..999 保留、1000+ 作品全局;
   - 当前 548 项 ABI 表对具名系统变量足够;548..999 在 5 个可读样本 0 引用,
     不需要为它们扩展 sysvar 表;若未来作品引用,再按 ycd/getter RE 补。
5. **555 独立剧本链路**
   - 用 `s56–s76` 的真实脚本对拍 `TEXT[FILE]` 的行数组、合并结果与
     lexer/parser 执行序(见 §13);不要改这些脚本。

### P1 — 补齐 555 常用面

6. `VARINFO[STRFIRST]` / `[NO2]`:<v555-1>/样本各 2 组;语义可由 495 的 `SEARCH`/`NO` 族类推,但需 RE 确认。
7. `CGACT[GRADATION]`:<v555-1>/样本各 1 组;先看 555 处理体是否等价于既有渐变/着色路径。
8. 文件源优先序(§16.6):先做受控实验,再决定是否调整 `SourceOrder`;不得单方翻转。

### P2 — 表里有、语料未用

6. `ALIAS`、`MOVIE2/MOVIE2END/MOVIE2INFO`、`WEBP`、`MATH[CALC]`、`CG` 旋转/裁剪族、`CGINFO[SEARCH/NUM/NAME/LSET/NO]`。
   - 按 AGENTS 终态政策:有三层证据才实现;否则保留显式 `Unimplemented` / `unsupported-by-official`,不得长期挂空。

### 测试与台账

7. 把 4 套语料纳入版本回归:
   - 表版本/计数/结构闭合(YSTD 两条公式、YSTL↔YSLB、argc 和);
   - 密钥:exe ID → CRC32 → 首窗口钥三者一致;
   - 运行:`<v555-1>` 240 帧无 VM 诊断;样本 `--patch gbk` 首帧 + 文本链路。
8. 新增 `docs/divergences` 档案时,555 的 `TEXT`/`PRODUCT` 若按 RE 实现且手册无页,标注 `fixed(no-manual)` 并附 555 exe 锚点。

---

## 10. 复现命令

```bash
# 0) 环境
export TEMP=<side-repo>/workbench/tmp; export TMP=$TEMP

# 1) 七表版本 + YPF 方言(封包/散盘均可)
<repo>/target/debug/examples/version_probe <games-555>/<v555-1>/pac/ysbin.ypf

# 2) 真引擎启动(headless,首帧)
<repo>/target/debug/yu-cli run <games-555>/<v555-1> \
  --mode headless --frames 1 --save-root <tmp>/saves --log-level debug
# 样本:
<repo>/target/debug/yu-cli run <games-555>/<v555-2> \
  --mode headless --frames 400 --patch gbk --save-root <tmp>/saves --log-level info

# 3) 静态命令/参数普查
<repo>/target/debug/examples/census_params <games-555>/<v555-2> \
  TEXT TEXT:FILE TEXT:LET PRODUCT LOAD:MEM VARINFO:STRFIRST CGACT:GRADATION

# 4) 系统变量 ABI
<repo>/target/debug/examples/abi_match <tmp>/<v555-2>/ysbin/ysv.ybn \
  <repo>/crates/yu-rt/data/sysvar-abi-0495-a626e40a.tsv

# 5) 单脚本指纹(参数名/命令序)
<repo>/target/debug/examples/dump_script <games-555>/<v555-2> 60 0 40
```

---

## 11. 残留未知(按证据缺口)

| 项 | 现状 | 需要 |
|---|---|---|
| 系统变量 ABI(完整 1000 槽) | **实测**:0..547 具名 + 548..999 `ty=0` 保留;0.495 与 6 个 555 样本 0..999 逐字段 0 差异;works 全局从 1000 起;548..999 在 5 个可读样本 0 引用 | 名称/语义需官方 `YSCom.ycd` 或 sysvar getter 分支 RE |
| `TEXT[FILE/LET/CLEAR]` 精确语义 | **Confirmed**(555 处理体 `0x452a0c`;按行写 LET 字符串数组,行尾 CR);调用链见 §13 | 实现后用官方 trace 做逐字节验收 |
| `PRODUCT` COM 映射 | **Confirmed**(§7.3 全分支表);PC 额外观测 `COM=1001`,<v555-4> 处理体链中无此分支(落空返回 0) | PC/555 产品分支 RE(仅当 1001 有可见行为) |
| 555 独立剧本系统 | **作品级选择**(§16.7):样本=文本 DSL + `s56–s76`;<v555-1>/PC=经典编译 YSTB | <game-c> 剧本形态需完整包 |
| 顶部 UI 条(`POSLOCK/QSAVE…`) | **Confirmed 非官方**(§14) | 官方 555 命令级 trace 定位分歧命令 |
| `LOAD/SAVE[MEM]` | 参数存在、每作 193–210/2 次、当前被忽略 | **优先级最高**:555 LOAD/SAVE 处理体 RE |
| 文件源优先序 | 555 事实单(Release=0 封包优先)与本仓库 0.495 RE(loose-first)相反;6 样本优先级随作品变化 | 同游戏、Release=0/1、松散 vs 包内同名资产的受控实验 |
| `ALIAS` / `MOVIE2*` | 表里有;5 个可读样本 0 次使用 | 等有使用面的语料 |
| `EMOTE` 555 新签名 | 24→18(E-ris ABI 变更);非 E-ris 样本 0 次;<game-c> 脚本不在包内 | E-mote 家族按 `deferred(master)`;不抢跑 |
| `CG`/`CGINFO`/`MATH[CALC]`/`FILEACT[WEBP]` 新参数 | 表里有;5 个可读样本 0 次使用 | 等有使用面的语料 |
| 555 新增表达式载荷 `B/I/W/F` | 已观测,未逐一解释 | 用 555 处理体/表达式层对照 495 |
| YSLB `field_b` 语义 | 495 起就未证 | 既有开放项,555 未改变形状 |
| `<v555-1>` 官方同作可视对照 | 本机 `<v555-1>.exe` 启动 `0xC0000142`;`<v555-1>_gpt.exe` 子进程无窗口 | 需要可运行环境或官方 trace |
| 样本原始性 | 版本资源被第三方改写 | 需要原始发行版哈希时另取 |

---

## 12. 本轮纪律

- 全部操作只读样本;探针产物写入 `<side-repo>/workbench/tmp/probe555/`;
- 未执行任何 git 写操作;
- 未使用 `save/data` 作为入口;未改样本目录内文件;
- 文档中的真名/路径一律用占位符,对照只留在 `AGENTS.md`。

---

## 13. 555 独立剧本系统(Confirmed,架构取证 2026-11-19)

> 结论先行:555 商业作的剧情**不是**全部编译进 `yst%05d.ybn`。
> 框架脚本编译在包内;剧情正文是**独立文本文件**(`scenario/*.txt`、
> `sc_cn_scenario/*.txt`、`sc_tw_scenario/*.txt`),由包内一整套
> 解释器脚本用 `TEXT[FILE, LET]` 读入字符串数组后再逐行解释。
> 这是 555 与 0.495 ERIS 语料最大的结构差异,也是样本首个 fatal 的来源。

### 13.1 剧本资产在包内的形态

以 `<v555-2>`(`update1.ypf`)为例:

```text
sc_cn_scenario/_staff.txt                 2622 B
sc_cn_scenario/saku_01_00.txt             7472 B
sc_cn_scenario/saku_01_01.txt             8330 B
sc_cn_scenario/…                          (共 66 条)
```

`<v555-3>`(`update1.ypf`)同时有:

```text
sc_cn_scenario/scenario_main.txt          3983 B
sc_cn_scenario/scenario_start.txt         2387 B
sc_tw_scenario/03_01_01.txt               11714 B
sc_tw_scenario/…                          (繁体/另一语种变体)
```

文本本身是**ERIS 脚本文本 DSL**:

```text
#SCENARIO_MAIN

\GO(saku_01_00)
```

```text
#saku_01_00
#RP_ona001
//
\Background(BG_black,,1)
\FIN(2000, bg_effect_00)
\BGM(bgm05)
\VO(MBA_SAKU_01_00_0001)
\LC("【Big Sister】「真伤脑筋……」")//￡
\DrawCG(tac005_o,bg_effect_00,3000)
```

- `#label` = 场景标签;`\Command(args)` = 宏调用;`//` = 注释;
  `\LC("…")` = 台词行(行尾 `//￡` 是原工具标记)。
- 文本按原始字节保存在包内;`sc_cn_` 与 `sc_tw_` 是同一逻辑场景的
  两个语言变体(简中/繁中),由加载器按行合并。

### 13.2 解释器脚本族(包内 `ysbin`)

`<v555-2>` 的 `yst_list.ybn` 中,`data\script\eris\scenario\` 下一组脚本:

| sid | 路径 | 角色 |
|---|---|---|
| 56 | `es_scenario_filelist.yst` | 场景文件枚举(`scenario/` 前缀、`PRODUCT`、`_DROPFILE` 过滤) |
| 57 | `es_scenario_flow.yst` | 场景流程 |
| 58 | `es_scenario_label.yst` | 标签表 |
| 59 | `es_scenario_lexer.yst` | 词法(文本行 → token;528 组,`LET×340`) |
| 60 | `es_scenario_load.yst` | **加载器**;`TEXT[FILE]` 读 base/`sc_cn_`/`sc_tw_` 三份并合并 |
| 61 | `es_scenario_parser.yst` | 解析(304 组;`LOAD×8`/`SAVE×4`) |
| 62 | `es_scenario_text.yst` | 文本行处理 |
| 63–76 | `scenario\macro\*.yst` | 宏定义/替换(演出、声音、控制、变量、立绘…) |

命令集合(`probe_cmdset` 统计 s56–s62 并集,节选):

```text
LET×587 IF×145 IFEND×145 GOSUB×113 LOOPCONTINUE×52 INT×51
RETURN×42 S_INT×37 STR×25 LOOP×22 LOOPEND×22
LOAD×8 DEBUGLIST×7 PRODUCT×7 TEXT×6 CGACT×5 VARINFO×5
SAVE×4 WAIT×2 CG×1 CGEND×1 FILEACT×1 FONT×1
```

> `DEBUGLIST` 在 0.495 是官方 no-op(现有台账),不需要实现。

### 13.3 加载链与 `TEXT[FILE]` 的真实用法

`es_scenario_load.yst`(`s60`)pc0–pc9 是三个成对函数:

```text
pc0  STR  $2993 = $PSTR[1]
pc1  TEXT  FILE=$2993            LET=$2868
pc2  TEXT  FILE="sc_cn_"+$2993   LET=$2869
pc3  TEXT  FILE="sc_tw_"+$2993   LET=$2870
pc4  RETURN

pc5  STR  $2994 = $PSTR[1]
pc6  TEXT  FILE=$2994            LET=$2871
pc7  TEXT  FILE="sc_cn_"+$2994   LET=$2872
pc8  TEXT  FILE="sc_tw_"+$2994   LET=$2873
pc9  RETURN
```

pc10 起把三份行数组按 `@2995..@2997`(`$2868..$2870` 的首元素长度)
和 `@3000/@3001` 合并;`es.SCENARIO.CRPREPLACE` 处理 CR;
每行长度写入 `@2874[]`。因此:

- `TEXT[FILE]` 的 `FILE` 是**完整资源名**(`sc_cn_` + 名称),
  由脚本拼接,不自动补扩展名 —— 与 555 处理体 RE 一致;
- 三份文件**缺失任意一份都不是 fatal**:官方处理体返回 -1,脚本继续;
- 读入的每一行末尾带 `0x0D`(555 处理体写 LET 时追加);
- 合并后再次交给 lexer/parser(`s59`/`s61`)。

### 13.4 文件枚举与 `_DROPFILE`

`es_scenario_filelist.yst`(`s56`)pc0–pc36 的核心分支:

```text
pc6   STR  $2944 = "scenario/"
pc8   PRODUCT COM=29 LET=@2945        ; 模式位 DAT_00872410
pc9   IF @2945 != 0
pc10    PRODUCT COM=21 PSTR=$2944     ; 枚举拖放/命令行文件
pc11    LOOP @15 (=_DROPFILE_NUM)     ; 逐个 $DROPFILE[i]
        … 收集 $2940[]
pc16  ELSE
pc19    PRODUCT COM=24 PSTR=$2944     ; 扫描文件内混淆串(9 字节哈希过滤)
pc20    LOOP @15
        … 按文件名匹配 $2946,收集
pc28    IF @2947 == 0 → 返回/给出默认值
```

- `@14/@15` 在 0495 ABI 中正是 `_DROPFILE`(STR[1024])/
  `_DROPFILE_NUM`(INT);官方正常启动没有拖放文件时两者为 0/空。
- 因此**最小兼容面**是:`TEXT[FILE]` + `PRODUCT[0x1D]=0` +
  `_DROPFILE_NUM=0`/`_DROPFILE=""`;`PRODUCT[0x18]` 的真实文件枚举
  只在拖放/命令行启动路径需要,常规启动可由空列表落回默认分支
  (推断,待官方命令级 trace 验收)。
- 注意:`PRODUCT[0x18]` 不是“遍历 VFS 目录”。RE 显示它遍历
  `DAT_008726fc[0..DAT_00872700]`(计数上限 `0x64`,写入点 `0x4646xx`),
  逐文件读取并解码混淆串,再按哈希过滤。该表是引擎维护的文件路径表
  (与拖放/命令行列表同型,最多 100 条),不是包目录枚举;
  写入函数尚未逐行归属。

### 13.5 当前内核首错点(未改代码,仅记录)

`<v555-2>` 120 帧(`--patch gbk`)当前运行诊断:

```text
UNIMP [vm] rt.cmd.unimplemented @ es_scenario_filelist.yst pc8: PRODUCT
FATAL [vm] rt.expr.malformed @ es_scenario_load.yst pc6:
  求值结果是被引用的变量槽($2871),不是值
UNIMP [vm] rt.sysvar.unimplemented @ es_scenario_filelist.yst pc11:
  _DROPFILE_NUM
UNIMP [vm] rt.sysvar.unimplemented @ es_scenario_filelist.yst pc12:
  _DROPFILE
```

- `rt.expr.malformed` 的根因:现有 `op_text` 按 0.495 形态先把所有窗口
  `eval_window`,把 555 的 `LET` 引用窗口当值求;不是 YSTB 解析错误。
- `PRODUCT`/`_DROPFILE*` 是静默诊断,不会直接 fatal,但会让文件枚举
  走空分支。
- 这三项构成 555 剧情链路的**最小兼容面**,属于后续实现项,不是
  脚本补丁。

---

## 14. 顶部 UI 条取证:`:/W1/DBG` 不是它(2026-11-19)

> 结论先行:当前内核在启动过渡阶段显示的那条
> `LOCK / DEFAULT / Q.SAVE / Q.LOAD / SAVE / LOAD / SCREEN / CONFIG`
> 不是 `es.DBG.INIT` 的 `:/W1/DBG` 调试窗口,而是
> `メイン画面設計`(`s168`)定义的 **GAMEMAIN 窗口控制按钮条**。
> 同版本官方 555 运行(窗口化,非全屏)没有这条;根因尚未闭合,
> 禁止用改脚本/大补丁掩盖。

### 14.1 启动链:`es_first` 与 `es.DBG.INIT`

`<v555-1>` `es_first.yst`(`s10`):

```text
pc153  IF @_DXMODE                 ; @113,值为 0 时跳过 DXBUF 分支
pc154..158  WINDOWINFO/CG[ID=DXBUF…]
pc159  IFEND
pc160  WINDOW[0,0]                 ; 主窗口先隐藏(0.495 语料同型 WINDOW[NO=0 E=0])
pc161  GOSUB es.DBG.INIT           ; 无条件
pc162  GOSUB es.EF.TASK
pc163  WAIT[1]
pc164  WINDOW[0,1]                 ; 主窗口恢复(同型 WINDOW[NO=0 E=1])
```

- `dx_mode=0` 实测 trace:`if_false s10 pc153 -> pc159`,
  随后 `gosub s10 pc161 -> s6 pc15`;`@_DXMODE` 不是 DBG.INIT 的门。
- `es.DBG.INIT`(`s6` pc15–pc26)只做:
  - `CG[ID=":/W1/" E=1 X=@1921 Y=@1922]`
  - `CG[ID=":/W1/DBG" E=1 … FILE=""]`
  - `IF @1162[37][21]` → `ES.DEBUGMODE.BTN.WATCHWINDOW.ON`
  - `IF @1162[37][31]` → `ES.DEBUGMODE.BTN.SPRITEMAPVIEW.ON`
- `ES.DEBUG.DEFINE`(`s187`)默认把 `@1162[37][21]`、`[37][31]`
  设为 0,两个调试按钮不会打开;`:/W1/DBG` 本身在 frame 180 的
  display JSON 里只是 **offscreen `:/W1` pass 的 240×600 纯黑 1×1**,
  没有合成进屏幕。

### 14.2 可见条的真实图层(frame 180,`<v555-1>`)

`--json` 导出的 Screen pass(节选;全部为**当前帧活图层**):

```text
:/W0/BT.OFFES.GAMEMAIN.BTN.POSLOCK=0=1   z=99000000020 x=532  y=1  snapshot 13
:/W0/BT.OFFES.GAMEMAIN.BTN.POSINIT=0=1   z=99000000020 x=585  y=1  snapshot 18
:/W0/BT.OFFES.GAMEMAIN.BTN.QSAVE=0=1     z=99000000020 x=665  y=1  snapshot 22
:/W0/BT.OFFES.GAMEMAIN.BTN.QLOAD=0=1     z=99000000020 x=730  y=1  snapshot 26
:/W0/BT.OFFES.GAMEMAIN.BTN.SAVE=0=1      z=99000000020 x=799  y=1  snapshot 30
:/W0/BT.OFFES.GAMEMAIN.BTN.LOAD=0=1      z=99000000020 x=849  y=1  snapshot 34
:/W0/BT.OFFES.GAMEMAIN.BTN.SCREEN=0=1    z=99000000020 x=904  y=1  snapshot 38
:/W0/BT.OFFES.GAMEMAIN.BTN.CONFIG=0=1    z=99000000020 x=976  y=2  snapshot 42
```

- 这些 ID/位置与 `メイン画面設計.txt`(`s168`)pc85 起连续定义的
  `es.BT.NAME.SET "■LOCK"`/`"■DEFAULT"`/`"■QSAVE"`…、
  `es.BT.CG.SET "main/button/type1/btn_lock_bt4"`…、
  `es.BT.XY.SET` 一一对应;组号为 `es.BT.GROUP.SET 1 0 0 0`。
- 位置常数来自 `@1000=-159`、`@6294=-532`、`@6295=-159`
  (`@6295=@1000`),即按钮条固定落在 y=1(屏幕顶边)。
- frame 240(标题出现)时这些图层已不在 Screen pass;frame 180→240
  之间按钮条被隐藏,说明它是**启动过渡期的瞬态显示**,不是常驻 UI。
- 因此:它不是 `:/W1/DBG`(后者在 offscreen 且纯黑),也不是
  `es_debug.yst` 的调试按钮(`debug/btn_watch_*` 等,只有
  `@1162[37]` 打开时才显示)。

### 14.3 同版本官方运行对照

用 Win32 `PrintWindow` 定时抓官方 555 窗口(脚本只读观察,结束即杀进程):

| 样本 | 窗口 | 时刻 | 结果 |
|---|---|---|---|
| `<v555-2>` | 868×517(窗口化,PrintWindow 整窗) | 4/10/18/28 s | 黑屏/CAUTION,无顶部条 |
| `<v555-3>` | 2575×1407(PrintWindow 整窗) | 3/6/10/15 s | 黑屏/加载画面,无顶部条 |

- 官方启动器(`<v555-1>.exe`)在本机以 `0xC0000142`(DLL init failed)
  退出,`<v555-1>_gpt.exe` 子进程无窗口;因此 `<v555-1>` 暂无官方同作
  可视对照,只有两个样本同版本对照。
- 官方窗口是窗口化 868×517(非全屏),仍然没有这条 ⇒ “全屏才有/没有”
  的窗口模式假设不成立。

### 14.4 尚未闭合的点(不得据此打补丁)

- 按钮条为什么在 frame ~180 可见:可能来自 `es.BT.ACTIVEGROUP`/
  `es.BT.DRAW` 的组激活时序、`es.BT.ONOFF` 初值,或标题加载前的
  过渡帧设计;目前只证明“图层是 GAMEMAIN 按钮条 + 官方不显示”,
  未证明具体分歧命令。
- `@1000/-159`、`@6294/-532` 的写入者尚未定位(常量在 `s168` pc0–
  pc2 计算,但源变量由谁初始化未闭合)。
- 下一步取证(按成本排序):
  1. 用 555 官方 exe 的命令级 trace(参考 v494 `engine_trace_v494b.py` 的
     handler 断点法)对比 `es.BT.ACTIVEGROUP`/`es.BT.DRAW` 的调用序;
  2. 对比官方/内核在 frame 180 的 `@1082/@1083/@1208/@1005/@1216[7777]`
     与按钮组状态;
  3. 只有在拿到“官方不执行/不显示”的明确命令证据后,才讨论实现修正。

### 14.5 纪律

- 本轮**没有**修改 `es_first`/`es_debug`/`s168` 或任何游戏脚本;
- 顶部条不是官方 UI ⇒ 不能用“把脚本调用删掉/把图层隐藏”的大补丁;
- 任何修正必须落在命令语义/宿主状态层,并附官方 trace 或处理体证据。

---

## 15. 本轮新增取证工具(未跟踪,仅本地)

| 工具 | 用途 |
|---|---|
| `crates/yu-rt/examples/probe_scan_str.rs` | 按密钥扫描全部脚本的字符串字面量(支持 `YSC_DIR` 复用包内 ysc) |
| `crates/yu-rt/examples/probe_scan_cmd.rs` | 按命令名扫描全部脚本,打印 pc/行号/首窗口值 |
| `crates/yu-rt/examples/probe_cmdset.rs` | 统计指定脚本的命令集合 |
| `crates/yu-engine/examples/probe_vars555.rs` | 启动 555 工程并按 `id:idx` 读运行时变量 |
| `crates/yu-engine/examples/probe_domain555.rs` | 打印启动期 CG/CGACT 域事件(可按 ID 过滤) |
| `crates/yu-rt/examples/probe_script_pc.rs` | 既有探针;本轮增加可选密钥参数,便于 555 包内脚本反汇编 |
| `crates/yu-rt/examples/probe_varid555.rs` | 按 id 统计脚本变量引用(用于判断 548..999 保留槽是否被使用) |
| `crates/yu-rt/examples/probe_param555.rs` | 按“命令:参数名”统计 555 新增参数的使用次数 |

> 这些是 `<side-repo>/workbench/tmp/probe555/` 取证流程的一部分,
> 不进入产品代码路径;是否入库由主人决定。

---

## 16. 跨语料复核(2026-11-19 新语料 + PC 第 6 样本)

> 新语料:`<games>/<game-c>-pack`
> (<game-c>,`ysc/ysv` 原文件 + 反编译/反汇编 + 事实单);
> 追加样本:`<games-555>/PC`(<v555-pc>,
> `<v555-pc>.exe`,v555)。
> 只读取证;不修改语料、不执行语料内代码。

### 16.1 新语料指纹核对(独立复算)

| 项 | 事实单声明 | 本轮独立结果 |
|---|---|---|
| `ysc.ybn` SHA-256 | `7ee03ebb…55e1` | **一致**;11102 B;`YSCM` 555 / 121 命令 / 1113 参数 |
| `ysv.ybn` SHA-256 | `843b4469…71cb9` | **一致**;59606 B;`YSVR` 555 / 3787 条 / 逐字节闭合 |
| 命令表跨样本 | 与另一款 555 作品 0 差异 | **更强**:4 个既有 555 样本 + PC + <game-c> 的 `ysc.ybn` **字节完全相同**(同一 SHA-256) |
| `ysv` ABI 前缀 | id 0..547 与 0495 表 0 差异 | **一致**;且 id 0..999 与 0.495 SDK `ysv` 逐字段(flag/file_id/ty/dims/init)0 差异 |
| YPF `bn.ypf` | 365 条目 / v500 / key 0xC9 | **本包不含 `bn.ypf`**,无法独立复算;PC 的 `bn.ypf` 是另一个样本(342 条目,v500),不能替代 |

> <v555-pc> 还带 1.01 / 1.02 补丁包(各自一份 `bn.ypf`)。三份 PC `bn.ypf` 的
> `ysc.ybn` / `ysv.ybn` **哈希完全相同**(`7ee03ebb…` / `ef2ea570…`),
> 只有 `yscfg.ybn` 的标题/版本与 `yst_list.ybn` 变化 ⇒ 再次证明
> `ysc/ysv` 是引擎 ABI,`yscfg/yst_list` 是作品数据。
>
> PC 的 `ysbin` 共 **334 个 YSTB,全部 version=555**;用
> CRC32(`<v555-pc>`)=`6732A887` 全量解析 334/334 通过
> (`probe_cmdset`);`yst_list` 342 条(其中 8 条纯声明无 `.ybn`)。
> 命令总量:GOSUB 48060、WORD 18174、RETURNCODE 17725、LET 12229、
> LOAD 440、SAVE 250、PRODUCT 8、TEXT 1。

### 16.2 重新测量 v555 系统变量 ABI(核心)

方法:对 0.495 SDK `ysv.ybn` 与 6 个 v555 样本(<game-c>、4 个既有样本、PC)逐条解析
`kind==1` 记录,比较 `(flag, file_id, ty, dims, init)`。结果:

```text
v555 样本数                     6
0.495 vs 每个 555 在 id 0..999  逐字段差异 = 0
id 0..547                       与 0495 ABI 表 (type,dims) 0 差异
id 548..999                     恰好 452 条,全部 ty=0 / flag=0 / dims=() / init 空
                                —— 0.495 SDK 也恰好是同样 452 条(548..999)
id 1000 起                      各作品自定义全局(类型/维数/初值随作品不同)
```

因此本轮的 ABI 结论:

1. **v555 的具名系统变量仍是 0.495 的 548 项**(id 0..547),没有新增具名项;
2. id 548..999 是**0.495 就已存在的 452 个 `ty=0` 保留槽**,不是 555 新增;
   它们没有类型/维数/初值,名称在 ycd 里(商业包不发行 ycd),本轮无法命名;
3. 游戏自定义全局从 **id 1000** 开始(编号规则:0..547 系统变量、
   548..999 编号垫片、1000+ 用户变量);0.495 SDK 为 1000..1166,
   <game-c> 1000..1219,<v555-1> 1000..1339;
4. 5 个可读脚本样本(<v555-1>、3 样本、PC)对 id 548..999 的引用次数 **0**
   (`probe_varid555`),所以当前内核把 548+ 当普通变量域不会在这批样本上产生可见分歧;
5. 新语料事实单把 548..1219 全部称为“555 追加 672 条”是**过度归因**:
   其中 452 条是 0.495 继承的保留槽,<game-c> 真正的作品全局只有 220 条
   (id 1000..1219);<v555-1> 是 340 条(1000..1339)。

测量产物:[`v555-sysvar-abi-measured.tsv`](v555-sysvar-abi-measured.tsv)
(1000 行:`id / name / type / dims / init_hex / init_text / source`;
0..547 名称来自 0495 ABI 表,548..999 名称留空并标 `reserved-ty0`)。

### 16.3 命令表差异(精确对齐)

`ysc_diff2.py` 的序列对齐结果已并入 §6.3。补充结论:

- 0.495 SDK(119) → 555(121):新增 `ALIAS / MOVIE2 / MOVIE2END / MOVIE2INFO / PRODUCT`,
  删除 `); / CDDA / EMOTEINFO`;104 条命令的 opcode 位移(必须按名解析)。
- `PRODUCT` 不是 555 首创:0.481 / 0.494 商业表已有,0.495 SDK 表没有;
  555 只是重新纳入。`);` 在 0.481/0.494 也已不存在。
- 尾部 37 条消息与 256 B 键表与 0.495 **逐字节相同**(555 的 ERROR 参数表虽增
  `COM_ALIAS`,但消息文案未变)。
- `SYSTEMMODE` 0.495 的 4 个无名占位参数在 555 被删除,并加入 `WEBP`;
  0 基下标 20/21/22 = `FILEPRIORITYDEVELOP/DEBUG/RELEASE`(与新语料事实单一致)。

### 16.4 使用面普查(6 样本,5 个可读脚本)

| 555 新面 | <v555-1> | <v555-2>/2/3 | <v555-pc> | 结论 |
|---|---:|---:|---:|---|
| `TEXT[FILE/LET]` | 0 | 6/6/6 | 0 | 仅样本的文本剧本系统 |
| `PRODUCT` | 13 | 39/39/39 | 8 | 全部作品都用 |
| `LOAD[MEM]` | 208 | 201/210/210 | 193 | **使用最广的 555 新参数** |
| `SAVE[MEM]` | 2 | 2/2/2 | 2 | 同上 |
| `VARINFO[STRFIRST/NO2]` | 2/2 | 2/2 | 0 | 部分作品 |
| `CGACT[GRADATION]` | 1 | 1/1/1 | 0 | 部分作品 |
| `CG` 13 个新参数 | 0 | 0 | 0 | 全部未用 |
| `CGINFO[SEARCH/NUM/NAME/LSET/NO]` | 0 | 0 | 0 | 全部未用 |
| `MATH[CALC]` / `FILEACT[WEBP]` | 0 | 0 | 0 | 全部未用 |
| `EMOTE` / `ALIAS` / `MOVIE2*` | 0 | 0 | 0 | 非 E-ris 样本未用;<game-c> 脚本不在包内,无法验证 |
| `TEXTINFO` | 0 | 0 | 0 | 555 已变 0 参数,但本批无执行 |

> 这直接决定优先级:迫在眉睫的主线是
> `TEXT[FILE/LET]`(仅样本)、`PRODUCT`、`LOAD/SAVE[MEM]`、
> `VARINFO[STRFIRST/NO2]`、`CGACT[GRADATION]`;
> CG/EMOTE/ALIAS/MOVIE2 的新参数可以等有使用面的语料再实现。

### 16.5 YSCF 555 新语义:`WEBP = 6`

5 个可核对样本(4 个既有 555 + PC;<game-c> 取证包未含 `yscfg.ybn`)的图像扩展名序均为:

```text
+0x1C: [1, 2, 3, 4, 5, 6, 0, 0]
        png bmp jpg gif psb webp
```

- 0.495 的已知语义只到 `5=psb`;555 新增 **`6=webp`**,与
  `SYSTEMMODE[WEBP]` / `FILEACT[WEBP]` 一致。
- 现有 `yscf` 解析器原样保存 8 字节,不会破坏;但文档/语义层必须补 `6=webp`。
- 画面尺寸:<v555-pc> 为 **1152×720**,其余 5 个为 1280×720 ⇒ 尺寸是作品配置,不是引擎常量。

### 16.6 文件源优先序:两边结论冲突(未裁决)

- 新语料事实单:555 `yscfg` 的 `filePriorityDevelop/Debug/Release` 在
  `+0x3C/+0x40/+0x44`;`Release=0` ⇒ 源序 `[0,1,2,3]`(封包先于目录),
  `Release=1` ⇒ `[0,1,3,2]`(目录先于封包);Dir-A 补丁把 `Release` 改成 1 来
  “免封包”。
- 本仓库 `SourceOrder`:依据 0.495 `DAT_0080288c=0` 记为 **loose-first**;
  默认 `LooseFirst`,与上述方向相反。
- 本轮实测 6 个 555 样本的 `(develop, debug, release)`:

```text
<v555-1>         (1,0,0)
<v555-2>       (1,1,0)
<v555-4>/3     (0,0,0)
<v555-pc>   (1,1,0)
<game-c>         (1,1,0)   ← 事实单
```

  ⇒ 这组值**随作品变化**,不是“零售 555 一律 (1,1,0)”。事实单的表述只对 <game-c>/PC 成立。
- 目前**不能**据此翻转 `SourceOrder`;需要一份受控实验裁决:
  同一 555 游戏、同一松散文件 vs 包内文件、分别设 `Release=0/1`,
  用官方 exe 读同一资产并比对字节/画面。实验完成前保持现状并标 `Likely`。

### 16.7 独立剧本架构的适用范围修正

- 样本(`<v555-2/3/4>`):包内有 `es_scenario_filelist/lexer/load/parser/text`
  与 `scenario/*.txt` / `sc_cn_*` / `sc_tw_*` ⇒ **文本 DSL 剧本**。
- <v555-1> / PC(<v555-pc>):包内没有上述解释器脚本,剧情是编译后的
  `yst%05d.ybn` ⇒ **经典编译剧本**。
- 因此“555 剧本是独立文本文件”是**作品级作者选择**,不是 555 引擎的强制格式;
  兼容层必须同时支持两种形态,不能把所有 555 作品都按文本剧本处理。
- <game-c> 取证包不含 `.yst`/`scenario` 文件,其剧本形态本轮无法判定。

### 16.8 对提供事实单的勘误/保留

| 事实单条目 | 本轮判断 |
|---|---|
| “555 只追加 672 条系统变量” | **过度归因**:452 条是 0.495 继承的 `ty=0` 保留槽;真正作品全局 <game-c> 220、<v555-1> 340 |
| “零售 v555 样本 `(dev,debug,release)=(1,1,0)`” | 仅 <game-c>/PC;<v555-1>=(1,0,0),<v555-4>/3=(0,0,0) |
| “YPF 365 条目 / key 0xC9” | 包内不含 `bn.ypf`,无法复核;PC 样本 342 条目,不能替代 |
| “命令表与另一款 555 作品 0 差异” | **成立且更强**:6 个 555 样本的 ysc 字节完全相同 |
| “Release=0 → 封包优先” | 与本仓库 0.495 RE 相反,**未裁决**;不得单方改 `SourceOrder` |
| “`sysvar_abi.rs` 的 `Likely` 可改 `Confirmed(v555)`” | 0..547 前缀成立;但 548..999 是继承保留槽,推荐表述为“0.495 ABI + 452 保留槽”,不是“555 新增” |

### 16.9 残留未知(本轮新增)

| 项 | 需要 |
|---|---|
| id 548..999 的名称/语义 | 官方 555 `YSCom.ycd` 或引擎 sysvar getter 的 id 分支 RE |
| `LOAD/SAVE[MEM]` 精确语义 | **Confirmed**(§16.10:内存槽 `DAT_008670c0[n]`/`DAT_00868860[n]`,YSSD/0x1e0);剩 `MEM=99999` 特殊分支 `FUN_0044d82c` 未展开 | 实现内存槽 + 99999 分支 RE |
| `PRODUCT COM=1001`(PC 观测) | PC/555 产品分支 RE;目前<v555-4> 处理体链中无此分支(落空返回 0) |
| E-ris `EMOTE` 555 新签名 | <game-c> 脚本或 E-mote 处理体 RE(主人暂缓,不抢跑) |
| 文件源优先序 | 受控松散/封包覆盖实验 + 官方 exe 观测 |
| <game-c> 剧本形态/`scenario` 资产 | 需要其完整包(本取证包未含) |

### 16.10 `LOAD/SAVE[MEM]` 形状测量(使用面最大)

555 参数表(`ysc.ybn`,opcode 54/86):

```text
LOAD: FILE, MEM, DNO, LET, ID, IDNO, G_INT..G_STR4
SAVE: FILE, MEM, DNO, SET, ID, IDNO, G_INT..G_STR4
```

5 个可读样本的静态计数:

| 样本 | LOAD 总数 | `FILE` | `MEM` | `DNO` | `LET` | SAVE 总数 | `FILE` | `MEM` | `DNO` | `SET` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| <v555-1> | 443 | 235 | 208 | 443 | 431 | 237 | 5 | 2 | 230 | 225 |
| <v555-2> | 493 | 292 | 201 | 493 | 489 | 293 | 10 | 2 | 281 | 279 |
| <v555-4>/3 | 505 | 295 | 210 | 505 | 501 | 296 | 10 | 2 | 284 | 282 |
| PC | 440 | 247 | 193 | 440 | 428 | 250 | 8 | 2 | 240 | 233 |

> `FILE + MEM = LOAD 总数`(四组精确相加:235+208=443、292+201=493、
> 295+210=505、247+193=440)⇒ **`MEM` 与 `FILE` 互斥,是“内存来源”替代“文件来源”**;
> `DNO` 每次都给,`LET` 是目标。
> `SAVE` 多数既无 `FILE` 也无 `MEM`(写当前/默认存档文件),少数显式
> `FILE`/`MEM`;目标参数名是 `SET` 而不是 `LET`。

已观测调用形状(`probe_scan_named`,按工程参数名标注;<v555-pc> 与<v555-2> 同型):

```text
LOAD[FILE="kidoku.sd", DNO=1, LET=@1168]
LOAD[FILE=$2148, DNO=@2149, LET=@2122]        ; 文件名在字符串变量里
LOAD[MEM=@2152, DNO=@2151, LET=@2122]         ; 内存槽 @2152,条目 @2151
LOAD[MEM=@x, DNO=@base+0..n, LET=@arr]        ; DNO 0 基(@base+0 实测)
SAVE[DNO=@2147, SET=@2122]                    ; 默认存档文件
SAVE[MEM=99999]                               ; 特殊分支(见下)
SAVE[MEM=@base+90000]                         ; 内存槽号可 >90000
```

> 注意:早期用 `probe_script_pc` 按窗口顺序打印时曾把 MEM 后面的值误标成
> `SET=…`;按 ysc 参数名的命名探针确认:PC/样本的 `SAVE[MEM]` **没有**
> `SET`/`DNO`,只有 `MEM` 一个窗口。

- `MEM` 是整数变量(**内存存档槽号**,不是文件名字符串);同函数内通常由
  `INT @2152 = @PINT[1]` 这类参数传入,`DNO` 为块内条目号。
- **555 处理体 RE 已闭合**(<v555-4> `LOAD=0x4447c8`、`SAVE=0x44b874`):
  - `SAVE[MEM=n]`:把当前存档容器序列化到 `DAT_008670c0[n]`(容量
    `DAT_008678a0[n]`;不足时 `realloc`);`MEM=99999` 走特殊分支
    `FUN_0044d82c`。
  - `LOAD[MEM=n]`:从同一内存槽读取,`DAT_00868860[n]` 作为读偏移;
    校验容器魔数 `0x44535359`(“YSSD”)与版本 `0x1e0`(480)——
    **与文件存档容器同格式**,只是来源换成内存。
  - 无 `MEM` 时 `SAVE` 走文件/默认存档路径,`LOAD` 走 `FILE`
    (含 `ID`+`IDNO` 拼名、`.sd` 补全)。
- 文件存档抽检(PC/样本 555):`config/global/extra/kidoku/seld/voice/save*.sd`
  头部全部是 `YSSD` + `0x1e0`(480),与内存槽容器一致 ⇒ `MEM` 分支只是
  “同一容器的内存来源”,不是新格式。
- 2026-11-19 之前内核 `op_load` 只识别 `FILE/DNO/LET/...`,`MEM` 落默认
  分支被忽略;现已实现(见 §17)。实现方式 = `yu-save` 内存槽表
  `BTreeMap<u32, Vec<u8>>`,容器字节与文件存档共用同一
  `build_container`/`decode_block` 路径(`YSSD`/0x1e0),不改游戏脚本。

---

## 17. P0 实现落点与验证(2026-11-19,代码已落地)

> 本节记录**已实现**的部分;§9 的"待批准"表述是当时的计划口径,以本节为准。
> 原则与 BETA 2/3/4 的适配范式一致:**按数据形状/参数名分派,不按版本号猜**。

### 17.1 与 BETA 2/3/4 既有范式的对应

| 既有范式(BETA 2/3/4) | 落点 | v555 的沿用 |
|---|---|---|
| YSTB 版本带 V2 `200<v<300` / V5 `>=300` | `yu-format/src/ystb.rs` | 555 是 V5,四区段/异或/布局不变;不新增版本分支 |
| YSCF/YSVR 候选布局按"宽高 + 标题 + 文件尾精确闭合"裁决 | `yscf.rs`/`ysvr.rs` | 555 的 YSCF 仍按候选闭合裁决;YSVR 前缀 ABI 指纹匹配 `0495` 表 |
| 命令按**工程 ysc 名字**分派,opcode 不硬编码 | `vm/handlers.rs::build_handler_table` | TEXT/LOAD/SAVE/PRODUCT 全部按名字注册 |
| CSV 按**参数表结构**(`LINE` 是否存在)区分 0.466/0.479 两代 | `build_handler_table` 的 `"CSV"` 分支 | TEXT 按 `FILE` 参数、LOAD/SAVE 按 `MEM` 参数区分 555 形态 |
| 系统变量 ABI 按 `ysv` 前缀指纹选表,不按版本号 | `sysvar_abi.rs` | 555 的 0..547 与 0.495 逐项一致,继续用 `0495` 表;实测 TSV 仅作证据 |

开源工具复核(2026-11-19,本地只读):

- `shimamura-sakura/yuris_decompiler`(MIT,0.247–0.494)、
  `Aionfatedio/YU-RIS-Script-Editor`(V2/V5)、
  1F1E33-float32/Tools(`>455` 的 ysvr/特殊命令):**都只覆盖到 V5 结构层**,
  没有 v555 的 `TEXT[FILE]`/`LOAD[MEM]`/`PRODUCT` 运行期语义;
  555 的这三项语义只能以本仓库对商业 exe 的 RE 为一手证据。
- 三者的 V2/V5 分带(`200<v<300`)与命令表解析流程,与我们的既有实现一致,
  未发现需要修正的格式假设。

### 17.2 实现清单

| 能力 | 代码 | 语义/证据 | 测试 |
|---|---|---|---|
| `SAVE[MEM=n]` | `yu-save/src/state.rs::save_mem`;`yu-rt/src/vm/handlers.rs::op_save` | REA <v555-4> `0x44b874`:内存槽 `DAT_008670c0[n]`,容器与文件存档同型;`MEM` 优先于 `FILE` | `yu-save::state::tests::mem_save_roundtrips_container_without_disk`;`fixture::save_and_load_mem_use_memory_slots_not_files` |
| `LOAD[MEM=n]` | `state.rs::load_block_mem/load_image_mem`;`op_load` | REA `0x4447c8`:同槽 YSSD/0x1e0;DNO 0 基(实测 `@base+0`) | 同上 + 文件/内存共用 `decode_block` |
| `TEXT[FILE, LET, CLEAR]` | `handlers.rs::op_text_file`、`text_file_lines` | REA <v555-4> `0x452a0c`:ASCII 小写化、`/`→`\`、不补扩展名;追加 CRLF 后按 CRLF/CR/LF 切行,每行加 `0x0D` 写字符串数组,末尾两个空元素 | `handlers::text_file_tests`(4 条);`fixture::text_555_file_fills_let_string_array` |
| `PRODUCT` 最小面 | `handlers.rs::op_product` | REA `0x459670`:`COM=29` 写 0 到 LET;`21/24` 按 VFS 目录枚举填充 `_DROPFILE*`;`51/52/80/81/100/110` 同型 no-op;其余已知 Win32 分支逐 COM 挂号一次;官方落空 COM 同型 no-op | `fixture::product_com29_writes_mode_bit_zero_to_let`、`fixture::product_com21_enumerates_vfs_directory_into_dropfiles` |
| `_DROPFILE*` | `sysvar.rs::read` + `vm.rs::enumerate_drop_files` | `PRODUCT[COM=21/24]` 在已挂载 VFS 上枚举 `PSTR` 目录;无枚举时为空表 | 同上 |
| `PRODUCT[COM=23]` | `handlers.rs::op_product` | 返回最近一次 `TEXT[FILE]` 的行数(555 label lexer 的逐行上界;工作假设,已由运行自证:66 份文件后 COM 22 被调用 8,887 次) | 运行日志 |
| `VARINFO[STRFIRST]` | `handlers.rs::op_varinfo` | 在 `SET` 字符串数组的 `[NO,NO2]` 闭区间找字典序最小元素,返回下标(REA `0x455200`) | `fixture::varinfo_strfirst_returns_smallest_index_case_insensitively` |
| `VARINFO[SJISCODE]` | `handlers.rs::op_varinfo` | v555 `0x455200` 的 `0x4554a9` 分支:`SET` 首字符 → 单字节原值 / lead byte `(lead<<8)|trail`(`0x455b9c`);555 剧本词法器的字符分类依赖它 | `fixture::varinfo_sjiscode_returns_first_char_code` |
| 555 剧本 DSL 词法器码表 | `scenario.rs::tokenize` | 消费方 `es_scenario_parser.yst` 的实测契约:标识符/数字 = **0x20**,`\`=0x33、`(`=0x34、`)`=0x35、`,`=0x36、`@`=0x37、`$`=0x38、`#`=0x39(与 `es_scenario_lexer.yst` 产出逐 token 对齐;`0x5e/0x5f/0x60` 只是词法器内部状态) | `scenario::tests` 5 条 |
| `GOSUB/RETURN` 三组参数 | `vm.rs::GosubFrame`、`handlers.rs::op_gosub/op_return`、`sysvar.rs` | 手册 `control/gosub.html`:同编号 PINT/PFLT/PSTR 各自落 `@_PINT/@_PFLT/$_PSTR`;引擎三组数组 `+0x10/+0x98/+0x120`。修复前单一 `args[16]` 让 v555 的 `PSTR` 命令名被 `PINT` 覆盖 | `fixture::gosub_keeps_pint_and_pstr_groups_apart`;档案 `DIV-RT-062` |
| 555 文本状态初值 | `text.rs::TextState::default` | 恢复 `output_mode=1`(0.495 `FUN_0045fcb4` 同源);555 的 TEXT 表无 `OUTPUTMODE` 不等于引擎初值为 0 | `text::tests` 18/18 |

### 17.3 验证命令与结果

```bash
# 结构/语义单测
cargo test -p yu-save                       # 23 passed(含官方 seld/缩略图 golden)
cargo test -p yu-rt --lib text::tests       # 18 passed
cargo test -p yu-rt --test fixture          # 132 passed

# 555 真机首帧(<v555-4>,GBK 补丁,400 帧)
yu-cli run "<v555-2>" --mode headless --frames 400 --patch gbk \
  --save-root "<tmp>/saves" --log-level debug
# 结果:exit 0;rt.cmd.unimplemented / rt.sysvar.unimplemented / FATAL = 0;
#       仅 1 条 rt.sysvar.write(既有 0.495 缺口,非 555 新增)
```

### 17.4 仍未闭合(显式,不猜)

1. `MEM=99999` 特殊分支 `FUN_0044d82c`:RE 已到"把 0..499 号槽打包到
   `DAT_0086a03c`,头 = 0,1,u16 0,u32 0,u32 总长,逐槽 = 原始长/压缩长/
   数据";**消费方未定位**,按 `report_unimplemented_variant` 每变体一次。
2. `PRODUCT[COM=22]` 的**输出契约已闭合**(2026-11-19):标识符/数字 token
   码 = 0x20,`#` = 0x39,`\`/`(`/`)`/`,`/`@`/`$` = 0x33..0x38(与
   `es_scenario_lexer.yst` 逐 token 对齐)。修正后 `es_scenario_parser.yst`
   的标签表可建(`$2878` 实测 106 条,含 `SCENARIO_MAIN`/`START_RELEASE`/
   `TITLE`),`es.Scenario.JumpLabel` 能把起始行设到标签行。
   剩余:COM=22 的 token 位置/长度输出尚未逐字段对拍;目前只消费文本 +
   类型码,未发现调用方需要更多字段。
3. `VARINFO[STRFIRST/NO2]`:已实现 `STRFIRST`(区间最小串下标,REA
   `0x455200`);`NO2` 作为结束下标从属参数。完整比较方向由语料
   `NO=1,NO2=2` + 调用方判 1/2 自证。
4. `PRODUCT` 的其它文件/对话框/注册表分支(`COM=26/33/34/36/37/38/
   10201/10205/21000/21031/0x2776..79`)仍逐 COM 挂账。
5. `CGACT[GRADATION]`:`yu-gfx` 已有 `GRADATION` 处理(REA 0x444713/0x4447db),
   555 语料 1 组,未另开分支。
6. 文件源优先序(§16.6)仍未裁决;`TEXT[FILE]` 复用内核既有源序,
   未按 555 单方翻转。
7. **画面层**:headless 探针已不再停在黑屏/`END`;修正 GOSUB 参数组后
   剧本链实际执行 `es.ScenarioMacro.CSEL`(s70 pc917),主任务停在
   `es_select2.yst` 的标题选择循环。官方同机实测启动序为
   「发行商 logo(点击)→ 标题画面(START/LOAD/EXTRA/CONFIG/EXIT)」,
   与内核当前阶段一致;探针无图形宿主故 `text: []`,标题由 CG 层渲染,
   不是文本链路残留。下一步是桌面宿主的标题画面逐帧对拍(非 headless)。
