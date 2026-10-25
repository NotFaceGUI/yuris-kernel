# TODO · 静默处理收口(诊断可定位化)

- 状态: **open**(待排期)
- 建立日期: 2026-10-25
- 触发场景: 商业包 `<game-a>` 片头 LOGO 不上屏,排查时**渲染报告只说
  `skip image x2 gfx.imageop.region.partial :/W0/SPL401`** —— 知道跳过了两次、
  知道跳的是哪个层,但**不知道是哪条脚本的哪个 pc**。最后靠自建四个挂宿主
  探针 + 手工对拍才定位到 `s66 pc6028` 的 `Y2=824.5` 被 `as_int` 判成「未给」。
  同类情况在项目里会反复出现,故单独立项。
- 与现有台账的关系: 本文件只记**机制性缺陷**,不替代
  `docs/todo-inventory.md`(功能缺口总账)与 `docs/hardcode-audit.md`
  (常量出处审计)。

---

## 0. 问题定义

「静默处理」在本项目指**同时满足**以下三条的行为:

1. 出错/不支持/参数缺失时不中止、不抛出;
2. 走默认值或直接跳过;
3. **调用方拿不到足够信息定位出处**。

第 3 条是关键。当前的诊断**并非完全没有** —— 问题在于**聚合时把定位信息丢了**,
于是从「一条错误 = 一个位置」退化成「N 次错误 = 一个计数」,排查时无法反查。

---

## 1. 已定位的具体机制(每条都可直接改)

### S1 `FrameDiag` 聚合丢掉 pc / 脚本号 ★最高优先

**位置**: `crates/yu-gfx/src/state.rs:2798` `push_diag`

```rust
fn push_diag(&mut self, command: &str, d: Diag) {
    let key = (d.code.to_string(), command.to_string(), d.detail.clone());
    match self.diags.get_mut(&key) {
        Some(existing) => existing.count += 1,   // ← pc 在这里被合并掉
        None => { self.diags.insert(key, FrameDiag::new(...)); }
    }
}
```

**后果**: 本作实测 `skip image x2 gfx.imageop.region.partial` —— 两个不同的
pc 用了同一个 `detail` 文案(其中带层名但不带 pc),聚合后只剩 `x2`。
若 `SPL401` 与 `SPLTRBUF401` 走同一条路径,连层名都会被合并。

**注入路径**: `Diag` 本身带 `where_: Loc`(`yu-log/src/lib.rs:142`),
VM 侧 `op_info`/`op_cg*` 在构造 Diag 时能拿到 `st.script_id` + `cmd.pc`,
但 `FrameDiag::new` 的签名只收 `(code, severity, command, detail)`。

**建议改法**: `push_diag` 的 key 加入 `where_`;`FrameDiag` 增加
`at: Vec<Loc>`(最多保留前 N 个不同位置)+ `count`。渲染报告与
`game_diag_probe` 按 `(code, 位置列表)` 输出。

**验收**: 复现本作 f86,报告应出现
`gfx.imageop.region.partial @ s66 pc6028`(而非只有层名)。

---

### S2 `as_int` / `as_f64` 的类型不匹配静默变 `None`

**位置**: `crates/yu-gfx/src/layer.rs:65`、`crates/yu-gfx/src/state.rs:2928`

```rust
pub fn as_int(&self) -> Option<i64> {
    match self {
        Self::Int(i) => Some(*i),
        Self::Float(f) if f.fract() == 0.0 => Some(*f as i64),  // ← 824.5 → None
        _ => None,                                              // 无诊断
    }
}
```

**后果**: `CGACT[… Y2=824.5 SX=1101 SY=825.5]` 的浮点被当成「参数未给」,
下游报的是 `region.partial`(缺字段)而**不是**"类型不符"。排查者会去查
脚本为什么少给参数,而真正的根因是读取口径。本次 LOGO 黑屏即此。

**建议改法**: 区分「未给」与「给了但不能转」。
`find_int` / `find_px` 在"给了但类型不符"时 `push_diag(ARG_TYPE)`,
detail 带实际值与目标类型。不要把它降级成 `None`。

**验收**: 临时把某个 `find_px` 改回 `find_int`,应出现
`gfx.arg.type.mismatch: Y2=824.5 期望整数像素`。

---

### S3 语义层大面积 `unwrap_or(0/1/false)` 不挂号

**位置** (抽样,生产代码共 **244** 处 `unwrap_or(...)`):

| 文件 | 行 | 场景 |
|---|---:|---|
| `yu-rt/src/vm/handlers.rs` | 1031 | `WAIT` 的 `frames.unwrap_or(0)` |
| `yu-rt/src/vm/handlers.rs` | 1171/1178/1187/1219/1253 | 系统变量写入值 `as_int().unwrap_or(0)` |
| `yu-rt/src/vm/handlers.rs` | 2028/2029 | `TASK` 的 `E`/`Z` 默认 1 |
| `yu-rt/src/vm/handlers.rs` | 2034/2036/2040 | `TINT`/`TFLT`/`TSTR` 元素 `unwrap_or(0)` |
| `yu-rt/src/vm/handlers.rs` | 2252-2280 | `TASKINFO` 各查询项缺节点时返回 0 |
| `yu-gfx/src/state.rs` | 1013/1155/1259 | `WINDOW`/`CG` 的 `NO` 缺省 0 |
| `yu-gfx/src/state.rs` | 1969-1974 | 图像动作的 `SET`/`X`/`Y` 缺省 0 |

**后果**: 脚本给错类型(如字符串给到整型槽)时,内核按 0 继续跑,
行为与官方不同却**不留任何痕迹**。

**注意**: 其中一部分**是正确的缺省语义**(手册写明"省略时取 0"),
不能一律改成报错。**必须先分类**,再只对"类型不符"那一类加诊断。

**建议改法**: 分三步 ——
1. 逐个标注: `// 缺省合法(手册 X)` vs `// 类型不符,应挂号`;
2. 后者改成 `match` + `push_diag`/`report`;
3. 加 lint 风格的测试:扫 `yu-rt`/`yu-gfx` 的 `unwrap_or` 是否都带注释标记。

**验收**: 新增测试 `silent_fallback_audit` —— 源码扫描,未标注的
`unwrap_or` 数量必须为 0(或白名单)。

---

### S4 `let _ = <Result>` 丢弃错误(72 处)

**位置**: `grep -rn 'let _ = ' --include=*.rs crates/*/src | grep -v test`

**分类**:
- **可接受**: `yu-cli/src/output.rs` 写 stdout(EPIPE 时无可报)、
  `child.kill()/wait()` 清理路径、`create_dir_all` 幂等;
- **需挂号**: `yu-cli/src/dev.rs:312` `let _ = walk(www, &mut files, false)`
  (文件遍历失败会被吞)、`yu-cli/src/engine.rs:824`
  `let _ = self.take_save_diags()`(存档诊断被主动丢弃)。

**建议改法**: 逐条分类,只处理第二类。可加 `#[allow(clippy::let_underscore_must_use)]`
的显式豁免注释说明理由。

---

### S5 `_ => {}` 空中性分支(23 处)

**位置**: `grep -rn '^\s*_ => {}' --include=*.rs crates/*/src | grep -v test`

**风险点**(不是全部都要改):
- `yu-gfx/src/state.rs:468` / `471`: `Domain::Window` 以外的事件静默丢弃 ——
  若某版本新增域,会无声无息。
- `yu-rt/src/vm/handlers.rs:458` / `475`: `MOUSE`/`SAVEEND` 之外的
  `Domain::Host` 子命令落入空分支。
- `yu-sound/src/host.rs:84/103/106`: 已注释说明"通用域路径挂号",可接受。

**建议改法**: 对前两类,把 `_ => {}` 改成 `_ => { self.push_diag(...) }`,
detail 写清"未识别的域/子命令 + 实际名字"。第三条维持现状。

---

### S6 计数型护栏只报"截断"不报"截断了什么"

**位置**: `yu-gfx/src/state.rs:2816`(`push_copy_log`/`push_image_log`)、
`yu-cli/src/engine.rs` 的 `SOUND_DIAG_CAP`、`yu-rt/src/vm.rs` 的
`DECODE_CACHE_CAP`

**后果**: 日志到上限时丢一半最旧项,只留一条 `gfx.log.truncated`,
不知道丢了哪些 op。长会话下历史被无声抹平。

**建议改法**: 记录被丢弃的 `seq` 区间(`dropped_from..=dropped_to`),
并在报告里显式标出"此区间不可回溯"。

---

## 2. 已知的"良性静默"(不用改,但要能自证)

| 位置 | 行为 | 为什么良性 |
|---|---|---|
| `yu-gfx` `OpStatus::Unimplemented` | 跳过 op 并计入 `report.skipped` | **有计数**,只是缺位置(见 S1) |
| `yu-rt` `report_unimplemented` | 挂号到 `Diag` | 有码有 detail,位置在 VM 侧可补 |
| `yu-format` 的 `#[allow(dead_code)]` 保留字段 | 原样保留未解释字节 | 设计意图,有注释 |
| `yu-gfx/src/path.rs` `DEFAULT_BASE = b"W0"` | 手册规定的根路径 | 格式常量 |

---

## 3. 建议的推进顺序

| 序 | 项 | 理由 | 预估 |
|---|---|---|---|
| 1 | **S1** `FrameDiag` 带位置 | 一次性解决所有 gfx 跳过项的可定位性 | 小 |
| 2 | **S2** `as_int` 类型不符挂号 | 本次 LOGO 黑屏的直接成因,同类复发概率高 | 小 |
| 3 | **S5** 域/子命令空分支挂号 | 防未来版本新增域时无声失效 | 小 |
| 4 | **S3** 分类 + 只对类型不符加诊断 | 工作量最大,需逐条判定是否合法缺省 | 中 |
| 5 | **S6** 截断区间记录 | 长会话排查质量 | 小 |
| 6 | **S4** `let _ =` 分类 | 多数可接受,收益最低 | 小 |

---

## 4. 验收口径(整体)

改完后应能对**任意一次画面异常**回答这三个问题,而不需要临时写探针:

1. **哪条脚本的哪个 pc** 导致了跳过?(S1/S2/S5)
2. **为什么**跳?(诊断 detail 要区分"未给参数" / "类型不符" / "语义未取证")
3. **跳了多少次、涉及几个不同位置**?(计数与位置列表分开)

当前状态:问题 1 无法回答(gfx 侧),问题 2 部分无法回答(类型不符被降级成
"未给"),问题 3 只给了次数。这就是本 TODO 要消除的缺口。

---

## 5. 附:本次排查用到的临时手段(可复用)

为定位 LOGO 黑屏临时写了 4 个探针,已入库(`crates/yu-render/soft/examples/`):

| 探针 | 作用 |
|---|---|
| `probe_host_cmds.rs` | **挂宿主**的命令级 trace(按 sid/pc 区间过滤) |
| `probe_host_vars.rs` | **挂宿主**的变量导出(只打非零项) |
| `probe_host_query.rs` | 包装 `GfxHost`,记录每次 `CGINFO` 提问与答案 |
| `probe_at_pc.rs` | 命令边界采样变量(定位同帧内的写/清竞争) |

> 这些探针本身说明 S1 的缺口:需要手写工具才能补上"哪条 pc"这个信息,
> 而这个信息本该在诊断里。

---

## 6. 重要提醒:裸 `Vm` trace 不可信

排查中曾因此得出**错误结论**(`DIV-GFX-046` §H/§I/§J 已作废):

`yu-rt` 里未挂 `GraphicsHost` 时,`op_info` 对 `CGINFO`/`WINDOWINFO`/
`FONTINFO` 一律 `report_unimplemented` 且**不写回 `LET`**,脚本读到全 0
⇒ **控制流走岔**,trace 里的命令序列是假的。

任何按 INFO 返回值分支的脚本(`es_Sprite.yst`、`es_EF*` 族)都必须用
挂宿主的探针。若未来把"没挂宿主"本身也做成 S1 类的显式诊断
(例如 `host.missing` 且**中止**该脚本),可避免同类误判。

---

# 追加(2026-10-25):本轮实测到的「静默错位」类问题

> 起因:修 `VARINFO` 槽号时,我自己的重构引入了两个**静默错值** bug
> (`G_INT4` 整组不恢复、`TINT2` 值读成 0),没有崩溃、没有诊断、旧测试
> 也没覆盖。这类问题比"未实现"危险得多 —— 因为"未实现"至少有挂号,
> 而错位是**看起来跑对了**。本节记录已扫到的同类点与已加的防呆。

## S7 名字后缀被当成索引 ★已修,但同类风险仍在

**已发生的两次**:

| 位置 | 错误写法 | 后果 | 正解 |
|---|---|---|---|
| `VARACT[G_INTn]` | 后缀 N ⇒ 掩码 bit N | `G_INT4` ⇒ bit 4 被 `bit>=4` 丢弃 ⇒ **整组不恢复** | bit `N-1` |
| `TASK[TINTn]` | 后缀 N ⇒ 索引 N | `TINT2` ⇒ 索引 2,消费者读索引 1 ⇒ **值静默丢失** | 索引 `N-1` |

**共同根因**:名字里的数字是 **1-based 序号**,不是 0-based 索引。
项目里所有 `Pn` 族都遵循这个约定(`_PINT(n)`/`_RINT(n)`/`_TINT(n)`
消费侧统一 `n-1`,`sysvar.rs:404`)。

**已消除的结构性原因**:

- 新增 `family_index(name, prefix)`(`handlers.rs`),后缀 N ⇒ 索引 `N-1`,
  越界返回 `None`;
- `GOSUB` 与 `RETURN` 的参数槽位统一走 `param_slot_index()`
  (原本 `RETURN` **连 1..=16 边界守卫都没有**,`RINT0` 会 `usize` 下溢、
  `RINT17+` 会越界 panic —— 顺手补掉);
- 三条端到端回归锁住行为:
  - `fixture::varact_g_int_group_bits_follow_suffix`(G_INT 组位)
  - `fixture::task_tint2_value_reaches_tint2_consumer`(TASK → _TINT)
  - `fixture::name_suffix_families_are_one_based_everywhere`(RETURN → _RINT)

**已用"改坏验证"确认这些用例真的能抓错**(不是自我循环):

```text
把 family_index 改回「后缀 N 当索引 N」后:
  varact_g_int_group_bits_follow_suffix   left: 99  right: 2   FAILED
  task_tint2_value_reaches_tint2_consumer left: 0   right: 7   FAILED
把 param_slot_index 改成 n+1 后:
  name_suffix_families_are_one_based_everywhere left: 0 right: 42 FAILED
```

**仍存在同类风险的地方(待你排期)**:

| 族 | 位置 | 现状 | 风险 |
|---|---|---|---|
| `KEY_*` / `MOUSE_*` | `input.rs:76..88`、`sysvar.rs:237..264` | 名字→码,`_KEYNUM_n` 带数字 | 若某版新增 `_KEY_5` 之类,需确认数字是几基 |
| `PAD_*` | `sysvar.rs:352`、`handlers.rs:1221` | 读写两侧都传**原始 n**,换算在 `pad_dir_code` 内部 | 两侧同函数 ⇒ 当前无错位;但**没有端到端用例** |
| `DIMSIZEn` | `handlers.rs:1399` | 已用 `family_index`,后缀 N ⇒ 第 N 维 | 有 `varinfo_query_dispatch_follows_param_names` 覆盖 |

**建议的通用防呆(未做)**:对每个 `Pn` 族写一个**生产→消费往返**用例
(像本轮那三条),而不是只测单侧算术。这样任何人把后缀当索引都会红。

---

## S8 静默错值比静默跳过更难查 —— 需要运行期"值域断言"

**问题**:`TINT2=7` 存到错误槽时,**没有任何信号**。日志里看不到,
诊断里看不到,渲染结果也可能"看起来正常"(只是某个特效参数错了)。

**可加的料(按成本排序)**:

1. **`record_events` 打开时,`TASK`/`GOSUB`/`RETURN` 的落位记一条
   `VmEvent::TaskSpawn` 扩展**(把 `tints`/`args` 的实际索引与值一起发出)。
   现在 `TaskSpawn` 只有 `id/script/pc/enabled`,看不到 T* 初值。
   → 之后 `probe_host_cmds` 这类工具能直接看到"写进了第几格"。
2. **`prof_commands` 打开时,给每个 `Pn` 族统计"被写过的索引集合"**。
   如果 `TINT2` 永远只写索引 2、索引 1 从没被写过,就能自动发现偏移。
3. **调试构建 `debug_assert`**:两版 ysc 表都在场时,校验
   「名字后缀 == 消费侧该读的索引+1」,不一致直接 panic。

---

## S9 夹具与真实数据失真会掩盖错位(已修一处)

**发现**:`tests/support/mod.rs::task_params` 曾把真实 **38 项**的 `TASK`
参数表压缩成 **18 项**(只留 TINT 十项),而夹具用真实槽号(`kind=34` 是 `#`)
构造窗口。旧代码按槽号硬匹配真实布局,**恰好能过** —— 这让夹具与真实
数据的偏差完全不可见。

**已修**:按真实 38 项布局展开(`TINT2..TINT10`/`TFLT*`/`TSTR*` 全在)。

**同类待查**:其他被"压缩"的夹具参数表。
**建议的检查**:写个测试,断言夹具 `default_commands()` 里每个命令的
参数个数与名字**与真实语料 `ysc.ybn` 一致**(现在只有部分命令对得上)。
这条能一次性暴露所有夹具失真。
