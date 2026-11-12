# 性能优化 A–D 组独立审查（2026-11-12）

> 审查对象：当前工作区未提交改动（基线 `855a983`，`855a983..HEAD` 无提交）。
> 范围：T0、T1、T2、T6、T7、T8a、T8b、T8c、T9、T10、T11（A/B/C/D 四组）。
> 语料：本轮按主人指定用 `kotoneiro`（只读）+ `--debug-ui`；未运行 `yu-cli case`。
> 结论先行：**C 组 fail（1 条 P0 语义破坏 + 1 条 P1 性能回退），A/B/D 组 pass-with-issues
> （实现基本对齐，但验证产物大面积缺失，见 §5）。**

---

## 0. 结论

| 组 | 批次 | 结论 | 依据 |
|---|---|---|---|
| A | T0 / T1 / T2 | **pass-with-issues** | #1/#19/#83/#2/#31 均已落地；600 帧 soft 逐帧 SHA 与 T0 逐字节一致（`t0/t1/t2-sha-soft.csv` md5 全为 `39e494e4…`）；T1 的 `case` 实际哈希 9/9 与 T0 一致。缺口：T2 没有 `--frame-stats` 实测；T2 的 `case --all` 只跑了 4/9 例。 |
| B | T6 | **pass-with-issues** | 纹理记账/LRU/失败条目/文本布局缓存均按语义等价实现；布局缓存新增单测通过。缺口：T6 无任何独立验证产物；`#37` 只部分达成；解码缓存每次查询都要分配 key。 |
| C | T7 / T8a / T8b / T8c | **fail** | **C-01（P0）**：`TASKINFO`/`TASK` 读到的任务链投影在同一逻辑帧内“自然结束”后陈旧，返回值变化。**C-02（P1）**：`GraphicsHost::apply_domain` 默认实现深拷贝 args，生产热路径比旧实现更慢。**C-04**：record-events on/off 的“证据”四个文件全是 CLI 参数错误，护栏从未真正执行。 |
| D | T9 / T10 / T11 | **pass-with-issues** | 静态逐项核对等价（含 256 key Murmur 对拍、CRC 表对拍、SOUNDINFO 位集单测）；kotoneiro 实测 BGM 流式 74 万帧样本、`underruns=0`。缺口：T9/T10/T11 **没有任何** 验证产物（无 CSV/日志/开包耗时）；`resolve_cache` 命中但装载失败时不再回退探测。 |

---

## 0.1 置顶：P0 语义破坏（C-01）

**`TASKINFO` / `TASK[SCRIPTPOS]` 的任务链投影在同一逻辑帧内陈旧**

- 位置：`crates/yu-rt/src/task.rs:630-632`（首次泵同步）、`:711`（仅链变化同步）、
  `:748-796`（自然 Finish / Err / ExitProgram 置 `finished=true` 后直接 break）。
- 旧行为（`855a983`）：`pump_node` **每个任务泵入口**都
  `exec.sync_task_view(&view_of(nodes, &ctx.pending))`。任务 A 自然结束后，
  任务 B 的泵入口立刻看到 `A.finished=true`。
- 新行为：`PumpCtx.view_dirty` 只在**每个逻辑帧第一个泵**和 `chain_changed`
  时同步；自然结束（`nodes[cur].finished = true`）不置 `view_dirty`。
  同一逻辑帧内后续任务读到的仍是“A 未结束”的旧投影。
- 可观察后果（同一逻辑帧内，脚本可见返回值变化）：
  1. `TASKINFO[EXIST ID=A]`：旧 `0`，新 `1`；
  2. `TASKINFO[E/A/Z ID=A]`：旧走“ID 未命中 ⇒ 写默认值 0”，新返回 A 的实际值；
  3. `TASK [SCRIPTPOS ID=A …]`（`crates/yu-rt/src/vm/handlers.rs:2464`）：
     旧走“目标任务不存在”Fatal，新走“已存在但无执行位 ⇒ 新建 `ExecState`”分支，
     给一个已结束任务重新插入执行位。
- 修复建议：自然 `Finish` / `Err` / `ExitProgram` 分支置 `ctx.view_dirty = true`
  （或就地 `sync_task_view`）。这两个分支低频，不影响该优化的收益。

---

## 1. 完成度表（逐 TODO）

> 状态口径：`done` = 代码落地且语义核对通过；`partial` = 落地但未完全达到条目目标；
> `missing` = 未找到实现；`wrong` = 实现与条目语义不符。
> 行号为当前工作区文件行号。

### A 组（T1 / T2）

| # | 状态 | 证据 |
|---|---|---|
| 1 | done | `crates/yu-gfx/src/state.rs:891` `frame()` 不再读写 `frame_cache`（字段已删）；`:767-770` `invalidate_frame` 只清 pending 烘焙。 |
| 19 | done（历史项） | `state.rs:1274-1282`（`windows_rev` 代次缓存）、`:1314-1319`（`diags_rev` 缓存）、`:903`/`:845` `image_probe_order.clone_from`。`f.screen_snapshots` 仍逐帧 `clone`（`:1349`，与 `#19` 备注一致）。 |
| 83 | done | `sort_unstable_by_key`:1219、`sort_unstable_by`:1293/1516、`sort_unstable`:5255（`sorted_children`）；键均含全局唯一 `seq`。 |
| 2 | done | `Node.layer: Arc<Layer>`（`:127-129`）；`CompositorFrame.layers: Vec<Arc<Layer>>`（`frame.rs:308`）；`Op::Layer`/`GroupDraw`/`TransitionDraw`/`MaskDraw`/`ScreenSnapshot.layers` 同步；写路径全部 `Arc::make_mut`。护栏：`crates/yu-gfx/tests/frame_cost.rs` 新增 `op_layers_share_the_frame_snapshot_handle`、`captured_frame_survives_later_layer_writes`（均通过）。 |
| 31 | done | `Node.children: FxHashSet<LayerId>`（`:131`）；发射统一走 `sorted_children`（`:5233`，`(z,seq,id)` 排序）；`group_has_visible_content` 只求布尔，顺序无关。 |

### B 组（T6）

| # | 状态 | 证据 |
|---|---|---|
| 10 | done | `texture.rs:209/326` `failed_entries` 计数；`:628-630` 未超限直接 return；所有增删路径同步（`:611-618`、`:644`、`:703-706`、`:1043-1046`）。 |
| 11 | done | `texture.rs:873` `touched_entry` 一次 `get_mut` 完成 touch+取值；`:889` `failure_issue` 复用 `Rc<AssetIssue>`。 |
| 17 | done（无运行时证据） | `text.rs:1436` 布局缓存（header+prefix+visible）；`:1334` `compose_layer_layout_owned` 消费 owned 基面；`paint_layout_range`（`:1250`）只画尾部。语义键覆盖：`layout_header`=origin/style/line_height（`:326`），`layout_prefix`=glyphs[0..n] 的 bytes/codec/x/y（`:338`）。 |
| 32 | done | `text.rs:1687-1690` 复用 `old_baked`，只为尾部 `dedup_key()`。 |
| 33 | done | `FaceKey`（`:390-420`，NAME→NO→Default 与 `TextStyle::font_request` 逐项一致）；`ascent_px` 按 `(face,sy)` 缓存（`:590`）；`decode_display_cached`（`:573`）。 |
| 34 | done | `text.rs:915` `timing = self.tuning.p8_timing`，`Instant::now` 改为 `then(Instant::now)`。 |
| 35 | done | `texture.rs:218` 嵌套 `FxHashMap<Vec<u8>, FxHashMap<i64,..>>` + `Borrow<[u8]>` 查询（`:799-803`）。 |
| 36 | done | `texture.rs:940-946` key 只由调用方构造一次，`movie_frame_keyed` 复用。 |
| 37 | **partial** | `texture.rs:970-982` 仍有 3 次 key 分配（`to_vec` + `Cached.resolved` 的 `clone` + `insert_cached` 内部的 `cache.insert(key.clone())`）。旧实现 5 次（另有 `inserted`/`meta`），确实下降，但注释“key 只构造一次”不成立。 |

### C 组（T7 / T8a / T8b / T8c）

| # | 状态 | 证据 |
|---|---|---|
| 43 | done | `vm.rs:411-445` `VaractPlan`；`vm.rs:2269` 在 `fill_cmd` 构建；`handlers.rs:5663` `build_varact_plan` 按参数名解析一次；执行期 `handlers.rs:1821-1833` 零名字扫描；`flags` 改 `SmallVec`（`:1800`）。 |
| 50 | done | `hash.rs:12-40` 编译期 256 项表；`crc32` 逐字节查表；新增 `table_matches_bitwise_reference` 用例（通过）。 |
| 84 | done | `vm.rs:822-832` boot 期求和（剔除 `u32::MAX` 哨兵）；`:1387` 直接返回。前提：`tables.boot.scripts` 启动后不变（代码中未见运行期改写）。 |
| 38 | done | `handlers.rs:455-465` 域命令复用顶部 `eval_arg_windows` 结果（`args.clone()` 给 Command，原件给 DomainCall）。已验证 `eval_window` 失败**不**自行发 Diag，故不会改变诊断计数。 |
| 39 | **partial** | `event.rs:370` 新增 `apply_domain` 默认实现；`vm.rs:1971-1993` `emit_domain_borrowed` 仅在 `!record_events && Graphics/Window && graphics_host` 时走借用入口。**但仓库内没有任何宿主覆写 `apply_domain`**（`grep` 仅命中 trait 默认实现与调用点），默认实现 `args.to_vec()` 反而把参数深拷贝一次（见 C-02）。 |
| 40 | done（理论边界见 P2-04） | `vm.rs:2248-2254` `eval_fails`；`expr.rs:952-984` `instr_supported` 白名单与 `Evaluator::step`（`expr.rs:645-800`）支持集逐项一致；`handlers.rs:373-377` 跳过必失败窗口。 |
| 41 | done | `vm.rs:2406` `eval_window_ref`；`handlers.rs:1522/1811/2162` 只读消费者使用。 |
| 12 | done | `input.rs:163` `key_code_of_id`；`:75-115` 启动期建表；`sysvar.rs:389-394` 读路径直查；`handlers.rs:1294-1300` 写路径 `set_counter_code`。已验证 `_KEY_ANY`/`_MOUSE_X0..3`/`_PAD_*` 不进该表（`from_sysvars` 的 match 顺序 + `is_mouse_button`）。 |
| 13 | done | `input.rs:322-350` `Rc<InputFrame>` 时间线 + `effective_shared`；`:621-676` `advance` 共享快照；`:858-889` `DownSet` 位图。`pulses` 语义与旧实现一致（旧 `at(frame)` 覆写在命中时是 no-op）。 |
| 42 | done | 同 #12；`sysvar.rs:270-277`。 |
| 51 | done | `handlers.rs:6119-6135` `write!` 写十六进制；`sysvar.rs:272-277` 删除 `format!`。 |
| 44 | done | `handlers.rs:5629` `TaskinfoFacts`（借用域内取标量）；`:2597-2620` 不再 clone `ExecState`；`:2568-2595` `present` 只存槽号。 |
| 45 | done | `handlers.rs:5926-5942` 复用 `w.instrs`，未预解码才回退 `decode`。 |
| 46 | done | `vm.rs:2674-2682` `find_label_target`（`Copy`）；`handlers.rs:5511/5564/5572` 换用。 |
| 47 | done | `vm.rs:532/846/1336/2798/2882` `exec_slot_seq` 按 `u64` 比较。 |
| 48 | done | `vm.rs:1462-1470` `text_bytes`/`text_flags_ref`（只读消费者）。 |
| 49 | done（引入 C-01） | `task.rs:262` trait 改 `Vec<TaskView>`；`vm.rs:2932` 直接接管；`task.rs:630-632/711` `view_dirty`。 |
| 79 | done | `vm.rs:2138-2152` `get_mut`/`contains`/惰性 detail；`:2154-2168`；全部调用点改为 `&'static str`。 |
| 80 | done | `value.rs:880-914` `write_owned`；`coerce_to_owned`（`:477-495`）与 `coerce_to`（`:455-473`）逐分支一致；`handlers.rs:1447-1455` 使用。 |
| 81 | done | `handlers.rs:6089-6097` `into_str_bytes`；`:4905-4909` `WORD` 使用；`cast_to_str` 对 `Str` 就是 clone，已验证等价。 |
| 82 | done | `handlers.rs:3157-3176` `named_command_values` 借用参数名；`:3178-3190` 值 move 进事件；`op_input`/`op_mouse`/`op_text`/`op_text_out` 改按槽号/借用。 |
| 87 | done | `handlers.rs:1471` `decl_target`、`:1488` 维数、`:1513`/`:1800` `flags`、`:4010` `flags` 均为 `SmallVec`；`op_math` 只改 `eval_window_ref`（其 Vec 原已收敛）。 |

### D 组（T9 / T10 / T11）

| # | 状态 | 证据 |
|---|---|---|
| 16 | done | `ypf.rs:166-205` `murmur2_xor`；`:850` 探测改零分配；新增 256 key 全量对拍用例（通过）。 |
| 52 | done | `ypf.rs:1019-1029` 先算 hash，再 `mem::take(name_enc)` 原地 XOR 并 move；`detect_layout`（`:752-790`）按值消费条目。 |
| 53 | done | `lib.rs:154-169` 删除 ancestors 收集，直接 `insert_dir(parent)`；`insert_dir`（`:125-152`）自身递归。 |
| 54 | done | `ypf.rs:316` `lookup: HashMap`；仅 `get`/`contains_key`（`:553/590/633`），不参与输出迭代；`dir.rs:34` 同理。 |
| 55 | done | `ypf.rs:599-601` 容量上限 `min(expected, 64MB)`。 |
| 56 | done | `ystb.rs:268-278` 借用键 `HashMap<&str,u16>`（首次出现为准）；`collect_label_refs`（`:366`）直查。 |
| 57 | done | `snappy.rs:108-118` `extend_from_within` 分块；`offset>=1` 由前置校验保证，语义与逐字节一致。 |
| 14 | done | `mixer.rs:87-95` 先跳过 `!audible || paused` 与无流容器；`mix_layer` 的两个 return 在 `:165-177`，逐项等价；层迭代顺序未改。 |
| 15 | done（存在 D-02） | `state.rs:1139-1156` 命中缓存不再构造候选名；未命中才 `load_candidates`；未找到诊断用同一函数重算。 |
| 58 | done | `host.rs:33-44` 参数名改 `Cow` 借用；`:66/104` 用 `as_ref()`；只对 ID/FILE clone。 |
| 59 | done | `state.rs:1093-1112` 先在局部 layer 上 `prefetch_stream`，再 move `key` + insert。 |
| 60 | done | `state.rs:422-445` 固定 22 项序表；`:1436-1459` 位集去重 + 短路；`:1462` `has_file`；单测 `query_field_order_is_an_inverse_mapping`（通过）。 |
| 61 | done | `state.rs:531/571` `capture_active`；`mixer.rs:117-125` relaxed 门控；start/take 成对开关（`:1323-1343`）。 |
| 62 | done | `state.rs:1305-1314` `pop_first`；注释已改正为“归一键字典序”而非“最旧”。 |
| 63 | done | `state.rs:546-550` `RESOLVE_CACHE_CAP=4096`；`:1155-1158` 满了清空。 |
| 72 | done | `crates/yu-platform/host-desktop/src/sound_device.rs:39-50` peak 只在 `first_audio` 未置位时计算。 |
| 64 | done | `cache.rs:166-178` 先 `next_tick` 再单次 `get_mut`；miss 时多推进一次 tick 不影响 LRU 相对序。 |
| 65 | done | `raster.rs:48/128-136/225-238` `(face,char)` 负缓存；`GLYPH_MISSING` 的 detail 只含 face index + 码点（`:145-151`），与字号无关。 |
| 66 | done | `source.rs:563-586` 排序前预计算小写键；比较次序 `(lower(name), index, FaceId)` 不变。 |
| 67 | done | `layout.rs:46-75` 每字符一次 `glyph_index`；upem 每行一次（仅 `Kerning::Font`）；失败语义等价。 |
| 88 | done | `render.rs:132-158` 第一遍保存 `Vec<GlyphBitmap>`，第二遍复用；像素路径与旧两遍一致。 |

---

## 2. 问题表

### P0

| ID | 位置 | 旧行为 | 新行为 | 复现/证据 | 建议 |
|---|---|---|---|---|---|
| C-01 | `crates/yu-rt/src/task.rs:630-632,711,748-796` | 每任务泵入口 `sync_task_view`，刚结束任务立即可见 | 仅每逻辑帧首泵/链变化同步，自然结束不置 `view_dirty` | 同帧内 A 自然 RETURN 后，B 执行 `TASKINFO[EXIST ID=A]`：旧 0 / 新 1；`TASKINFO[E/A/Z]`、`TASK[SCRIPTPOS]` 亦分歧（§0.1） | 自然 Finish/Err/ExitProgram 置 `ctx.view_dirty = true`（或同步） |

### P1

| ID | 位置 | 旧行为 | 新行为 | 复现/证据 | 建议 |
|---|---|---|---|---|---|
| C-02 | `crates/yu-rt/src/event.rs:370-388`、`crates/yu-rt/src/vm.rs:1988` | 生产路径（`record_events=false`）`emit_domain` 把 owned `args` move 进事件，0 次深拷贝 | `apply_domain` 默认实现 `args: args.to_vec()`，每条 Graphics/Window 域命令**多一次 `Vec<ArgValue>` 深拷贝**（含字符串字节） | 仓库内无宿主覆写 `apply_domain`（grep 仅 trait 默认实现）；CG/CGACT/CGEND 是菜单帧 1.7 万次级热路径 | 给 `GfxHost`（`crates/yu-gfx/src/host.rs:87`）覆写 `apply_domain` 直接消费借用参数；否则删除该入口回到旧路径 |
| C-03 | 全仓 | — | `cargo test --workspace --all-targets` **不绿**：`silent_fallback_audit` FAILED | 输出：`state.rs 未标注 22 > 白名单 18`、`input.rs 11 > 9`、`value.rs 5 > 3`、`vm.rs 5 > 4`、`handlers.rs 80 < 92`（白名单需下调） | 新站点补 `// silent-ok:`：`input.rs:713`、`:339`；`value.rs:483/489`（`coerce_to_owned`）；`vm.rs:2458`（`param_plan` 兜底）；同步下调 `handlers.rs` 白名单到 80。`state.rs 22>18` 为基线既有（`git show 855a983` 同样 22），另行处理 |
| C-04 | `target/tmp/perf/t8a-*-on/off.log` | 需求：`record_events=on` golden 逐字节对比 | 四个文件内容均为 `[yu-cli] 未知参数:--record-events`，**该护栏从未执行** | `cat target/tmp/perf/t8a-before-on.log` 等 | 用 `--mode headless --record-events on --json …` 重跑并保存 JSONL 对拍 |
| A-01 | `target/tmp/perf/` | T2 验收：菜单/稳态 `--frame-stats` 对照 | 无 `t2-*` CSV；`t2-case-all.log` 只含 4/9 例 | `ls target/tmp/perf/t2-*` | 补 600 帧稳态 + 480 帧菜单 CSV，与 T0 同口径 |
| V-01 | 验证产物 | 任务书要求每批必跑对应矩阵 | T6/T7/T8b/T8c/T9/T10/T11 **无任何** CSV/日志/开包耗时/trace 产物；仅有 T8a bench、T1/T2 部分产物 | `ls target/tmp/perf` | 按任务书 §2.3 逐批补跑；否则完成度只能记 partial |
| B-01 | `crates/yu-render/core/src/text.rs:573-585` | `decode_display` 每帧分配 `Vec<char>` | 缓存命中路径仍要 `bytes.to_vec()` 构造 key（一次新的堆分配 + 哈希），短串上可能不划算 | 代码路径；`TextDraw.bytes` 高频 | key 改 `(Codec, u64 指纹)` 或按 draw 的稳定 id 缓存；或先 profile 再决定 |
| D-02 | `crates/yu-sound/src/state.rs:1124-1138` | 缓存名字读不出来时回退完整探测，可能找到其它候选 | 直接按“未找到”收口（只重算候选名做诊断） | 数据目录在运行期被外部改动时才可观察 | 恢复“缓存命中但 load 失败 ⇒ 清缓存项并走完整探测”，保持旧边界 |
| B-02 | `crates/yu-render/core/src/texture.rs:970-982` | 5 次 key 分配 | 3 次（仍非 1 次） | 静态计数 | `insert_cached` 接受 `Vec<u8>` 并让 `resolved` 复用同一 `Rc<[u8]>`/`Arc<[u8]>`，或把 `resolved` 改为 `Option<Rc<[u8]>>` |

### P2

| ID | 位置 | 说明 |
|---|---|---|
| C-05 | `crates/yu-rt/src/vm.rs:2250` + `expr.rs:1014-1016` | `instr_supported` 用 `decode_string_literal`（固定 Cp932）判定 0x4d，而实际求值用窗口 codec。理论分歧仅当“非 Cp932 工程的字符串字面量里出现 Cp932 lead byte 紧邻终止符”（对合法 UTF-8/GBK 文本不可达），故记 P2；建议改成 `decode_string_literal_with(payload, codec)`（需把 codec 传进 `fill_cmd`）。 |
| A-02 | `crates/yu-gfx/src/frame.rs:308` 等 | 公开类型破坏性变更：`CompositorFrame.layers`、`Op::Layer`、`GroupDraw`、`TransitionDraw`、`MaskDraw`、`ScreenSnapshot.layers` 由 `Layer` 改 `Arc<Layer>`；`TaskExecutor::sync_task_view` 签名改为 `Vec<TaskView>`；`Cmd.varact_plan`/`CmdWindow.eval_fails` 为新增公开字段。仓内消费点已全部适配，但需主人确认下游无外部使用者。 |
| A-03 | `crates/yu-rt/src/vm.rs:3020` | `fn param_index_bytes(...) {    let mut end = ...` 同一行两个语句，非 rustfmt 形态（纯格式）。 |
| A-04 | `crates/yu-platform/host-desktop/examples/probe_buttons.rs:139/199`、`probe_flow.rs:163` | 为适配 `Arc<Layer>` 增加 `.map(|l| l.as_ref())`；测试探针改动，非生产行为。 |
| A-05 | `crates/yu-render/soft/src/render.rs:1219/1279` | soft 文件被改：仅 `solid_fill_texture(layer.map(|l| l.as_ref()))` 的机械类型适配（该函数本来收 `Option<&Layer>`），无像素/算法改动。严格按“soft 不在范围”记录为越界文件，但属 T2 编译必需。 |
| X-01 | `yu-emote` 子模块 | 指针 `1b46604 → a54cbd3` 与 A–D 批次无关；按 AGENTS §4.1 不应随本批提交，需主人单独确认。 |
| X-02 | `docs/tasks/todo-perf-2026-11-11.md` / `task-perf-2026-11-11.md` | 台账/任务书未回写：T2/T6/T7/T8a/T8b/T8c/T9/T10/T11 仍写“未开始”，与本轮代码不符；#2/#31 也未改成 `已收口`。 |

---

## 3. 语义对齐核对（重点项逐条）

### 3.1 A 组

- **`frame()` 重复调用逐字节一致**：`frame()` 已无整帧缓存（`state.rs:891-1360`）；投影只读
  `self.nodes`（BTreeMap）、`self.text_*`（BTreeMap）、`self.diags`（BTreeMap），
  没有 HashMap 迭代参与输出；`last_screen_key` 写入是幂等纯函数。静态判断：一致。
  自动化单测目前**没有**“连续两次 `frame()` 相等”的断言（建议补）。
- **`Arc<Layer>` 后状态快照/JSON/层序**：`f.layers` 仍按 `(z,seq,id)` 排序（键全序）；
  `json.rs` 手工序列化按字段读取，`Arc` 的 `Debug`/`PartialEq` 均代理到 `Layer`，
  序列化与比较语义不变。`t0/t1/t2-sha-soft.csv` 逐字节相同 + `case` display 哈希
  一致是最强证据。
- **`sort_unstable` 只在键唯一处使用**：`seq` 由 `next_seq` 单调分配、层删除不复用；
  `texture_seq` 同样全局唯一（`state.rs:1215-1218` 注释与实现一致）。成立。
- **capture/last_screen**：`capture_screen_snapshot` 的 layers 改为共享句柄后，
  后续命令写层走 COW；`frame_cost::captured_frame_survives_later_layer_writes` 覆盖。
  `last_screen_key` 的输入未改。
- **缓存失效完整性**：`windows_rev` 只在 `apply_window` 末尾与 `apply_windowend`
  删除后递增；`apply_window` 的越界 `NO` 早退发生在任何 `windows` 写入之前
  （越界时 `get_mut(no)` 也必然 miss），无漏失效。`diags_rev` 在 `FrameStart`
  清空与每次 `push_diag` 递增，覆盖 `count`/`at` 变化。

### 3.2 B 组

- **纹理预算/淘汰顺序/失败条目上限/issues 计数**：`bytes` 语义（成功像素字节、失败 0）、
  `used` 推进点、`inserted` 只在插入覆盖时推进，均与旧 `meta`/`inserted` 表一致；
  `pick_victim` 的 tie-break `(used,key)` 不变；`enforce_failed_cap` 早退不改变结果。
  `issues()` 仍按 `(key,code,detail)` 排序，`Rc` 只省克隆。
- **文本 layout 缓存**：键 = 层 id + `base_key` + 尺寸 + header(origin/style/line_height)
  + 已画前缀指纹；覆盖影响像素的全部 `TextGlyph` 字段（`bytes/codec/x/y`；`full` 不参与
  绘制）与字体快照。`reveal` 回退走整体重烤；`visible` 前进只补尾部。
  纯命中复用缓存 `hash`；增量重算 `content_hash_rgba`。
  `LayoutStats` 重放 `draws/glyphs/missing/diags`，与原“每帧重画”的聚合计数一致；
  `text.font_init` 一次性诊断被排除（`take()` 后旧路径也不会再报）。
- **增量烘焙前缀**：`Draws` 分支复用 `old_baked`，仅尾部 `dedup_key()`；`hit_len` 之前
  用 `matches_draw` 逐字段确认（不是哈希单独判等）。
- **`faces` HashMap 键等价**：`FaceKey::of_style` 与 `TextStyle::font_request`
  （`yu-gfx/src/text.rs:146-158`）逐分支一致（NAME 优先，其次 NO，否则 Default）；
  `TextStyle` 的 `Hash` 已存在，`layout_header` 可直接哈希。`describe()` 文案逐字保留。
- **`p8_timing` 关闭零计时**：`timing.then(Instant::now)`，`timing=false` 时
  `Instant` 不取、`dbg_*` 不累加；`:949` 的 trace 条件用 `timing`。

### 3.3 C 组

- **事件顺序/name/args/script/pc**：`emit_domain_borrowed` 的丢弃条件与旧 `emit`
  的 `!record_events && consumed_by_host && Graphics|Window` 一致；其余情形回落
  `emit_domain`，字段与旧路径相同（`frame()` 就是字段本身，已验证）。
  `record_events=true` 时 `Command` 与 `DomainCall` 的 args 来自同一次求值，
  旧实现两次求值但无副作用、无重复 Diag（`eval_window` 失败不自报），载荷一致。
- **VARACT 槽位按参数名**：`build_varact_plan` 用 `param_names`（工程 ysc）按名取槽，
  未写死槽号；`DIMSIZE` 复合码取该槽窗口 `rec.slot >> 8`（静态）；
  `g_slots` 覆盖所有 `G_INT/G_FLT/G_STR` 名（在场判定）与 `<4` 的掩码位。
- **CRC32 检查值**：`"123456789" → 0xCBF43926`，另有逐字节按位实现对拍。
- **`_TEXT_NUM`**：boot 期求和、剔除 `u32::MAX`，与旧函数体逐字一致。
- **输入 advance 的 prev/current/pulses**：`prev = Rc::clone(current)`、`current = now`
  与旧 clone 顺序相同；旧 `pulses` 覆写在 exact 命中时是 no-op，未命中时旧实现本身
  也保持前值，故等价。
- **sysvar id→code 覆盖**：`_KEY_ANY`、`_KEYLAST`、`_KEYNUM_*`、`_MOUSENUM_*`、
  `_MOUSE_X/Y/X0..3`、`_PAD_*` 均不进 `key_code_by_id`（match 顺序 +
  `is_mouse_button` 过滤），回落到原专用分支；ABI 表无 code 0，`set_counter_code`
  的 `code==0` 早退与旧 `set_counter` 无可观察差异。
- **`write_owned` 与 `write` 的 coerce_to 等价**：已逐分支对照（`:455-473` vs `:477-495`），
  含 `Str→Int/Float` trim+parse、`Int/Float→Str` 的 `{f:.6}` 格式。
- **未接线挂号计数/文案**：`report_unimplemented`/`report_aligned_stub` 只在首次构造
  `detail`，文案逐字不变；`unimplemented_cmds` 计数路径不变。
- **#47 `exec_slot_seq`**：`seq` 由调度器创建、任务生命周期内稳定；换任务/结束时
  置 `u64::MAX`。`flush_exec_slot` 也清零。正确。
- **#44 TASKINFO**：`present` 由名字改槽号后，`ORDER` 选择改为“取 ORDER 中位置最小者”
  （与旧 `find` 同义）；`FILENO0+SCRIPTPOS` 二次写入的在场判定用
  `names.position("SCRIPTPOS") → present.contains(slot)`，与旧名字判定等价；
  缺层/缺执行位的默认值与 `scriptpos` 兜底一致。

### 3.4 D 组

- **YPF 名字 key 探测 256 个 key 逐位等价**：`murmur2_xor` 在 4 字节块与余数上先 XOR，
  自源到目标不重叠；新增用例覆盖 256 key × 6 样本 + 257 字节长样本 + seed。
- **名字 XOR/move 只做一次**：`parse_hashed_len` 先算 `name_hash_ok`，再
  `mem::take(name_enc)` 原地 XOR move；`detect_layout` 按值消费条目、`type_counts` move。
- **DirTree 祖先插入顺序**：`insert_dir` 自身递归 root→…→parent，与旧“收集 ancestors
  后反向逐个插入”顺序一致，且少构造键。
- **HashMap 不参与输出迭代**：`YpfSource.lookup` 仅 get/contains；`DirSource.files`
  仅 get/contains/len/insert；`YpfSource.entries`/`DirTree` 保持原顺序。
- **Snappy 重叠复制**：`chunk = min(remaining, out.len()-start)`，首轮 `offset`，
  逐轮翻倍；`offset==0` 已被前置校验拒绝，无死循环。
- **混音层顺序/浮点求和顺序**：`layers_iter()` 不变；新增跳过只省略“无贡献层”
  （`mix_layer` 对它们是立即 return），不改变可听层的求和顺序。
- **SOUNDINFO 短路/最后者胜出**：位集下标 = `order()`，短路取 `0..=6` 最小者，
  非短路按序全执行；`has_file` 等价 `ordered.contains(&File)`；新增单测覆盖。
- **PCM 淘汰**：`pop_first` 与 `keys().next().cloned()+remove` 都是“最小 key”，等价。
- **字体缓存 key/负缓存/字号排序/照排**：见 §1；`missing_memo` 只缓存
  `GLYPH_MISSING`（face+char），detail 与字号无关；`WholeRun` 两遍改一遍栅格，
  合成顺序不变。

---

## 4. 运行验证（kotoneiro，release，wgpu）

命令：

```bash
target/release/yu-cli.exe run "D:/Art/ai/projects/YurisDroid/workbench/games/kotoneiro" \
  --mode desktop --debug-ui --frames 900 \
  --save-root …/target/tmp/review/koto-save \
  --frame-stats …/koto-frames.csv --log-file …/koto.jsonl \
  --spike-log …/koto-spikes.log --spike-ms 20
# 第二跑：同工程，1800 帧，无 --debug-ui，用于稳态/长空闲
```

### 4.1 实测表（跳过前 5 帧；单位 ms）

| 场景 | 帧数 | 指标 | p50 | p95 | p99 | max | avg |
|---|---:|---|---:|---:|---:|---:|---:|
| 标题/启动（debug UI，900 帧） | 895 | work | 2.258 | 3.710 | 6.372 | 79.505 | 2.689 |
| | | tick | 0.725 | 1.943 | 3.948 | 76.332 | 1.149 |
| | | render | 0.634 | 0.925 | 1.102 | 2.922 | 0.679 |
| | | snap | 0 | 0 | 0 | 0 | 0 |
| | | interval | 16.666 | 18.273 | 31.376 | 83.449 | 16.983 |
| 标题/长空闲（1800 帧） | 1795 | work | 2.494 | 3.936 | 14.393 | 85.651 | 3.002 |
| | | tick | 1.448 | 2.448 | 12.460 | 83.210 | 1.853 |
| | | render | 0.732 | 1.262 | 1.613 | 51.734 | 0.838 |
| | | snap | 0 | 0 | 0 | 0 | 0 |
| | | interval | 16.661 | 17.740 | 33.511 | 100.123 | 17.056 |

- 900 帧：`frames=900 elapsed=15.64s avg_fps=57.5 backend=wgpu viewport=1100x825`；
  `frame pacing p50=16.67ms p95=18.27ms`；spike 4 条（host 75/76/78/238）。
- 1800 帧：`avg_fps=57.8`；spike 15 条（75…1707），层数峰值 440、passes 峰值 64
  （`koto-spikes-1800.log`）；`skipped=0`、`missing_assets=0`。
- 声音：`sound_stream_frames=741446`、`sound_underruns=0`、`sound_seeks=1`、
  `sound_decode_ms=55.0`（T10 混音/流式路径在真实语料上无欠载）。
- 文本：1800 帧内 `text_gpu_layers=278`、`glyphs=4676`、`draws=34086`、
  `text_gpu_fallback=8`；`text_bake` 全程 0（复用缓存，符合 #17/#32 预期）。
- 诊断：900 帧 `total=5`（`sv.file.missing`×2、`table.fallback`、`rt.cmd.aligned_stub`、
  `rt.cmd.unimplemented(EMOTE, deferred)`）；1800 帧 `total=7`（新增
  `sv.command.unimplemented`：`FILEACT[COPY] save30002.sd → save30001.sd`，属
  `yu-save` 既有边界，非本批 diff）。
- `gfx.layer.already_absent`：900 帧 2547 条 / 1800 帧 9965 条（Verbose；CGEND 幂等空操作，
  菜单按钮重建常态，未发现随空闲线性增长的异常证据）。

### 4.2 与 T0 基线对比

**不可直接比较**：T0 基线语料是 `<project-eris>`（`data.ypf`，headless + desktop），
本轮按要求用 `kotoneiro`；宿主/窗口尺寸/后端均不同。只能用 T0 的 ERIS CSV 复核本批
自身前后（见 §4.3），不能拿 kotoneiro 数字对 T0 做归因。

### 4.3 仓库内已有产物的对照（只做复核，不代替本次实测）

| 产物 | 结果 |
|---|---|
| `t0-sha-soft.csv` / `t1-sha-soft.csv` / `t2-sha-soft.csv` | md5 全为 `39e494e4…` → T1/T2 后 600 帧 soft 逐帧像素哈希**逐字节不变** |
| `t1-probe-soft.out` / `t2-probe-soft.out` | 末帧 `a5a618d0…`，与 T0 文档 §7.3 一致 |
| `t0/t1-case-all.log` | 9 例 PASS/FAIL 与 7 条 display/png actual 哈希**完全一致** |
| `t2-case-all.log` | 仅 4/9 例；已跑的 2 条 display 哈希与 T0 一致（证据不完整） |
| `t8-case-all.log` | 仅 6/9 例；已跑的 4 条哈希与 T0 一致（证据不完整） |
| `t8a-bench-before/after.log` | steady 31.3 → 35.5 fps（+13%），但两者都低于 T0 的 41.9 fps（-15%/-24%）；T0 文档已提示并行会话负载，**需静默机复测**；C-02 是可解释该回退的机制之一 |
| T1 steady/menu CSV | work p50 1.964/1.907/1.679 vs T0 1.858/1.827；render p50 0.575/0.561/0.507 vs 0.580/0.569；菜单 p95 work 4.274 vs 4.430（`t1-menu-2` 为 6.399 离群）→ 收益在噪声内，不能宣称稳定提升 |

---

## 5. 越界检查

| 项 | 结论 |
|---|---|
| `crates/yu-render/soft` | **有改动**：仅 `solid_fill_texture(layer.map(|l| l.as_ref()))` 两处（`soft/src/render.rs:1219/1279`），是 T2 `Arc<Layer>` 的编译必需适配；无像素/合成算法改动。按“soft 不在范围”记录，建议主人确认可接受。 |
| 暂缓 13 条（#24–#29、#70、#76、#85、#86、#89、#90、#91） | **未触碰**：#24/#25/#26 的 `screen_contract_key`/`capture_screen_snapshot`/`on_event` 未改；#27/#28/#29 日志裁剪未改；#70 server 未改；#76 wgpu `text_signature` 未改；#85/#86 expr 只新增 `instr_supported`；#89 GlyphKey 未改；#90 yssd 未改；#91 engine 未改。 |
| E/F 批次（T4/T5a/T3/T12/T13/T14） | 未做：#3/#5–#9/#18/#20/#30/#4/#68/#69/#71/#73/#74/#75/#77/#78/#23 均无对应实现（`state.rs` 的 `CG`/`CGINFO` 只做 rustfmt；wgpu soft 只做类型适配）。 |
| 无关重构 | 多为 rustfmt 换行（`state.rs`/`handlers.rs`）；`param_index_bytes` 出现非 rustfmt 同行双语句（P2）。 |
| 公开 API | 见 A-02；均为本批语义所需，仓内消费点已适配。 |
| 子模块 | `yu-emote` 指针变化与批次无关（X-01），不得随本批提交。 |

---

## 6. 无法确认 / 需要主人操作或补语料

1. **交互场景全部未覆盖**：本轮自动化只跑到标题/长空闲（`main_finished=false`）。
   F1 面板、F5 暂停/恢复、F6/F7 步进、F8 录制、F9 抓取；对话逐字、菜单开关、
   存档界面（缩略图/CGS）、场景切换与淡入淡出、读档后画面——**需要主人在
   `--debug-ui` 窗口内操作**，我只能观察 CSV/日志。当前证据不足以判定这些场景的
   `work/tick/render/snap`。
2. **T2 的性能收益未实测**：无 `t2-*` frame-stats；无法确认 `Arc<Layer>` 在
   kotoneiro 菜单（本轮未打开菜单）上的分配下降幅度。
3. **T6/T9/T10/T11 的验收口径未跑**：缺 `--frame-stats`、开包耗时、`sound_census`、
   `raster_bench`、`case --all`（本轮禁跑 case）等产物；只能给出静态等价结论 +
   kotoneiro 的“无崩溃/无欠载”观察。
4. **T8a record-events golden 未验证**：证据文件是 CLI 用法错误（C-04）。
5. **性能回退疑点需静默机复测**：T8a bench 最终值 35.5 fps vs T0 41.9 fps；
   本轮无法区分系统负载、C-02 与其它批次的影响。
6. **kotoneiro 的 `preload_err=10` / `sv.command.unimplemented(FILEACT[COPY])`**：
   需要与官方引擎同场景对照，才能判断是商业包缺资产/既有边界还是本批回归；
   本批 diff 不含 `yu-save`/资产预取逻辑，倾向既有，但未取证。
7. **`frame()` 连续两次调用的自动化断言缺失**：静态判断一致，建议补一个
   `assert_eq!(gfx.frame(), gfx.frame())`（`CompositorFrame: PartialEq`）作为 #1 护栏。
8. **台账/任务书未回写**（X-02）：完成状态与代码不一致，需主人或实现方按 §5 模板更新
   `todo-perf-2026-11-11.md`、`task-perf-2026-11-11.md`、`PROGRESS.md`。

---

## 7. 建议修复顺序

1. C-01（P0，一行 `view_dirty`）→ 补 `TASKINFO` 同帧回归用例。
2. C-02（GfxHost 覆写 `apply_domain` 或回退该入口）→ 重跑 `yu-cli bench`。
3. C-03（silent-ok 标注 + 白名单下调）→ `cargo test --workspace --all-targets` 转绿。
4. C-04 / V-01 / A-01（补验证产物）。
5. B-01/B-02、D-02（性能与边界收口）。
6. X-01（确认 `yu-emote` 指针是否随本批提交）、X-02（台账回写）。
