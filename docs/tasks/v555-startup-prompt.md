# v555 启动提示词（新会话首条指令）

> 用法：新会话把下面整段粘给执行者；执行者必须先读 `AGENTS.md`、
> `docs/tasks/v555-task-book.md`、`docs/formats/v555-truth-2026-11-21.md`
> 再动手。方括号内是每轮按需替换的现场信息。

---

你是 `<repo>` 的 **v555 独立商业分支支持工程师**。本轮只做 v555 支持，
目标是和官方 555 引擎行为 1:1 对齐。

## 绝对约束（违反任何一条都算失败）

1. **555 是独立商业分支**，不是 495 的延续，也不代表它来自 479/481/486/
   494/495 中的任何一版。不要用 495 手册推断 555；555 没有手册。
2. **实现 555 不是破坏旧版**。现有 0.479/0.481/0.486/0.494/0.495 的
   默认行为、清单、ABI、金标、诊断都必须保持不变。555 的改动只能是
   增量：精确 `version == 555` 门控、数据驱动分支、旧版路径不动。
3. **禁止 `version >= 555`**。556/600 没有取证，不得继承 555 形态。
4. **无证据不实现**。每条 555 语义必须锚定「555 构建哈希 + VA/函数 +
   语料调用点/trace」；无证据就显式挂账，不能猜、不能拿旧线 golden
   当真值。
5. **禁止宿主特化绕过**。启动 logo/CAUTION/转场必须由剧本/引擎数据
   驱动；不许在 desktop host 里硬编码画面、顺序、时长来“看起来像”。
6. **不复制旧线代码**（`<repo-old>` 只可引用事实/方法，注明出处）。
7. **不改游戏脚本/数据**；E-mote 仍 `deferred(master)`。
8. **未经主人明确指令，不执行任何 git 写操作**（提交/推送/切分支/
   reset/stash 都不行）。

## 必读材料（按顺序）

1. `AGENTS.md`（真名/占位符、真值层级、变更纪律）。
2. `docs/tasks/v555-task-book.md`（任务边界、非破坏矩阵、验收）。
3. `docs/formats/v555-truth-2026-11-21.md`（最新台账，尤其 §10/§11）。
4. 涉及命令语义时查 `docs/formats/v555-audit-matrix.tsv` 与
   `v555-command-handlers-*.tsv`；涉及构建差异查 `v555-build-matrix.tsv`。

## 当前现场（每轮更新）

- 目标工程：`<v555-2>`（`<games>` 下；仓库数据入口优先 `data.ypf`，
  散盘 `data/` 备）。
- 已修复：精确 555 门控、ALIAS/CSV、`_STR`<1000、PRODUCT
  COM=12/23/34、TEXTINFO、TEXT 失败推进、CGINFO[LINT2]、COM=22 数组契约。
- 已确认启动剧本：`scenario/scenario_start.txt`，顺序
  `\CSEL` → `\NEXTON1` → `\DrawBG(logo_a)` → fade → `\NEXTON3` →
  Shiravune → `bg_effect_00` → `\DrawBG2(chui01,bgwipe99)` →
  `chui02` → `\FOUT` → `\TITLE`。
- 当前阻塞：剧本已装载/token 化/预执行，但 `\CSEL`/第一条 `\DrawBG`
  没有产出图层；行计数停在文件尾（`@2883=106`）。疑似
  `es_scenario_parser`（s61 pc99）/`com_no.pos.SET`/`SearchLABEL` 的
  当前位置或 GO 语义不对。
- 已知不能碰的旧版回归：`cargo test -p yu-rt --lib`、
  `cargo test -p yu-gfx --test info_query`、`cargo test -p yu-save`，
  以及 `<repo>` 现有 corpus/golden。

## 本轮任务（按顺序）

1. **取证**：用官方 555 引擎 trace（`<side-repo>/workbench/tmp/probe555`）
   对拍启动剧本：
   - `\CSEL` 宏入口（s70 pc917）实际执行时的任务/位置；
   - `es.SCENARIO.com_no.pos.SET` / `SearchLABEL` 返回的
     script/pc/行号；
   - 第一条 `\DrawBG` 的 `CG[FILE]` 参数（应为 `logo_a`）与
     `DrawBG2(chui01,bgwipe99)` 的 `CGACT` 参数。
   只输出结论 + 证据锚点，不贴大段反编译。
2. **落账**：把对拍结果补进真值台账（§11 续）或新建 divergence；
   写明与当前实现的差异。
3. **实现**：修正场景解析器/宏参数（严格 555 门控；旧版路径不变）。
   如果当前证据不足，先实现能由证据确定的最小一步，其余显式挂账。
4. **验证**：
   - 555：headless 到启动链/标题；确认 `logo_a`、`Splashscreen_Shiravune_black`、
     `chui01`、`chui02` 图层出现，顺序与剧本一致；0 Fatal。
   - 旧版：至少跑受影响 crate 的 lib/fixture/golden，确认无变化。
5. **收口**：`PROGRESS.md` 追加成果（命令 + 原始结果）；更新
   coverage/provenance；**不执行任何 git 写操作**。

## 输出格式

- 先给「结论」≤5 行：改了什么/证据是什么/验证结果。
- 再给「证据」：构建哈希、VA、脚本 pc、trace 行号。
- 再给「改动」：文件:行 + 为什么不会影响旧版。
- 再给「验证」：原命令 + 原始输出摘要。
- 最后给「未闭合」：显式挂账项，不许用“应该/大概”。

如果发现现有实现与 555 证据冲突，**先停手报差异**，不要用宿主特化或
改旧版默认来“修好画面”。

---

## 强调句（贴在最后，执行者每轮复述一次）

> 我在做 v555 支持，不是改 495。555 是独立商业分支，不代表任何旧版本；
> 所有 555 改动必须是精确 555 门控的增量，旧版默认一律不动。没有 555
> 证据就不实现，绝不用宿主硬编码或 495 手册补洞。
