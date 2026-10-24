# 任务书 B1 · 存档闭环(DIV-RT-042:变量引用身份契约)

> **状态:完成(2026-10-24)**。三契约点按裁决执行(裁 1=A / 裁 2=A /
> 裁 3=取证定论:es 脚本自 LOAD,零内核新机制)。取证勘误:SAVE 处理体 =
> `0x44c45c`(`0x44e1c8` 实为 SAVEINFO 本体,SAVEINFO 挂账 B7)。
> 台账七项已划(DIV-RT-042、coverage-matrix、todo-inventory、symptom S4、
> audit P0-1、本头、PROGRESS 成果 B1)。

> 仓库:`<repo>`。动手前先读:`AGENTS.md`、本文件、
> `docs/audit-2026-10-24.md` §1(P0-1)、`docs/divergences/DIV-RT-042` 相关条目
> (`crates/yu-rt/docs/divergences/README.md` 表行)、
> `crates/yu-rt/docs/coverage-matrix.md` §2 SAVE/LOAD 行、
> `docs/symptom-ledger.md` §S4(销账注记里的 `sv.write.refused` 残留)、
> `docs/tasks/task-p4-save-domain.md`(yu-save 交接注记 H-P4-01/H-P4-02)。
>
> 背景:内核主链路(boot→标题→本篇→对话→按钮)P1–P8 后已通;**存档是唯一
> 功能级 P0**。`SAVE[FILE]` 刷盘被 `yu-save` 硬门禁拒绝、`LOAD[LET]` 不恢复
> 任何变量、既读色(`seld.sd`)不通——根因全部是同一个契约缺口:域事件层
> 丢失变量引用身份(DIV-RT-042,blocked)。YSSD 容器层已逐字节 roundtrip
> 闭环,**只差"块内容从哪来"**。
>
> 文件占用:
> - `crates/yu-rt`(event.rs / vm.rs / vm/handlers.rs);
> - `crates/yu-trace`(record.rs 穷举匹配);
> - `crates/yu-save`(state.rs 消费侧);
> - `crates/yu-cli`(engine.rs 接线 + 用例);
> - **不碰**:`yu-gfx` / `yu-render` / `yu-font` / `yu-sound`(SOUNDINFO[LET]
>   只随契约字段顺带解,不动其状态机)/ DIV-RT-050 相关代码。
> - 零 git 写操作;每处改动主人审阅。

## 0. 契约裁决点(动手前主人拍板;不拍不动工)

**裁 1:引用身份怎么进事件?**

| 方案 | 内容 | 代价 | 评价 |
|---|---|---|---|
| **A(推荐)** | `ArgValue` 加 `reference: Option<VarRef>` 字段;`VarRef { space, id, indices }`。`eval_window` 对**引用型参数槽**(SAVE[SET]/LOAD[LET]/FILEINFO[LET]/SOUNDINFO[LET])在求值时保留 lvalue 身份 | 事件结构体加一字段;yu-trace 序列化/解析同步;旧 trace 向后兼容(缺字段=None) | 单点变更,四条命令共用;与现有 `slot`/`value` 并列,语义清晰 |
| B | 新增独立事件变体 `DomainRefCall` 与 `DomainCall` 并行发送 | 事件流膨胀一倍(该四条命令);消费侧要去重配对 | 不推荐 |
| C | 维持只传值 | 即现状,写不出忠实存档 | 违反 1:1 目标,否决 |

**裁 2:LOAD 的值写回通道?**

| 方案 | 内容 | 评价 |
|---|---|---|
| **A(推荐)** | `SaveHost` 新增同步方法(如 `load_vars(&mut self, dno) -> Vec<(VarRef, Value)>`),VM 在 LOAD[LET] 处理点同步拉取后写变量空间——与 `GraphicsHost::query` 同模式 | 与既有宿主查询面一致;无新事件 |
| B | SAVE 域事件新增 `Loaded` 变体异步写回 | 多一跳时序,官方是命令处理体内联完成,同步更贴近 |

**裁 3:启动既读色**(`seld.sd` → `@es.SELD`,P7 续已取证块值 `[1]=6`):
boot 链在 `es_First` 之前由宿主/内核谁发起 LOAD?——按官方启动链 REA 取证后定,
若取证不到就按"es 脚本自己 LOAD"处理(es_logset 族),不新增内核机制。

## 1. 取证步骤(动手前,顺序执行)

1. **REA 锚定官方 SAVE 写盘路径**:0.495 exe `SAVE` 处理体(`0x44e1c8` 一带,
   SAVEINFO 同族)——确认 `SET=` 参数取变量**地址**(引用)还是值;确认块编码
   前变量如何枚举(与 `yu-format/src/yssd.rs` 现有 roundtrip 的块布局对照)。
   产出:旁线笔记 + 本档案更新。
2. **官方 golden 存档**:用官方引擎(或既有官方 .sd 样本)取一份真实存档字节,
   作为字节级对照目标(真值层级 3:原版数据自证)。已有官方 5 文件 269 块
   roundtrip 样本可复用(`yu-format/tests/yssd_roundtrip.rs`)。
3. **语料形态清点**:`es_logset.yst` / `es_number.yst` / `es.yst` 里 SAVE/LOAD/
   FILEINFO/SOUNDINFO 的引用参数槽清单(SET/LET 出现次数、字面量 vs 变量),
   用 `yu-rt/examples/dump_label_cmds.rs` 或 scanmath 同款工具。这决定
   `VarRef` 需要覆盖的形态(标量/带下标数组/字符串变量)。

## 2. 实施分解(每步可独立编译、独立审阅)

1. **yu-rt · 事件层**:`event.rs` `ArgValue` 加 `reference: Option<VarRef>`
   (裁 1-A);`vm.rs`/`handlers.rs` 的 `eval_window` 在 lvalue 求值路径保留
   身份;只有 SAVE/LOAD/FILEINFO/SOUNDINFO 四命令的指定槽携带(先在第 1 步
   取证里确认官方是否仅此四条——是则硬编码白名单,不是则扩大)。
2. **yu-trace**:record.rs 穷举 match 补字段序列化/解析(编译期守卫会强制
   全量 match);旧 trace 文件缺字段按 None 读。
3. **yu-save · 写侧**:`ReservedFact` 带引用;`SAVE[FILE]` 按 REA 确认的块
   布局取当前变量值编码进 YSSD(用已闭环的 `YssdWriter`);移除
   `sv.write.refused` 门禁,替换为"引用缺席才拒"(防半忠实写盘)。
4. **yu-save · 读侧**:`LOAD[FILE]` 解容器 → 裁 2-A 同步接口把值交 VM 写回
   (int/str/旗标三类;YSSD 配置块 btype 1 段内语义未逆向,只恢复变量块,
   配置块保持结构保真不解释)。
5. **既读色**:按裁 3 结论接 `seld.sd` 启动读取;es.SELD 恢复后点过的选项
   变色回归。
6. **yu-cli**:新用例 `cases/save-load-loop.json`(进本篇→存→读→变量/像素
   对拍)+ `fatal_codes` 断言;engine.rs 若需 SaveHost 接口变化则同步。

## 3. 验收标准(全过才算完成)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
export YURIS_SAMPLE_ERIS_ROOT="D:/Art/ai/projects/YurisDroid/workbench/project-eris"
export YURIS_SAMPLE_ERIS_YSBIN="$YURIS_SAMPLE_ERIS_ROOT/save/data/ysbin"
cargo test -p yu-rt -p yu-trace -p yu-save -p yu-format
cargo run -p yu-cli -- case --all   # 含新 save-load-loop,全绿
```

1. 新回归:SAVE 预约→刷盘→容器字节(结构级断言 + 可选官方 golden 对拍);
   LOAD 恢复 int/str/旗标各一。
2. 正常路径 `sv.write.refused` 零出现(诊断统计 grep)。
3. 用例 `save-load-loop.json` 全绿且 `fatal_codes=[]`。
4. 主人桌面实测:存档→读档→继续推进;点过的选项变色(既读色)。
5. 旧 8 条 case 无回归。

## 4. 明确不做(防蔓延)

SAVEINFO 查询(REA `0x44e1c8` 单独挂账)、存档缩略图/图层快照/时间戳列表
(H-P4-02,批次 B7)、FILEACT BMP/PNG/JPG 导出、YSSD 配置块(btype 1)段内
语义、DIV-RT-050、ACM/声音anything。

## 5. 强制收尾(完成定义的一部分;**不做不算完成**)

> 这是主人点名的纪律:**修完必须把台账划掉**,防止"明明修复了却说没修复"。

1. `crates/yu-rt/docs/divergences/README.md`:DIV-RT-042 行 `blocked` →
   `fixed`(影响列写清方案与回归)。
2. `crates/yu-rt/docs/coverage-matrix.md`:§2 `SAVE 16`/`SAVEEND` 行与 §8
   (若涉 SOUNDINFO[LET])补"存档闭环落地"口径;§1 若有对应行同步。
3. `docs/todo-inventory.md`:§5 如有存档行、§3.2 相应条目更新;头部快照注记追加 B1。
4. `docs/symptom-ledger.md`:S4 销账注记里的「`sv.write.refused` 仍 open 归
   DIV-RT-042」句更新为已解。
5. `docs/audit-2026-10-24.md` §1 P0-1 行标注 ✅(B1)。
6. 本任务书头部加「状态:完成(日期)」一行。
7. `PROGRESS.md` 追加「成果 B1」:验证命令 + 原始输出(测试计数、case 结果、
   桌面实测记录)。
8. 若取证推翻本任务书任何假设(如官方不止四条命令用引用参数、块布局与
   roundtrip 不符):**先改本档案与 DIV-RT-042 条目再继续**,禁止按错误前提写码。

## 6. 禁止事项

- 未取证不猜块布局/变量枚举序(真值层级:官方 trace > REA 事实 > 原版 .sd 自证)。
- 不写"半忠实"存档:引用身份缺失时保持拒绝并挂号(现状语义),不落 `Int(0)` 兜底。
- 零 git 写操作;证据产物只放 `target/tmp`;`src/` 无 stdout 宏。
- `debug_mode`、DIV-RT-050、宿主呈现一律不顺手改(归 B2/B3/B4)。
