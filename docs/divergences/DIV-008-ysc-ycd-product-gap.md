# DIV-008 · ysc/ycd 命令名集合差:COM_PRODUCT 只在编译器语言表中

- 状态: **explained**(版本差异证据存档;内核如实装载各自文件)
- 优先级: 中
- 发现日期 / 发现方式: 2026-10-19,任务书 B "ysc 名字表三方互证"
  (ysc.ybn ↔ YSCom.ycd ↔ 手册 menu.html)自动比对发现。

## 实测(官方 0.495 同盘一手数据)

| 集合 | 数量 | 说明 |
|---|---|---|
| `ysc.ybn` 命令 | 119 | 顺序 = opcode(字母序) |
| `YSCom.ycd` 的 `COM_*` | 105 | 即 ERROR 命令的 105 个 `COM_*` 参数 |
| ycd COM − ysc | **1**:`PRODUCT` | 编译器语言表有、引擎命令表无 |
| ysc − ycd COM | **15**:`);` `CDDA` `COMPILEMODE` `EMOTE` `EMOTEINFO` `F_FLT` `F_INT` `F_STR` `F_VAR` `MATH3D` `MIDIEND` `MIDIINFO` `PROJECTFOLDER` `RELEASEMODE` `SYSTEMMODE` | 编译器指令(`;`/`_IFDEF` 族之外的设置项)、无错误级参数的 0.495 命令 |
| 手册 menu.html 文档化命令 | 71 | 全部 ⊆ ysc ✓(含 IF 页共挂的 ELSE/IFEND/IFBREAK/IFCONTINUE) |

- 结论 1:**ysc 命令表 ⊉ ycd 的 COM_* 全集**,跨版本对表必须以
  ysc.ybn(引擎实际装载的表)为准;ycd 是编译器侧语言定义,两表
  不同步是官方常态。
- 结论 2:ERROR 命令的参数表 = "可设错误等级的命令"名单,恰好 105 条
  (107 参数 = `NO` + `SET` + 105 个 `COM_*`);`CDDA`/`EMOTE` 等
  0.495 新命令不在其中 —— ERROR 参数表随版本演化,不能当命令全集用。
- 结论 3:`PRODUCT`(制品名登记类命令)在 0.495 引擎 ysc 表中缺席,
  说明其执行面未随编译器语言表同步,或该命令属"仅编译期"条目;
  待遇商业 v55x 语料后复核(若 v55x 的 ysc 含 PRODUCT,则 0.495 为
  版本差;若仍缺,则为编译器单方面条目)。

## 回归

- `cargo test -p yu-format --test corpus_eris`(需 `YURIS_SAMPLE_ERIS_YSBIN`
  与 `YURIS_SAMPLE_SDK0495`):三方互证断言(105 COM / 差集钉死 / 手册 71 ⊆ ysc)
  全部通过;原始输出见 `PROGRESS.md` 成果 B2。
