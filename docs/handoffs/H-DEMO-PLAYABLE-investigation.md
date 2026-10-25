# 交接:ERIS demo「剧情无法推进 / 无法结束 / 回退失效」调查记录

- 日期:2026-10-25
- 状态:**调查中途交接**。未完成修复,已落地的 3 处改动见 §6,其余为证据与判断。
- 语料:`D:\Art\ai\projects\YurisDroid\workbench\project-eris`(data.ypf,官方 0.495 全量编译)
- 断言口径:✅ = 有实测/反汇编证据;⚠️ = 我的判断但未闭环;❌ = 已排除

---

## 1. 一句话结论

**VM 层的剧情推进其实一直是好的**(文本逐句累积、任务逐节前进,§3 有原始数据);
问题出在 **VM → GfxState 的文本呈现与清屏链路**,以及**几处挂账的未实现命令/系统变量**
把官方文本泵该做的事情跳掉了。**不是按钮点击不命中** —— 用户能进菜单、能点按钮,
说明输入链路正常,这一条我判断错过一次,已勘误。

---

## 2. 复现方式(两种都要)

### 2.1 桌面(用户实测,最接近真实)

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
./target/release/yu-cli.exe run "<project-eris>" --mode desktop \
    --log-level info --log-file target/tmp/desk.jsonl \
    --frame-stats target/tmp/frames.csv
```

路径:标题 → `スタート` → `β4機能紹介` → `画像表示関連機能` → `フェイスウィンドウサンプル`。

### 2.2 Headless 直连标签(取证用,绕开菜单)

我为此修了 `BootConfig::entry_label` 的非 ASCII 支持(见 §6.1),现在可以直接:

```bash
export YURIS_ENTRY_LABEL="フェイスウィンドウサンプル"
cargo run -p yu-rt --example text_scenario -- 400 20 2 6
cargo run -p yu-backend-soft --example text_probe -- 200
```

**这条很重要**:直连标签时点击**能推进**(§3.1),说明文本泵本身没坏。

---

## 3. 实测证据

### 3.1 直连 `フェイスウィンドウサンプル` + 周期点击 → 9 行全部推进 ✅

```text
$ YURIS_ENTRY_LABEL="フェイスウィンドウサンプル" \
  cargo run -q -p yu-rt --example text_scenario -- 400 20 2 10
first_text=Some(1) first_returncode=Some(1) text_events=9 text_bytes=495 returncodes=7
hits: WORD=9 _=1 RETURNCODE=7
text @1:  フェイスウィンドウサンプルです。
text @20: 表示しました。（文字見づらくてすみません；）
text @40: 今回のβ４から、フェイスウィンドウのＺ値を…
text @60: また、フェイスウインンドウの後ろに背景画像を…
text @80: 背景画像の指定は、メイン画面設計で、「
text @100:背景画像もフェイスウィンドウに合わせて自動的に…
text @120:フェイスウィンドウの読み込み方法は以上です。
```

⇒ **VM 的文本状态机、RETURNCODE 让出/恢复、任务调度都是通的。**

### 3.2 桌面实跑:脚本逐节前进,但画面停在第 1 句 ✅

带逐帧追踪跑了 3161 帧(`YU_DEBUG_TEXT=1` 环境变量,追踪代码已撤回)。
`s136` = `jp_0054.txt` = フェイスウィンドウサンプル。

```text
f1054  bytes=32   s136 pc=3     ← 第 1 句
f1096  bytes=76   s136 pc=6     ← 第 2 句
f1134  bytes=170  s136 pc=8     ← 第 3 句
f1167  bytes=244  s136 pc=11
f1191  bytes=379  s136 pc=16
f1217  bytes=451  s136 pc=19
f1240  bytes=495  s136 pc=21
f1679  bytes=515  s136 pc=57
f1892  bytes=561  s136 pc=60
f1916  bytes=609  s136 pc=62
f2132  bytes=671  s136 pc=65
f2159  bytes=707  s136 pc=67    ← 9 句走完,进入 FOUT/FIN → GO
```

原始日志:`target/tmp/desk2.jsonl`(逐帧 `yu-cli::debug` 行)、`target/tmp/desk2-out.txt`。

**关键**:`bytes` 单调增长到 707(全部 9 句)。脚本**确实在推进**。
但用户看到的画面始终是第一句 —— 所以用户口径的「循环」「不能推进」= **呈现层没跟着走**。

### 3.3 每句都在「暂停→恢复」,不是死锁 ✅

```text
f1134  s136 pc=8   paused=False   (mousel=3,按下沿)
f1135  s136 pc=8   paused=True    (mousel=4)
f1167  s136 pc=11  paused=False
f1168  s136 pc=11  paused=True
```

`es.ScenarioScript` 在每句末尾被 `es.TXTASK.TX.E0` 暂停(`TASK[E=0]`),
下次点击时 `TX.E1` 恢复。**这是设计行为**(es_Text.yst:355-378),不是 bug。

---

## 4. 已定位的具体缺口(按证据强度排序)

### 4.1 `$_CHR(n)` 未实现 → 文本控制符全部识别失败 ✅(证据最强)

- 位置:`crates/yu-rt/src/sysvar.rs` 落到 `undoc()` 的兜底分支。
- 调用量:桌面会话 **46696 次**(`rt.sysvar.unimplemented ... es_text.yst:SYSTEM@1203`)。
- 用法(`es_Text.yst:878/882/886/2199/2243/2279`):

```text
IF[$one==$_CHR(0xEF)+$_CHR(0xF3)] //C
IF[$one==$_CHR(0xEF)+$_CHR(0xF0)] //R
IF[$one==$_CHR(0xEF)+$_CHR(0xF2)] //P
```

- **语义已定**:编译器把剧本里的 `\R` 展开成 `$_TEXT(239)+$_TEXT(240)`,
  即文本流里的原始字节 `0xEF 0xF0`。所以 `$_CHR(n)` 必须返回**单字节 n**,
  不能按 Unicode 码位再编码成 CP932(那样 0xEF 会变两字节,比较恒不成立)。
- 失败后果:返回 `Err` ⇒ 该表达式取不到值 ⇒ `$one` 保持上一轮值/空串,
  三个 `IF` 全部走 `ELSE[]` 分支 ⇒ `CRPREPLACE` 的 C/R/P 替换、
  `BUFSET.SPD` 的控制符跳过、`BUFSET.XY` 的禁则处理**全部失效**。
- 手册:`_CHR` 在 SDK 0.495 手册全库 **0 命中**(已扫全部 html/txt)。
  按项目政策(AGENTS.md §0)属 `fixed(no-manual)`,标注 REA/语料锚点即可实现。

### 4.2 `TASK[RESTART=1]` 未实现 ✅(REA 证据完整)

- 位置:`crates/yu-rt/src/vm/handlers.rs` `op_task` 的 `other =>` 分支直接返回
  `Unimplemented`。
- 语料用法:`es_Text.yst:1545`,在 `es.TX.OUTPUT.Z`(行末 2 = 一句文本彻底结束)里:

```text
@TX.POS = 1
@TX.TASK.POS = 1
...
GOSUB[#=es.TX.TIMEINIT]
TASK[RESTART=1]
```

- **官方语义(REA 0.495 `TASK` 处理体 `0x4506f4`)**:槽 37 的存在标志
  `[0x684185]` 被读于 `0x450c46`;存在 ⇒

```text
0x450c55  mov dword ptr [0x801944], 1
0x450d05  call 0x42f7bc            ; 按 Z 重排任务链
```

  `[0x801944]` 就是内核 `Scheduler::extra_frame` 的同名量
  (`0x40413d` 在逻辑帧开头清零)。
- ⇒ 语义 = **本 vsync 再跑一个逻辑帧**。
- 手册 `control/task.html` 只列 ID/E/Z/#/SCRIPTPOS,**无 RESTART 页** ⇒ `fixed(no-manual)`。
- 影响:`TASK` 命令整条返回 Unimplemented ⇒ **该命令之后的效果全部被跳过**,
  包括紧邻的收尾。实测该诊断在桌面上出现 577 次
  (`rt.cmd.unimplemented ... ystdlib.yst:SYSTEM@378 TASKINFO` 附近)。

### 4.3 `clear_pending`(引擎 `+0x36`)是死字段 —— 写入无人消费 ✅

- `op_returncode` 手动模式置 `self.text.clear_pending = true`(`handlers.rs:3660`),
  但**全仓没有任何读取点**(`grep -rn clear_pending` 只有定义/赋值/快照转发)。
- 官方同型:REA `0x44c437 mov byte ptr [eax+0x36], 1`(RETURNCODE 手动分支),
  但我在 `WORD`(`0x458d44`)与 `RETURNCODE`(`0x44c390`)处理体内**没扫到**
  `+0x36`/`+0x37` 的读取。⚠️ 消费点尚未定位 —— 可能在别处(文本泵主循环
  `0x404935` 一段读 `+0x37`,见下)。
- 官方文本泵 `0x404935` 附近有:

```text
0x40493e  movsx esi, byte ptr [edx + 0x37]
0x404942  test  esi, esi
0x404944  je    0x40495e
0x404946  mov   byte ptr [edx + 0x37], 0
```

  ⇒ `+0x37` 是「清屏已完成」的一次性标志。⚠️ `+0x36`(请求)→ `+0x37`(已清)
  的转换点仍需定位。**这是下一位接手最该先做的事**。

### 4.4 `RECTPAINT3` 对 Snapshot 目标的支持 ✅(precheck 已放行,但值得复核)

- 清屏路径:`es.Mes.Clear` → `\BT.CGCLEAR("ES.GAMEMAIN.TIP.MESW.TX")` →
  `CGACT[ID="BT.OFF"+$tmp RECTPAINT3=1 SET=0]`(`es_Button.yst:4555`)。
- `crates/yu-gfx/src/state.rs:3010 rect_paint_precheck` 的放行集合是
  `Asset | SolidBlack1x1 | Snapshot`,**已含 Snapshot**。
- 实测 `:/W0/BT.OFFES.GAMEMAIN.TIP.MESW.TX=0=1` 的 texture 是
  `{"kind":"asset","path":"cgsys/main/tip_meswindow_txspace"}`、
  `status.executable = true` ⇒ 清屏**应该**能执行。⚠️ 但需实测确认。

### 4.5 我撤掉的一个改动(重要,别重复踩)

我曾给 `Engine::tick_inner` 加过「每帧 `sync_text_state(&vm.text_state())`」。
结果是**多出一个画在左上角 (0,0) 的文本层**,因为:

- `GfxState` 有两套文本通道:
  1. `text_layers` / `frame.text_layout` —— 自动文本层(本内核的 `WORD`/`_` 通路);
  2. `text_draws` —— `CGACT[TEXT]` 逐字画进**目标图层**(DIV-GFX-058,官方写表面语义)。
- ERIS 的对话正文**走第 2 条**(`es_Button.yst` 里 `CGACT[TEXT]` 逐字画到
  `:/W0/BT.OFFES.GAMEMAIN.TIP.MESW.TX=0=1`),实测该层 `text_draws` 有 80 项。
- 我加的同步同时激活了第 1 条(origin 恒 (0,0)),造成叠字。**已撤回**。
  ⇒ 若要修呈现,必须在**第 2 条**上做,不要动 `sync_text_state`。

`target/tmp/full950.json` 里有 `text_draws` 的完整样本可参考。

---

## 5. 已排除(不要重复调查)

| 假设 | 结论 | 依据 |
|---|---|---|
| 按钮点击不命中 | ❌ 用户能进三级菜单,说明命中正常 | 用户实测 + 我后来承认判断错误 |
| `RETURNCODE` 让出/恢复坏了 | ❌ 直连标签 9 行全推进 | §3.1 |
| 文本任务死锁 | ❌ 每句都在 `E0/E1` 暂停恢复 | §3.3 |
| `CGINFO[SX/SY]` 恒 0 导致按钮矩形为 0 | ❌ 资源尺寸注入正常(`btn_click_bt3 = 144x18`) | 探针 `note_asset_size` 输出 |
| `@BT.GX` 组偏移为 0 导致按钮跑到屏幕外 | ❌ 菜单按钮全在屏内且能点 | §3.2 会话截图 |
| 标题「終了するる」多一字 | ❌ **实测没有多字**,放大截图逐字正确 | `target/tmp/title_btn_zoom.png` |
| 文本原点不随窗偏移 | ❌ **用户明确指示:官方就是不偏移的,不要改** | 用户原话 |
| `WINDOWINFO[DISPLAYNO]` 参与推进判据 | ❌ 只在 `es.GSD.SAVE` 里做 6 次取值,不在分支判据 | s12 pc656-661 反汇编 |

---

## 6. 当前未提交的改动(`git diff`,共 58 行)

```text
crates/yu-rt/src/sysvar.rs       | 22 ++++++++++++++
crates/yu-rt/src/vm.rs           | 11 ++++++++-
crates/yu-rt/src/vm/handlers.rs  | 27 +++++++++++++++-
```

完整 diff 已存 `target/tmp/handoff-current.diff`。

### 6.1 `crates/yu-rt/src/vm.rs` — 入口标签支持非 ASCII ✅ 建议保留

`LabelTable::get` 只做 ASCII 等值,而语料里有 `フェイスウィンドウサンプル` 这类
标签名。改为先按 CP932 编码再走 `get_bytes`(ASCII 名结果不变)。
无此改动则无法直连样本标签做取证。

### 6.2 `crates/yu-rt/src/sysvar.rs` — `_CHR` 实现 ✅ 建议保留

按 §4.1 的结论:`$_CHR(n)` 返回单字节 `n`,附 REA/语料锚点注释与「手册无页」标注。

### 6.3 `crates/yu-rt/src/vm/handlers.rs` — `TASK[RESTART]` 实现 ⚠️ 需复核后定

按 §4.2 的结论:置位后 `push_effect(Effect::RequestExtraFrame)`。
**注意**:`Effect::RequestExtraFrame` 的现有语义是「主任务 spawn 时请求续帧」,
`Scheduler` 已能消费(`extra_frame`)。但 `TASK[RESTART]` 的目标任务不一定是
当前任务,且官方同时调了 `0x42f7bc`(重排任务链)—— 是否需要额外重排**未验证**。

---

## 7. 建议的下一步(按优先级)

1. **先定位官方 `+0x36` → `+0x37` 的消费点**(§4.3)。这是「一句结束后清屏」的
   唯一未闭环环节,也是「画面永远叠第一句」最可能的直接原因。
   入口:REA `0x404935` 附近的文本泵主循环,以及 `0x4638e4`(`TEXT[CLEAR]`)的调用者。
2. **实测 `RECTPAINT3 SET=0` 是否真的执行了**(§4.4)。在 `cgact_image` 里临时打点,
   或看 `image_log` 里 `BT.OFF...MESW.TX` 的 status。
3. **在 `text_draws` 这一条通路上查「为什么只显示第一句」**(§4.5)。
   重点看 `LayerTexts` 的去重键与 `clear_layer_texts` 的触发时机。
4. 复核 §6.3 的 `TASK[RESTART]` 实现(是否需要任务链重排)。
5. `WINDOWINFO` 字段族(WSX/WSY/FULLSCREEN/FULLSCREEN2/DISPLAYNO/CAPTION/ACTIVE/
   MINIMIZE/MAXIMIZE/CLOSEMODE)仍是 `gfx.info.unsupported`,桌面会话 **10224 次**。
   虽不直接卡推进,但属 B2 在册项,建议一并补齐。
6. `FILEINFO[SIZE]/[RELATION]`:手册有页(`file/fileinfo.html` 例 3/4),
   语义明确(文件大小 / 关联 exe 全名),语料 1 处 `RELATION`。B10 在册。

---

## 8. 可复用的取证工具(已入库)

| 工具 | 用途 |
|---|---|
| `crates/yu-rt/examples/text_scenario.rs` | 裸 `Vm` + 周期点击,打印文本事件/diag/任务。**支持 `YURIS_ENTRY_LABEL`** |
| `crates/yu-render/soft/examples/text_probe.rs` | 挂 `GfxHost` 跑,输出逐字排版 TSV + PNG/JSON |
| `crates/yu-cli/examples/input_probe.rs` | `Engine` 全链路 + 定时点击 + 逐帧截图/JSON |
| `crates/yu-rt/examples/dump_game_cmds.rs` | 倾倒任意脚本 id 的 pc 区间反汇编(命令+窗口载荷) |
| `crates/yu-rt/examples/probe_labels_of.rs` | 列标签 / 按关键词搜全表 |
| `crates/yu-rt/examples/probe_script_map.rs` | 脚本 id ↔ 路径映射 |

临时探针(`probe_text_flow` / `probe_cginfo_sx` / `probe_bt_arrays` /
`probe_engine_text` / `probe_text_loop`)均为会话内一次性工具,**已在收尾时删除**,
需要时可参照本文档描述重建。

### 8.1 关键脚本 id

| sid | 文件 | 说明 |
|---|---|---|
| s0 | `es_Button.yst` | 按钮系统(4576 行,含 `BT.CGCLEAR`、`BT.MOUSECHECK`) |
| s12 | `es_GSD.yst` | 全局存档(含 `WINDOWINFO` 6 连取) |
| s20 | `es_Scenario.yst` | 场景主环(泵环 pc192-249、暂停/恢复) |
| s71 | `es_Text.yst` | 文本任务(3418 行,`TEXTTASK.LOOP`/`CRPREPLACE`/`BUFSET.SPD`) |
| s72 | `es_TXExec.yst` | `es.Mes.Clear` 等执行件 |
| s136 | `jp_0054.txt` | **フェイスウィンドウサンプル 样本本体** |
| s139 | `scenario_start.txt` | 场景启动(LOGO/TITLE 分支) |
| s140 | `start.txt` | 主菜单树 |

---

## 9. 值得记录的教训

1. **不要把「画面没反应」当成「脚本没推进」**。先用 `Vm::text_state()` 看
   `bytes` 是否增长,能一次分开「VM 层」与「呈现层」。
2. **headless 探针会给出假象**。`text_scenario` 直连标签时推进正常,但真实
   菜单路径下卡住 —— 差别在整条启动链的状态(尤其是 `es_Text` 的
   `@TX.BUF.*` 缓冲与 `FES.STATE`)。取证要贴近真实路径。
3. **不要动 `sync_text_state`**。本作对话正文走 `CGACT[TEXT]`(`text_draws`),
   不是自动文本层(`text_layout`)。改动前先确认哪条通道在生效。
