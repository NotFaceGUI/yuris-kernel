# 任务书 · 零风险 CPU 优化普查与落地（2026-11-28）

> 依据：5 路子代理静态普查（`yu-rt` / `yu-render`+`yu-font` / `yu-gfx`+`yu-platform` /
> `yu-format`+`yu-vfs`+`yu-save`+`yu-sound` / `yu-engine`+`yu-cli`+`yu-log`+`yu-trace`），
> 叠加 `docs/perf-audit-2026-11-25.md` 的实测口径与已完成结论。
>
> 本文件**只收录零语义风险项**：行为、输出、像素、日志/错误文本、迭代顺序、数值
> 全部逐字节不变，改动只涉及「容量预置 / 借用替代克隆 / 常量提升 / 重复查找合一 /
> refcount 往返」。结构性优化与需设计裁定的项集中在 §5「非目标」，**不在本轮排期**。
>
> 台账条目 `Z<批>-<序>` 是最小可勾销单位；批次 `Z0…Z7` 是**可独立开工、独立回归**
> 的提交序列。一个批次只动一个子系统。

> ⚠ **行号是 2026-11-28 工作树快照的锚点**。工作树存在并行改动（`yu-format/src/yscf.rs`、
> `yu-rt/src/sysvar_abi.rs` 等），落地前每条都必须重新 grep 定位，禁止直接按行号改。

---

## 0. 硬约束（违反即拒收）

1. **零可观测差异**：像素、字节输出、诊断码与文案、聚合计数、事件条数与顺序、
   迭代顺序、浮点运算顺序、溢出语义，全部保持现状。
2. **不改公开 API 语义**。签名的等价收窄（`&String`→`&str`、`&PathBuf`→`&Path`）
   只有在所有调用点仍能编译、且不改变借用存续期语义时才允许。
3. **不引入新依赖**，不改 `unsafe` 前提，不改既有 `#[inline]`/内联提示。
4. **不换哈希函数**（`FxHash`→`ahash` 之类）、不换分配器、不引入 SIMD——属需基准
   验证的投机项。
5. **不把参与输出迭代的 `BTreeMap` 换成 `HashMap`**（`text_draws`、`diags`、`windows`、
   `screen_snapshots`、声音混音层迭代）。
6. 同一文件不拆到多个提交；每个提交在 HEAD 上必须
   `cargo build --workspace --all-targets` 通过。
7. **未经主人明确指令，不执行任何 git 写操作。**

---

## 1. 状态口径

| 状态 | 含义 |
|---|---|
| `未开始` | 未动代码 |
| `进行中` | 已改，未过验证 |
| `已完成` | 过 §6 全部验证并回写台账 |
| `撤回` | 落地后发现等价性论证不成立，已回滚并记录原因 |

完成回写（与 `task-perf-2026-11-11.md` 同口径）：

1. 本文件对应条目状态改为 `已完成`，并记「日期 / 提交号」；
2. `PROGRESS.md` 追加「成果 N」，附验证命令与原始结果；
3. 若过程中发现新问题，**新增台账行**，不静默扩大本批范围。

---

## 2. 实测基线（用于收益校准）

### 2.1 已有实测（`perf-audit-2026-11-25.md`，release 本机）

| 命令 | 单价 | 典型单帧次数 | 单帧成本 |
|---|---:|---:|---:|
| `LET` | 0.07–0.12µs | 9.6 万 | 8–12ms |
| `IF` | 0.08–0.19µs | 7.4 万 | 6–14ms |
| `GOSUB` | 0.10–0.61µs | 2.4–2.7 万 | 3–5ms |
| `RETURN` | 0.10–0.32µs | 2.4 万 | 2–8ms |
| `LOOPEND` | 0.03µs | 5.0 万 | 1.5ms |
| `CGINFO` | 6.10µs | 1360 | 8.3ms |
| `CGEND` | 3.07µs | 1401 | 4.3ms |
| `FILEINFO` | 20.35µs | 50 | 1.0ms |

菜单开关帧实测 41281 次 `CG`、1401 次 `CGEND`；存档界面 541–1500 层；
v555 单轮诊断 6–9 万条 Verbose。

### 2.2 已完成、**不要重复提案**的项

`perf-audit-2026-11-25.md` A1–A5 已落地并复测（tick p50 1.435→0.968ms、长帧 39→23）：

- 逐 `GOSUB`/逐 `CG`/逐 op 的 `env::var_os` → `OnceLock`（`yu-rt`、`yu-gfx`、`yu-render` 内）；
- `op_info` 事件构造门控；
- `GfxState::frame()` 的 `by_id` 按需；
- `CGEND` 快照层集合按代次缓存；
- 逐字字形降级诊断门控。

**唯一遗留**：平台侧每帧 2 次 `env::var_os`（本任务书 `Z6-01`）。

---

## 3. 批次总表

| 批次 | 主题 | 覆盖条目 | 依赖 | 风险 | 预计收益 | 状态 |
|---|---|---|---|---|---|---|
| Z0 | 基线与验证设施校准 | — | — | 无 | 建立可比较口径 | 未开始 |
| Z1 | yu-rt 每命令主路径 | Z1-01…Z1-05 | Z0 | 低 | 每帧 5–20ms（v555 构建帧） | 未开始 |
| Z2 | yu-gfx 路径与帧投影 | Z2-01…Z2-03 | Z0 | 低 | 每帧 0.3–1.5ms；菜单 3–8ms | 未开始 |
| Z3 | yu-log 诊断洪水 | Z3-01、Z3-02 | Z0 | 低 | 每轮 6–9 万条时 5–15% | 未开始 |
| Z4 | yu-render / yu-font | Z4-01…Z4-03 | Z0 | 低 | 文本帧 0.3–1.5ms | 未开始 |
| Z5 | 格式 / 存档 / 声音 / VFS | Z5-01…Z5-06 | Z0 | 低 | 每次装载 1–8ms；存档单次省 8MB 拷贝 | 未开始 |
| Z6 | 平台 / 影片 | Z6-01、Z6-02 | Z0 | 低 | 每帧 0.2–2µs；播片每帧省 30 万次除法 | 未开始 |
| Z7 | engine / cli | Z7-01、Z7-02 | Z0 | 低 | 每帧 1 次 malloc | 未开始 |

> Z1–Z7 之间无相互依赖，**可并行开工**；Z0 必须先做（否则无法区分回归与既有红项）。

---

## 4. 逐条明细

### 批次 Z0 · 基线与验证设施校准

**目标**：把「当前工作树哪些测试是红的、为什么红」钉死，之后任何批次失败都能立即归因。

| 条目 | 动作 | 完成判据 |
|---|---|---|
| Z0-01 | 跑 `cargo test --workspace --no-fail-fast`，把失败清单与 `git rev-parse HEAD`、`git status --short` 一起存档到 `target/tmp/perf/baseline-2026-11-28.txt` | 文件存在，逐条标注「基线红 / 环境缺样本 / 待查」 |
| Z0-02 | 跑 `yu-cli case <project-eris> --all`，存基线 | 全绿，或明确列出既有红 case |
| Z0-03 | 跑 `yu-cli run <project-eris> --frames 600 --frame-stats`，存 p50/p95/max | 与 11-25 审计 §1.1 同量级（p50 ≈16.7ms） |

**已知基线红项（2026-11-28 观测，需 Z0-01 复核）**：

| 测试 | 现象 | 归因状态 |
|---|---|---|
| `yu-cli --test logging_guards::silent_fallback_audit` | `yu-gfx/state.rs` 47>32、`yu-rt/vm/handlers/control.rs` 6>5、`system.rs` 16>15、`value.rs` 4<5 | **基线红**：`println!`/`unwrap_or` 计数与 HEAD 逐字相同，属 `YU_SP_TRACE` 取证块与白名单不同步 |
| `yu-cli --test logging_guards::src_has_no_direct_terminal_macros` | `state.rs`/`control.rs`/`gosub.rs` 含 `println!` | **基线红**（同上） |
| `yu-gfx --test eris_first_screen` | `passes` 74 vs 83；`snapshot_reads` 索引与全扫描不一致 | **归因未定**：观测时工作树正被并行改动（`yscf.rs`/`sysvar_abi.rs` 均影响命令/系统变量表），必须 Z0-01 在冻结工作树上复核 |
| `yu-backend-wgpu --test wgpu::diffusion_permutes_blocks_on_gpu_and_matches_soft` | 多测试并发时偶发；单跑通过 | 判为 GPU 适配器竞争，**非回归** |
| `yu-engine --test export`（2 条） | 需导出样本环境 | 环境缺样本，非回归 |

> Z0 的存在意义：上述红项不清理，Z1–Z7 的任何失败都无法归因。

---

### 批次 Z1 · yu-rt 每命令主路径

#### Z1-01 · `LET` 左值窗口的注定失败求值（本任务书最大单点）

- **位置**：`crates/yu-rt/src/vm/handlers/mod.rs` · `Vm::eval_arg_windows`
  （现工作树约 494–536 行；插入点在 ref 槽分支之后、`if let Ok(v) = self.eval_window(st, w)` 之前）
- **热度**：per-opcode。`LET` 单帧 52.7 万次（场景构建帧实测）。
- **类别**：白付求值（含两次 `format!` 分配 + 一次 `Diag` 构造）

**现状**

```rust
for w in &cmd.windows {
    if w.rec.length == 0 { continue; }
    if w.eval_fails { continue; }          // 已有:静态「必失败」标记
    if ref_slots.contains(&w.rec.kind) {
        if let Ok(lv) = self.eval_lvalue_window(st, w) {   // 引用槽:保留变量身份
            let var = self.canonical_var(lv.var);
            if let Ok(v) = self.read_var(st, var, &lv.indices) {
                args.push(ArgValue::with_ref(w.rec.kind, v, var));
                continue;
            }
        }
    }
    if let Ok(v) = self.eval_window(st, w) {   // ← LET 左值窗口在这里必失败,结果被丢弃
        ...
    }
}
```

**改法**（插入点必须在 ref 槽分支**之后**）

```rust
        }   // ref_slots 分支结束
        // 单条 0x56/0x76(取变量引用)窗口的求值结果恒为 Slot::Ref:
        // 表达式机只压引用、不读变量也不写状态,eval_value_instrs 末尾必报
        // EXPR_MALFORMED(构造 Diag 里还有一次 r.display() 的 format!),
        // 而这里 `if let Ok(v)` 会把它整个丢弃。LET 的左值窗口就是该形态。
        if let Some([only]) = w.instrs.as_deref() {
            if matches!(only.op, 0x56 | 0x76) {
                continue;
            }
        }
        if let Ok(v) = self.eval_window(st, w) {
```

**等价性论证**（三层，缺一不可）

1. `expr.rs` 的 `Evaluator::step` 对 `0x56 | 0x76` 只执行 `stack.push(Slot::Ref(r))`，
   **不调 `access.read`**（对比同臂的 `0x48` 才 `access.read(r, &[])`）；
2. `eval_instrs` 入口即 `stack.clear()`，`eval_value_instrs` 末尾 `stack.pop()` 命中
   `Slot::Ref` → 返回 `Err(EXPR_MALFORMED)`；本处 `if let Ok` 丢弃该 `Err`；
3. `expr.rs:1210` 的既有文档已把该行为定为契约：
   「`0x56`/`0x76` 在参数记录时按旧行为求值失败、不产生 arg」。

因此跳过 ⇒ `args` 的元素与顺序逐项相同，无副作用。

**反例排查（已做）**

| 疑问 | 结论 |
|---|---|
| `w.simple` 快路径会不会接住 0x56/0x76？ | 不会。`simple_expr` 单指令分支只认 `one.op == 0x48`（`expr.rs:150`） |
| `w.const_value` 会不会命中？ | 不会。`const_value_instrs` 不折叠引用指令 |
| `w.instrs == None` 时会不会漏判？ | 不会。`instrs == None` 且 `payload` 非空 ⇒ `eval_fails == true`，前面已 `continue` |
| 会不会破坏 SAVE/LOAD/FILEINFO/SOUNDINFO 的引用槽？ | 不会。这些命令的 `ref_slots` 非空，其 lvalue 窗口在**上一分支**就 `args.push(...with_ref)` 并 `continue`；本插入点在其之后 |
| 数组左值（`0x56 … 0x29` 多指令）会不会被误跳？ | 不会。守卫要求 `[only]` 恰一条 |

**收益**：`LET` 每次省「求值 + Diag + 2 次 `format!`」。按 52.7 万次/帧估算，
即使单次仅 30–60ns，也在 16–32ms/帧量级（构建帧）。

**改动量**：1 文件，约 8 行。
**验证**：`case --all`（`record-events-on` 类用例）+ `--profile-commands` 观察 `LET` 单价下降。

---

#### Z1-02 · v555 系统变量读的无条件 `gosub_frames` 查找

- **位置**：`crates/yu-rt/src/other/sysvar.rs` · `read()` 开头（现约 22–27 行）
- **热度**：per-opcode（v555/0.500 运行面）。`@_LC` 语料 18,521 处，循环条件里逐次读。

**现状**

```rust
let name = vm.sysvars().get(var.id)?.name.as_str();
let frames = state                        // ← 无条件
    .gosub_frames
    .get(vm.current_task())
    .map(Vec::as_slice)
    .unwrap_or(&[]);
match name {
    "_PINT" | "_PFLT" | "_PSTR" => Some(read_pint(frames, st, name, indices)),
    ...
    "_STR" => Some(read_str(vm, state, st, indices)),   // ← 不用 frames
    "_INT" => Some(read_int(vm, state, indices)),       // ← 不用 frames
    _ => None,                                          // ← 不用 frames
}
```

`gosub_frames` 是 `BTreeMap<String, Vec<..>>`，按借用键查找 = **字符串比较的树下降**。

**改法**

```rust
fn task_frames<'a>(vm: &Vm, state: &'a State) -> &'a [super::gosub::GosubFrame] {
    state.gosub_frames.get(vm.current_task()).map(Vec::as_slice).unwrap_or(&[])
}
// match 内只在 _PINT/_PFLT/_PSTR、_RINT/_RFLT/_RSTR、_PFLG、_RFLG 四个分支调用
```

**等价性论证**：`get` 是纯只读查找（不写不插入）；`read_pint`/`read_rint` 只取
`frames.last()` 读数组，与本处调用位置无关；四个分支内取到的仍是同一个 `&[GosubFrame]`。

**影响面**：仅 v555/0.500；0.495 不走本模块。
**改动量**：1 文件（1 个私有 helper + 4 处调用）。
**验证**：v555 样本跑首屏到正文，对比 `--profile-commands` 中 `LET`/`IF` 单价；
golden 事件条数不变。

---

#### Z1-03 · `var_shape` 白克隆维数表

- **位置**：`crates/yu-rt/src/vm.rs` · `Vm::var_shape`（现约 2869 行）
- **调用点（只需类型，忽略 dims）**：`vm/handlers/variables.rs:876`、`:1014`、`:2127`、`:2152`；
  `vm/handlers/system.rs:321`；`vm/handlers/file_io.rs:417`
- **保留调用点（需要 dims）**：`variables.rs:562`、`:1087`、`file_io.rs:113`、`:1321`、
  `other/handlers/text.rs:131`、`other/handlers/product.rs` 数处
- **热度**：`VARINFO` 单帧可达 2 万次（源码注释自记）。

**现状**

```rust
pub(crate) fn var_shape(&self, var: VarRef) -> Option<(crate::value::ElemType, Vec<usize>)> {
    if var.id < self.sys_count {
        let d = self.sysvars().get(var.id)?;
        let ty = match d.value_type { 2 => Float, 3 => Str, _ => Int };
        return Some((ty, d.dims.iter().map(|&x| x as usize).collect()));   // ← 堆分配
    }
    if !var.space.is_legacy() {
        return self.store.shape(var).map(|(ty, dims)| (ty, dims.to_vec())); // ← 堆分配
    }
    let canonical = self.canonical_var(var);
    self.store.shape(canonical).map(|(ty, dims)| (ty, dims.to_vec()))       // ← 堆分配
}
```

调用点形态（全部丢弃 dims）：

```rust
if matches!(self.var_shape(t.var), Some((ElemType::Float, _))) { ... }
let ty = self.var_shape(target.var).map(|s| s.0).ok_or_else(...)?;
let (ty, _) = self.var_shape(target.var)?;
let Some((ty, _)) = self.var_shape(target.var) else { ... };
.var_shape(target.var).map(|(t, _)| t).unwrap_or(ElemType::Int)
```

**改法**

```rust
    /// 只取元素类型,不克隆维数表。分支与 `var_shape` 逐条同源。
    pub(crate) fn var_elem_type(&self, var: VarRef) -> Option<crate::value::ElemType> {
        if var.id < self.sys_count {
            let d = self.sysvars().get(var.id)?;
            return Some(match d.value_type {
                2 => crate::value::ElemType::Float,
                3 => crate::value::ElemType::Str,
                _ => crate::value::ElemType::Int,
            });
        }
        if !var.space.is_legacy() {
            return self.store.shape(var).map(|(ty, _)| ty);
        }
        self.store.shape(self.canonical_var(var)).map(|(ty, _)| ty)
    }
```

**等价性论证**：同一组分支、同一 ABI 表、同一 `store.shape`（返回
`Option<(ElemType, &[usize])>`，不分配）；只丢弃调用方本来就用 `_` 忽略的维数表。
**必须保留 `var_shape` 原样**给需要 dims 的 6 处调用点。

**改动量**：1 处新增 + 6 处一行替换。
**验证**：`VARINFO`/`TASKINFO`/`REGINFO`/`FILEINFO` 相关 golden 用例。

---

#### Z1-04 · `eval_*_window` 每窗口一次 `Rc` 引用计数往返

- **位置**：`crates/yu-rt/src/vm.rs` · `eval_window`（约 2925）、`eval_window_int`（约 2966）、
  `eval_lvalue_window`（约 3010）
- **热度**：per-window。`LET` 每帧 52.7 万次，每命令通常 2 个窗口。

**现状**

```rust
let instrs = match &w.instrs {
    Some(instrs) => Rc::clone(instrs),      // ← 只为两分支类型一致
    None => self.decoded_window(&w.payload)?,
};
```

**改法**

```rust
let decoded;
let instrs: &[crate::expr::RawInstr] = match &w.instrs {
    Some(instrs) => instrs.as_ref(),
    None => {
        decoded = self.decoded_window(&w.payload)?;
        &decoded
    }
};
// 后续 &instrs → instrs
```

**等价性论证**：`w: &CmdWindow` 是入参，不借自 `self`；`[RawInstr]` 的存续由
`cmd_cache` 里的 `Cmd` 保证；`decoded` 只延长 `None` 分支解码结果的存活期，
与今天 `Rc::clone` 取得所有权的时机等价。求值顺序、`eval_scratch` 的
take/放回顺序不变。

**反例排查**：`None` 分支里 `self.decoded_window(...)` 取 `&mut self`，
`decoded` 是独立局部，不与 `VmAccess { vm: self }` 冲突（编译器已验证语义可行）。

**改动量**：3 个相邻函数，每个约 5 行。
**验证**：全量 `cargo test -p yu-rt`；golden 事件一致。

---

#### Z1-05 · `TASK` 入口标签白克隆 `Label` 的两个堆字符串

- **位置**：`crates/yu-rt/src/vm/handlers/variables.rs` · `TASK` 的 `Some("#" | "")` 分支
  （约 1489 行），下游 `TaskSeed` 构造（约 1571）
- **热度**：每文本行（`es_Text.yst:1545` 的 `es.TX.OUTPUT.Z` 行末都 `TASK[RESTART=1]`）

**现状**：`self.find_label(&bytes)` → `get_bytes(name).cloned()`，克隆
`Label { name_bytes: Vec<u8>, name: String, .. }`；下游只用 `label.script_id` 与 `label.pc`。

**改法**：改用既有 `find_label_target(&bytes)`（`vm.rs` 约 3291 行，返回 `Copy` 的
`LabelTarget { script_id: u32, pc, base_a }`），`let mut label: Option<crate::vm::LabelTarget> = None;`，
`TaskSeed { script_id: label.script_id, pc: label.pc, .. }`（去掉冗余 `as u32`）。

**等价性论证**：`find_label_target` 就是对同一条 `by_name` 记录投影 `script_id/pc/base_a`，
命中判据是同一个 `get_bytes`；`label`/`base_a` 原本都不被 `TaskSeed` 使用。
失败文案逐字节不变。

**改动量**：1 文件 3 行。
**验证**：`TASK[RESTART=1]` 相关用例；正文推进时 `TASK` 单价。

---

### 批次 Z2 · yu-gfx 路径与帧投影

#### Z2-01 · 路径归一每次都付一次 `Vec` 中转

- **位置**：`crates/yu-gfx/src/path.rs`
  - `normalize` 快路径（约 61–67）
  - `normalize_bytes` 出口（约 109–120）
  - `parent`（约 131–133）
- **热度**：per-command。场景构建帧实测 41,281 次 `CG`。
- **本质**：`LayerId` 是 `SmallVec<[u8; 24]>`，常见 ID 整串 ≤24 字节，但当前先建
  `Vec` 再 `.into()` ⇒ **每次 1 次 malloc + 1 次 free 只为搬 ≤24 字节进内联存储**。

**现状**

```rust
let mut out = Vec::with_capacity(id.len() + 4);
out.extend_from_slice(b":/W0/");
out.extend_from_slice(id);
return ParsedPath { path: LayerId(out.into()), is_dir: false };
...
let mut out = Vec::with_capacity(bytes.len() + 2);
...
ParsedPath { path: LayerId(out.into()), is_dir }
...
if pos <= 1 { Some(LayerId::new(b":/".to_vec())) }
else { Some(LayerId::new(b[..pos].to_vec())) }
```

**改法**

```rust
let mut out = smallvec::SmallVec::<[u8; 24]>::with_capacity(id.len() + 4);
...
ParsedPath { path: LayerId(out), is_dir: false }
...
let mut out = smallvec::SmallVec::<[u8; 24]>::with_capacity(bytes.len() + 2);
...
let bytes: &[u8] = if pos <= 1 { b":/" } else { &b[..pos] };
Some(LayerId(smallvec::SmallVec::from_slice(bytes)))
```

**等价性论证**：`LayerId(pub SmallVec<[u8;24]>)` 字段 `pub`、同 crate 内构造；
`SmallVec::with_capacity(n)` 在 `n <= 24` 时**不分配**，`n > 24` 时一次配足
（与今天 `Vec::with_capacity` 同量级，无二次增长）；字节内容、`is_dir`、
返回类型全部不变。`LayerId::new` = `SmallVec::from_vec`，直接构造等价。

**改动量**：1 文件 3 处。
**验证**：路径归一单测（`path.rs` 内已有）；首屏 `CG` 序列 golden。

> 可选延伸（**不在本条目**）：`normalize_bytes` 的分段表 `segs: Vec<&[u8]>` 换
> `SmallVec<[&[u8]; 8]>`。慢路径，收益小，建议观察后再定。

---

#### Z2-02 · `frame()` 同一个 `nodes.get(&c.dest)` 查两次

- **位置**：`crates/yu-gfx/src/state.rs` · `GfxState::frame()`（约 1687–1694）
- **热度**：per-frame × 每条 `copy_log`

**现状**

```rust
let dest_holds = self
    .nodes
    .get(&c.dest)
    .is_some_and(|n| matches!(n.layer.texture, TextureRef::Snapshot(id) if id == c.id));
let dest_live = match self.nodes.get(&c.dest) {      // ← 同一个键再哈希一次
    Some(_) => dest_holds || needed_snapshots.contains(&c.id),
    None => needed_snapshots.contains(&c.id) || snapshot_layers.contains(&c.dest),
};
```

**改法**

```rust
let dest_node = self.nodes.get(&c.dest);
let dest_holds = dest_node
    .is_some_and(|n| matches!(n.layer.texture, TextureRef::Snapshot(id) if id == c.id));
let dest_live = match dest_node { ... };
```

**等价性论证**：`Option<&Node>` 是 `Copy`；`get` 纯读；两次调用之间 `nodes` 无写入
（本函数 `&self`，无 `RefCell`/`Cell` 内部可变），第二次查表必得同一项。
**注意**：若未来 `frame()` 改成 `&mut self` 并在两查之间写 `nodes`，本条目须重评。

**改动量**：1 处。
**验证**：`case --all` 的 `save-title-flow` / `until-visible`。

---

#### Z2-03 · `CGEND` 末尾两次临时构造 `:/W0`

- **位置**：`crates/yu-gfx/src/state.rs`（约 4691）
- **热度**：每次 `CGEND`；菜单拆卸帧 1401 次。

**现状**

```rust
if parsed.path == Self::w0() && !self.nodes.contains_key(&Self::w0()) {
    self.ensure_w0();
}
```

`Self::w0()` = `LayerId::new(b":/W0".to_vec())`（`state.rs` 约 7269），每次调用一次
`Vec` 分配 + 一次内联搬运。

**改法**

```rust
if parsed.path.as_bytes() == b":/W0" && !self.nodes.contains_key(&parsed.path) {
    self.ensure_w0();
}
```

**等价性论证**：`LayerId` 的 `==` 是字节比较；`&&` 短路顺序不变，进入第二项时
`parsed.path` 已等于 `:/W0`，因此用 `&parsed.path` 查表与用 `&Self::w0()` 查的是
同一个键（`FxHashMap` 的 `Eq/Hash` 只看内容）。

**改动量**：1 行。
**验证**：`CGEND` 相关 golden；菜单开关帧 `CGEND` 单价。

---

### 批次 Z3 · yu-log 诊断洪水

#### Z3-01 · `emit_diag` 的去重 key 每行 2 次分配 + `loc_display` 算两次

- **位置**：`crates/yu-log/src/console.rs` · `Console::emit_diag`（约 663–700）
- **热度**：per-diag。v555 单轮 6–9 万条 Verbose。

**现状**

```rust
let key = format!(
    "{}\u{1f}{}\u{1f}{}",
    diag.code,
    loc_display(&diag.where_).unwrap_or_default(),   // ← 分配 #1
    diag.detail
);                                                    // ← 分配 #2
match self.seen.get_mut(key.as_str()) {
    Some(entry) => { entry.count += count; entry.cached_line.clear(); }
    None => {
        let loc = loc_display(&diag.where_).unwrap_or_default();   // ← 同一值再算一次
        ...
    }
}
```

**改法**

1. `Console` 加字段 `key_scratch: String`，`new`/`with_writer` 两个构造点各
   `key_scratch: String::new()`；
2. `console.rs` 顶部加 `use std::fmt::Write as _;`（已有 `std::io::Write`，必须 `as _`）；
3. 函数体：

```rust
let loc = loc_display(&diag.where_);            // 只算一次
self.key_scratch.clear();
let _ = write!(
    self.key_scratch,
    "{}\u{1f}{}\u{1f}{}",
    diag.code,
    loc.as_deref().unwrap_or_default(),
    diag.detail
);
if let Some(entry) = self.seen.get_mut(self.key_scratch.as_str()) {
    entry.count += count;
    entry.cached_line.clear();
} else {
    let secs = elapsed_secs();
    let loc = loc.unwrap_or_default();
    let key: Box<str> = self.key_scratch.as_str().into();   // 仅新 key 分配
    self.seen.insert(key, AggEntry { severity: diag.severity, count, first_secs: secs,
        code: diag.code.to_string(), loc: loc.clone(), detail: diag.detail.clone(),
        cached_line: String::new() });
}
```

**等价性论证**：key 的字节内容、`\u{1f}` 分隔符、`AggEntry` 各字段值与原来逐字节相同；
`HashMap` 迭代序不参与任何输出（`summary_lines` 按 `count/code/loc/detail` 全序排序，
`agg_len` 只看长度）；`loc_display` 是纯函数，算两次与算一次结果相同。

**风险点**：新增字段会让 `Console` 尺寸变大（+24 字节），不改变任何行为。
**改动量**：1 文件（结构体 + 2 构造点 + 函数体 + 1 import）。
**验证**：`cargo test -p yu-log`（含 `line_format_matches_contract` 与聚合去重单测）。

---

#### Z3-02 · `format_line` 每行 3–4 个临时 `String`

- **位置**：`crates/yu-log/src/console.rs` · `format_line`（约 259–271）
- **热度**：per-line（`live` 输出且级别通过时；WARN+ 默认通过）

**现状**

```rust
out.push_str(&format!("[{secs:>+9.3}s]"));
out.push_str(&format!("\x1b[1;{color_code}m{word:<5}\x1b[0m "));
out.push_str(&format!("\x1b[{}m[{channel}]\x1b[0m ", channel_color(channel)));
out.push_str(&format!("[{secs:>+9.3}s] {word:<5} [{channel}] "));
```

**改法**：改 `write!(out, ...)`，格式说明符**逐字不动**（`>+9.3`、`<5`）。

```rust
let _ = write!(out, "[{secs:>+9.3}s]");
let _ = write!(out, "\x1b[1;{color_code}m{word:<5}\x1b[0m ");
let _ = write!(out, "\x1b[{}m[{channel}]\x1b[0m ", channel_color(channel));
let _ = write!(out, "[{secs:>+9.3}s] {word:<5} [{channel}] ");
```

**等价性论证**：`format!` 与 `write!` 走同一套 `core::fmt`；`String: fmt::Write`
永不失败（`let _` 与 `push_str` 一样不会 panic）；行字节完全相同。
**验收**：`console.rs` 单测 `line_format_matches_contract` 原样通过即证明字节一致。

**改动量**：1 处。
**验证**：`cargo test -p yu-log`。

---

### 批次 Z4 · yu-render / yu-font

#### Z4-01 · GPU 字形路径每 draw 重解码整串文本

- **位置**：`crates/yu-render/core/src/text.rs` · `collect_glyphs_skipping_missing`（约 1406）
- **热度**：per-draw / 逐字吐字时每帧
- **本质**：同文件 `paint_one` 早已用 `decode_display_cached`，注释明确
  「结果与 `Self::decode_display` 逐项相同（纯函数，只依赖隧道表）」，但 GPU 路径漏了。

**现状**

```rust
let (offsets, passes) = draw.composite_offsets();
let (chars, invalid) = self.decode_display(draw.codec, &draw.bytes);
if invalid > 0 { return Err("text.decode_invalid".to_string()); }
...
for ch in &chars {
```

**改法**

```rust
let (offsets, passes) = draw.composite_offsets();
let decoded = self.decode_display_cached(draw.codec, &draw.bytes);
if decoded.invalid > 0 { return Err("text.decode_invalid".to_string()); }
let chars = &decoded.chars;
...
for ch in &chars {
```

**等价性论证**：解码是 `(codec, 隧道, 字节)` 纯函数；缓存键带源字节逐字节复核
（`decode_fingerprint` + `hit.source == bytes`）；错误串与字符序列不变。
**注意**：`decode_display_cached` 取 `&mut self`，必须在 `self.renderer.as_mut()`
**之前**调用（现状顺序已满足）；缓存有上限（`DECODE_CACHE_MAX_ENTRIES`），
满了整体清空，只影响性能。
**副作用**：内存里多若干缓存项（与 CPU 路径同机制、同上限）。

**改动量**：1 处 + 循环写法（`for ch in &chars`，因为 `chars` 变成借用）。
**验证**：`cargo test -p yu-render-core`；wgpu↔soft 文本哈希对拍
（`text_layout_frames_match_soft_exactly` 等）。

---

#### Z4-02 · 组重绘为「回滚报告」克隆整份 `RenderReport`

- **位置**：`crates/yu-render/soft/src/render.rs`
  - `compose_screen_snapshot`（约 1146–1147，变量名 `saved_report` / `saved_skipped`）
  - `compose_group`（约 1256–1257，变量名 `report` / `skipped`）
- **热度**：per-frame × 每个 `COPY2` 组源 / 屏幕快照（按钮栏这类每帧都复制）

**现状**

```rust
let saved_report = st.report.clone();
let saved_skipped = st.skipped.clone();
for op in &pass.ops { draw_op(textures, snapshots, frame, op, &mut surface, st); }
st.report = saved_report;
st.skipped = saved_skipped;
```

`RenderReport` 含 `skipped: Vec<SkippedOp>`（每项 2 个 `String`）、
`failed_targets: Vec<Vec<u8>>`、`missing_assets`、`text_diags`；`st.skipped` 是
`BTreeMap<(&'static str, String, String), u32>`。

**改法**

1. `crates/yu-render/core/src/report.rs`：`RenderReport` 派生加 `Default`
   （`#[derive(Clone, Debug, Default, PartialEq, Eq)]`；全部字段 `u64/usize/Vec/String/&'static str`
   均可 `Default`）；
2. 两处 `clone()` 改 `std::mem::take(&mut st.report)` / `std::mem::take(&mut st.skipped)`。

**等价性论证**：克隆值**只用于回填**，期间无人读取。已逐条核对 `soft/src/render.rs` 中
`st.report.*` 的全部用法：`passes/ops_total/ops_drawn/offscreen_passes/skipped_ops_total`
是 `+=`，`missing_assets` 是赋值（在主流程、不在组重绘内），`text_diags` 是
`contains/push/sort`（写入，且结果会被回滚丢弃）。**无一处读取 `frame/viewport_width/
viewport_height/layers/filter_*/color_space`**。因此 take 走（留下 `Default` 占位）与
克隆回填的最终状态逐字段相同。

**反例排查表**

| 疑问 | 结论 |
|---|---|
| `text_diags.contains()` 在 take 后看不到既有项，会多 push 一条？ | 会，但这条 push 进的是**将被丢弃**的临时报告；回填后不存在 |
| `skip_op` 会不会读 `report.frame` 用作诊断序号？ | 不会；`skip_op` 只写 `st.skipped` |
| `merge_text_stats` 会不会读既有计数做增量？ | 不会；只做 `+=` |
| 组重绘抛错提前返回会不会丢报告？ | 不会；`draw_op` 无返回值，不存在提前 `return` 的路径；`?` 只在函数开头的查找处 |

**改动量**：2 文件（1 处 derive + 2 处 take）。
**验证**：`case --all` 的报告字段对拍（soft/wgpu 报告须逐字段一致）。

---

#### Z4-03 · `apply_rotation` 先克隆整图再写新缓冲

- **位置**：`crates/yu-render/soft/src/imagefilters.rs` · `apply_rotation`（约 56）
- **热度**：每次脚本 `CGACT[ROTATION]` 烘焙一次（冷-中）；1024² 约 4MB 拷贝。

**现状**

```rust
let src = texture.rgba.clone();
let mut out = vec![0u8; src.len()];
for dy in 0..h { for dx in 0..w { ... texel(&src, ...) ... } }
texture.rgba = out;
```

**改法**

```rust
// 搬运而非拷贝:此后只从 src 读,texture.rgba 要到函数末尾才写回。
let src = std::mem::take(&mut texture.rgba);
```

**等价性论证**：`texture.rgba` 在 take 点之后、赋值之前**没有任何读取**
（`src` 是唯一读者）；`src.len()` 与克隆版相同；`deg == 0.0` / `w==0||h==0`
的提前返回都在 take **之前**，不会留下空 buffer。
**注意**：同文件另有 3 处 `texture.rgba.clone()`（`apply_mosaic` 等），
**本条目只改 `apply_rotation`**，其余需各自论证。

**改动量**：1 行。
**验证**：`CGACT[ROTATION]` 像素对拍（wgpu/soft 哈希）。

---

#### Z4-04 · `GlyphImage::draw_onto` 每像素重算裁剪

- **位置**：`crates/yu-font/src/image.rs` · `draw_onto`（约 130–154）
- **热度**：per-glyph（`composite_offsets` 最多 9 pass 均调它）

**现状**

```rust
for sy in 0..self.height as i32 {
    let dy = y + sy;
    if dy < 0 || dy >= dst_h as i32 { continue; }
    for sx in 0..self.width as i32 {
        let dx = x + sx;
        if dx < 0 || dx >= dst_w as i32 { continue; }
        ...
    }
}
```

**改法**

```rust
let sy0 = (-y).max(0);
let sy1 = (dst_h as i32 - y).min(self.height as i32);
let sx0 = (-x).max(0);
let sx1 = (dst_w as i32 - x).min(self.width as i32);
for sy in sy0..sy1 {
    let dy = y + sy;
    for sx in sx0..sx1 {
        let dx = x + sx;
        ...
    }
}
```

**等价性论证**：被跳过的像素集合由 `0 <= sy < h ∧ 0 <= y+sy < dst_h`（及 x 同型）
唯一确定；等于 `sy ∈ [max(0,-y), min(h, dst_h-y))`。四类边界（`y >= dst_h`、
`y+h <= 0`、x 同型）逐一验证均为空区间或原区间。写入内容、顺序、混合公式不动。

**改动量**：1 处。
**验证**：`cargo test -p yu-font`；文本像素对拍。

---

### 批次 Z5 · 格式 / 存档 / 声音 / VFS

#### Z5-01 · YCG 两段 zlib 各自一个中转 `Vec`

- **位置**：`crates/yu-format/src/ycg.rs` · `decode` 的 `method == 1` 分支（约 150–166）
- **热度**：每张 method=1 的 CG/UI 大图（1920×1080 = 8.3MB）

**现状**

```rust
let mut decoder = ZlibDecoder::new(&bytes[offset..end]);
let mut decoded = Vec::with_capacity(raw);
decoder.read_to_end(&mut decoded)...?;
if decoded.len() != raw { return Err(... "YCG 第 {which} 段长度不符: 头部 {raw},实际 {}", decoded.len() ...); }
pixels.extend_from_slice(&decoded);
```

**改法**

```rust
let before = pixels.len();
let mut decoder = ZlibDecoder::new(&bytes[offset..end]);
decoder.read_to_end(&mut pixels)...?;
if pixels.len() - before != raw {
    return Err(... "YCG 第 {which} 段长度不符: 头部 {raw},实际 {}", pixels.len() - before ...);
}
```

**等价性论证**：`pixels` 顶部已 `Vec::with_capacity(expected)`，且上游已验证
`raw1 + raw2 == expected` ⇒ 两段追加都不触发扩容；`pixels.len() - before` 与
`decoded.len()` 是同一个数（同一段 inflate 的产出字节数），成功路径追加的字节序列
相同，失败路径的数字与文案逐字相同；错误直接 `return`，不泄漏半截输出；
`offset = end` 的推进规则不变。
**禁止**：不要顺手给 `ZlibDecoder` 加 `.take(raw)`——那会让损坏流的「实际 N 字节」
变成 `raw`，改变错误文本。

**改动量**：6 行。
**验证**：`cargo test -p yu-format`（webkit 含 YCG 合成用例）；真实 CG 像素哈希。

---

#### Z5-02 · 缩略图编码先造整图副本再拷进输出

- **位置**：`crates/yu-save/src/image_block.rs` · `encode`（约 147–152），副本来源 `swap_rb`（约 120）
- **热度**：每次 `SAVE[ID]` 缩略图；v555 缩略图可达 1920×1080×4 ≈ 8.3MB

**现状**

```rust
let mut out = Vec::with_capacity(8 + descriptor.len() + image.rgba.len());
...
out.extend_from_slice(&swap_rb(&image.rgba));
```

**改法**

```rust
out.extend_from_slice(&descriptor);
let tail = out.len();
out.extend_from_slice(&image.rgba);
for px in out[tail..].chunks_exact_mut(4) { px.swap(0, 2); }
out
```

**等价性论证**：容量已含 `rgba`，追加不扩容；`chunks_exact_mut(4)` 从切片首字节起
分块，与 `swap_rb` 内**完全相同**的分块与下标交换；长度非 4 倍数时尾部 1–3 字节
两者都原样保留。
**注意**：`swap_rb` 仍被 `decode`（约 213 行）使用，**不要删除该函数**。

**改动量**：1 处。
**验证**：`cargo test -p yu-save`（图像块 roundtrip）；
真实存档 `save00001.sd` 字节对拍（官方样本）。

---

#### Z5-03 · `snapshot_payload` 从 0 容量累积

- **位置**：`crates/yu-save/src/state.rs` · `snapshot_payload`（约 1679）
- **热度**：每次 `SAVE[FILE]` / `SAVE[MEM=n]` 的每个预约块（`seld` 类 2001 元素数组）

**改法**

```rust
// INT/FLT 每元素恒 8 字节;STR 每元素至少 4 字节长度头。
let min_per_elem = if snap.ty == ElemType::Str { 4 } else { 8 };
let mut data = Vec::with_capacity(snap.data.len() * min_per_elem);
```

**等价性论证**：只有容量变化；三种 `ElemType` 的分支判定、`to_le_bytes` 写入顺序、
类型不匹配写 0 的兜底全部原样；产出的 `YssdValue.data` 字节序列不变。
**风险**：`snap.data.len() * 8` 极端值溢出？——`usize` 乘法在 debug 下 panic。
需确认 `snap.data.len()` 有上界（存档块长度受文件大小约束）。**落地前必须核对**；
若无上界，改用 `saturating_mul`。

**验证**：`cargo test -p yu-save`；官方 `seld` 块读回自证。

---

#### Z5-04 · WAV PCM 从 0 容量累积

- **位置**：`crates/yu-sound/src/decode.rs` · `decode_wav_pcm`（约 290）
- **热度**：每次装载未命中 PCM 缓存的 `.wav`

**改法**

```rust
// len() 是头部 data 长度推出来的数,可能被伪造;每个样本至少占 1 字节,
// 所以用实际文件字节数夹住,避免损坏头触发巨额预分配。
let cap_hint = (reader.len() as usize).min(bytes.len());
let mut samples: Vec<i16> = Vec::with_capacity(cap_hint);
```

**等价性论证**：容量不参与任何数值计算，`push` 序列与元素值逐位不变
（`scale_to_i16` 的 `f64` 运算一字不改）；中途 `Err` 提前返回不受容量影响。
**必须夹上界**：`hound 3.5.1` 的 `WavReader::len()` = `data_len / bytes_per_sample`，
`data_len` 来自头部、可被伪造；不夹会 `with_capacity(4e9)` → 分配失败 abort，
属于**行为回归**。

**改动量**：1 行 + 注释。
**验证**：`cargo test -p yu-sound`；真实 WAV 装载样本。

---

#### Z5-05 · 声音层 ID 归一的 3 处 0 容量累积

- **位置**：`crates/yu-sound/src/path.rs` · `normalize` 快路径（约 45）、慢路径 `tmp`（约 54）、
  慢路径出口 `key`（约 102）
- **热度**：每条 `SOUND`/`SOUNDINFO`/`SOUNDEND`（`SOUNDINFO[TIME]` 类轮询每帧都可能发）

**改法**

```rust
// 快路径
let mut key = Vec::with_capacity(SOUND_ROOT.len() + 1 + id.len());
key.extend_from_slice(SOUND_ROOT);
...
// 慢路径 tmp(绝对 ID 分支的 rest 只是 id 的真子集,同一上界成立)
let mut tmp: Vec<u8> = Vec::with_capacity(SOUND_ROOT.len() + 1 + id.len());
...
// 归一键长度恒 <= tmp.len()
let mut key = Vec::with_capacity(tmp.len());
```

**等价性论证**：只改 `Vec` 容量，不碰任何字节、不碰 `segs`/`out` 的折叠逻辑、
不碰 `trailing_slash` 判定、不碰 `tmp.contains(&b' ')` 的 -1 分支。

**改动量**：3 行。
**验证**：`cargo test -p yu-sound`（`path.rs` 归一单测）。

---

#### Z5-06 · `LookupKey::new` 的归一缓冲预算

- **位置**：`crates/yu-vfs/src/key.rs` · `LookupKey::new`（约 38）
- **热度**：每次资产/声音路径解析（`Mount::open_path`、`VfsSoundLoader::load`、`YpfSource::open`）

**改法**

```rust
let components = split_components(raw, codec)?;
// 归一不增字节(折叠只删字符,分隔符统一成 1 字节)。
let mut bytes = Vec::with_capacity(raw.len());
```

**等价性论证**：归一化只做「分隔符统一 / ASCII 折叠 / 多字节整对跳过」，
输出长度 ≤ 输入长度（空组件被丢弃、`..` 弹出只会更短）。
**说明**：本条只是 1 行预算，收益有限（组件 `Vec<Vec<u8>>` 的每组件分配仍在）。
若需更大收益，走 §5「非目标」的「单趟归一」重写，但那属结构性改动。

**改动量**：1 行。
**验证**：`cargo test -p yu-vfs`（`key.rs` 单测 + `real_sample`）。

---

#### Z5-07 · `ArgReader::flag` 循环内 `format!` 构造裸名

- **位置**：`crates/yu-cli/src/args.rs` · `flag`（约 59）
- **热度**：冷（每次进程启动一次）；但属纯不变式提升。

**改法**

```rust
let eq = format!("--{name}=");
let bare = format!("--{name}");   // 与 toggle(约 76) 同形,移出循环
...
if self.tokens[i] == bare {
```

**等价性论证**：比较的字符串内容相同（纯循环不变量提升）；同文件 `toggle`
已是这种写法。

**改动量**：1 行。
**验证**：`cargo test -p yu-cli`。

---

### 批次 Z6 · 平台 / 影片

#### Z6-01 · 桌面宿主每帧 2 次 `env::var_os`

- **位置**：`crates/yu-platform/host-desktop/src/lib.rs` · `about_to_wait` 内
  `if std::env::var_os("YU_TEXT_TRACE").is_some()`（约 3180）与
  `if std::env::var_os("YU_FRAME_LAYERS").is_some()`（约 3390）
- **热度**：per-frame。注释写「默认零开销」，但 `var_os` 本身每帧仍取一次进程环境
  快照并扫描比较（Windows 上是 `GetEnvironmentVariableW`，约 0.3–2µs/次）。
- **这是 `perf-audit-2026-11-25.md` A1 的**遗留项**（该表列了这两处，但已落地批次只覆盖了
  `yu-rt`/`yu-gfx`/`yu-render`）。

**改法**

```rust
fn text_trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("YU_TEXT_TRACE").is_some())
}
fn frame_layers_trace() -> bool { /* 同上,YU_FRAME_LAYERS */ }
// 两个 if 改 text_trace() / frame_layers_trace()
```

**等价性论证**：环境在本进程内不会被外部改写（仓库内无 `SetEnvironmentVariable`），
缓存值与逐帧查询结果必然相同；两个块本身就是逐帧判定同一个常量。
与 `yu-gfx/src/state.rs:150-171`、`yu-render/core/src/text.rs:537-541` 的既有 `OnceLock`
写法同型。
**风险**：若有测试在运行期 `set_var` 后再跑宿主，OnceLock 会读到旧值。已 grep 全仓：
`YU_TEXT_TRACE`/`YU_FRAME_LAYERS` 无运行期 setter，**但落地时需再确认一次**。

**改动量**：2 处调用 + 2 个私有 fn。
**验证**：桌面起一帧，`YU_TEXT_TRACE=1` 下 `[text-trace]` 行数与改动前一致。

---

#### Z6-02 · pl_mpeg 转换的逐像素运行期整数除法

- **位置**：`crates/yu-platform/movie-plmpeg/src/lib.rs` · `convert_frame`（约 150–166）
- **热度**：per-pixel；640×480 一帧 30.7 万次 `div`（`advance_to(target, 4)` 追赶时单 tick 最多 4 帧）

**现状**

```rust
for y in 0..h {
    let row = y * w;
    let cy = if chroma_h == 0 { 0 } else { (y * chroma_h / h).min(chroma_h - 1) };
    for x in 0..w {
        let cx = if chroma_w == 0 { 0 } else { (x * chroma_w / w).min(chroma_w - 1) };
```

`chroma_w`/`w` 是运行期值，`x * chroma_w / w` 是非常量除数 ⇒ 每像素一条硬件 `div`。

**改法（A：整帧列表，表达式逐字照抄）**

```rust
let mut cx_lut: Vec<usize> = Vec::with_capacity(w);   // 或挂到宿主 struct 复用
for x in 0..w {
    cx_lut.push(if chroma_w == 0 { 0 } else { (x * chroma_w / w).min(chroma_w - 1) });
}
...
for x in 0..w {
    let cx = cx_lut[x];
```

**改法（B：无分配增量递推）**

`acc += chroma_w; if acc >= w { acc -= w; cx += 1 }`，起始 `acc = 0, cx = 0`，
对 `x = 0..w` 精确给出 `floor(x*chroma_w/w)`（`.min(chroma_w-1)` 仍保留）。

**等价性论证**：A 用逐字相同的表达式生成表，取值必然一致，遍历顺序仍 `0..w`；
B 是整除的精确增量形式（余数累积跨过 `w` 即进位），两者都与原式逐个相等。
BT.601 整数算式、`data[...]` 索引、`rgba[o]` 四字节写入全部不动，纯整数无浮点。
**收益**：每帧省 `w*h` 次除法（1080p 约 200 万次）。
**改动量**：1 函数。
**验证**：`movie-plmpeg` 探针（`examples/probe_mpg.rs`）逐帧 RGBA 哈希对拍。

---

### 批次 Z7 · engine / 每帧容器

#### Z7-01 · `capture::observe` 每帧重建 `live` 集合

- **位置**：`crates/yu-engine/src/capture.rs` · `GfxCapture::observe`（约 75）+ `retain_live`（约 111）
- **热度**：per-frame（`engine.rs` 每帧调 `observe`；`capture_bridge` 常态存在）

**现状**：`observe` 无条件 `retain_live(frame)`，后者把本帧全部 `Op::Copy` 的 `CopyId`
与快照层 `TextureRef::Snapshot` 收进 `BTreeSet`（O(n log n)），再对 `seen`/`pixels`/`wanted`
各做一次 `retain`。

**改法**

```rust
// wgpu 稳态:三个集合都为空时,retain_live 的三次 retain 与随后 wanted.retain
// 全是恒等变换,返回值必然为空 Vec。eager(soft)不能走这条:它靠遍历产生 fresh。
if !self.eager && self.seen.is_empty() && self.pixels.is_empty() && self.wanted.is_empty() {
    return Vec::new();
}
self.retain_live(frame);
```

**等价性论证**：仅在三集合皆空时跳过；此时三次 `retain` 作用在空集合上是恒等变换，
原路径返回的也正是空 `Vec`。非空时走原逻辑一字不改。`retain_live` 不触碰任何计数器
（`missing_pixels`/`snapshot_replays`），已逐行核对。

**改动量**：4 行。
**验证**：`cargo test -p yu-engine`；`SAVE[ID]` 用例（pixels/wanted 非空路径必须覆盖到）。

---

#### Z7-02 · 每帧 `take_events()` 丢弃事件 `Vec` 容量

- **位置**：`crates/yu-engine/src/engine.rs`（约 1805）+ `yu-rt/src/vm.rs` 的
  `take_events`（约 1228）/ `drain_events`（约 1233）
- **热度**：per-frame。`record_events=false` 时 `emit` 仍入队 `FrameStart/FrameEnd`
  与 sound/save/file 域调用 ⇒ 每帧至少一个非空 `Vec`。

**现状**

```rust
let events = vm.take_events();      // std::mem::take: VM 侧容量归零
self.events_drained += events.len() as u64;
// events 之后整只 drop
```

**改法**：`Engine` 加 `events_scratch: Vec<yu_rt::event::VmEvent>`，
用既有的 `vm.drain_events(&mut scratch)`（注释即「取走已排队事件并追加到调用方缓冲」），
之后 `scratch.clear()` 并放回。

**等价性论证**：`drain_events` 与 `take_events` 的唯一差别是内部 `Vec` 容量是否保留；
`events_drained` 计数、事件被丢弃的事实、`events_dropped` 记账（`emit` 里只看
`events.len()`）全部不变；此处的 `events` 只用来取 `len`。
**注意**：必须 `scratch.clear()` 后再放回，否则下一帧会重复计数。

**改动量**：2 处 + 1 字段。
**验证**：`cargo test -p yu-engine`；`--profile-commands` 的 `events_drained` 不变。

---

## 5. 非目标（本任务书**不做**，需另立任务书或主人裁定）

| 项 | 位置 | 为什么不在本轮 |
|---|---|---|
| soft `OwnedImage` 整图按值拷贝 → `Rc` | `soft/src/render.rs:1769/1818` | 每帧每过渡省整图 memcpy（1100×825 ≈ 3.6MB ×2），但约 40 行、新增 `ImageData` 枚举；需像素对拍 |
| soft `apply_layer_text` 每层全表扫文本 | `soft/src/render.rs:1540` | 存档界面 541–1500 层 × 全量 `text_draws`；要动 6 个 `draw_op` 调用点 + 3 处建桶 |
| soft `draw_image` 逐像素内层循环 | `soft/src/render.rs:2517` | 每像素去两次索引+两次边界检查；改内层循环需像素级对拍 |
| wgpu 每帧 `Vec<Vertex>` 复用 | `wgpu/src/plan.rs:362` + `FramePlan` | 要改 `plan()` 签名 + 渲染器字段 |
| `other::dispatch` 按名匹配 → opcode 表 | `other/handlers/mod.rs:20` | 启动期建表，仅 v555；两文件，需 boot 时机论证 |
| `param_name` → `Option<&str>` | `vm.rs:3044` + 3 调用点 | `media_info.rs:499` 处 `name` 借用会跨越 `&mut self`，必须重排语句 |
| Android YUV 逐像素 10 次饱和运算 → 4 次 | `movie-android/src/convert.rs:41` | 纯整数可证等价，但本机无设备验证 |
| `VmEvent` 的 `args`/`name` → `Rc` | `rt/vm.rs:3284`、`handlers/mod.rs:580` | **改公开事件类型语义**，下游全部消费点要动 |
| `CGINFO` 名称表 `position` → `match` | `gfx/state.rs:2729` | 1.7 万次/帧 × 28 项 strcmp；机械重排，收益待测 |
| `image_log` / `screen_contract_key` 增量哈希 | `gfx/state.rs:1437-1562`、`1763` | 发射语义敏感，须先加观测 |
| `LookupKey::new` 单趟归一（去掉每组件 `Vec`） | `vfs/key.rs` | 分配从 2n+1 降到 2，但要重写 `new` 并与 `split_components` 的错误顺序对齐 |
| 整屏 `screen → screen_surface` 拷贝 | `wgpu/src/lib.rs:869-873` | 3.7MB/帧（1080p 8.3MB）；需双缓冲渲染目标 |
| 读回兜底整屏 `Surface::clone()` | `wgpu/src/readback.rs:304-308` | 改 `Arc<Surface>` 共享 |
| `CGEND` 批量化 | `gfx/state.rs:6289-6367` | O(CGEND×image_log) → O(1) 次，需语义核对 |
| boot 期 5 份 YPF/VFS | `engine.rs:936/3214/3383/3391/3410` | 共享 `Arc<YpfIndex>`，改动跨 crate |
| `CGINFO[SX/SY]` 压缩条目整条解压 | `vfs/ypf.rs:997` | 只为读 4KB 头却整条 inflate；需前缀 inflate |
| `refresh_slots` 重读全部 `.sd` | `save/state.rs:641` | 只重解析刚写的文件 |
| `InputTimeline` 无界 | `rt/input.rs:329` | 滑窗淘汰，属行为边界变更 |

---

## 6. 已核实为误报，**禁止**列入优化

| 位置 | 结论 |
|---|---|
| `soft/src/text.rs:27/87` `resolve().cloned()` → `resolve_shared` | 无收益：调用方随后 `Rc::try_unwrap` 必然失败，仍要 clone 一次 |
| `core/texture.rs:891/1070` `ensure()` 的 `contains_key`+`get_mut` | 合成一次查找需要未稳定的 `raw_entry`，否则无法在 `load(&mut self)` 期间持 entry |
| `yu-font/cache.rs:166` `GlyphCache::get` 的 `lru.remove/insert` | 懒淘汰会改 `CacheStats.evictions`（进报告），非零风险 |
| `soft/src/render.rs:2359` `blend_pixel` 每像素 `mode.engine_effective()` | 小 enum match，几乎必然内联；提升出去收益小于噪声 |
| `movie-libav/src/lib.rs:352` 每帧 `vec![0u8; need]` | 缓冲交给 C shim 填写，`ret != 1` 路径无法证明写满，复用会改「未写字节 = 0」 |
| `movie-android/src/android.rs:682` 每帧 8MB `Vec` | 缓冲逃逸进 `Frame` 队列，复用需回收池，跨 3 个函数改状态机 |
| `SemanticProfile::is_v555_family()` | `u32` 比较，启动派生一次 |
| 图像扩展名探测（YDG/WebP/JXL/QOI/AVIF） | 失败结果已有缓存（`MAX_FAILED_CACHE_ENTRIES=4096`），非每帧 |
| `PRODUCT[COM=34]` 磁盘剩余空间 | boot 一次注入，查询只读字段 |
| `yu-emote/` | 不在 workspace members、引擎零引用 |
| 所有 `log::info!/warn!` | `log` 0.4 宏在级别关闭时不格式化，**不存在「先 format 再判级别」** |
| `tick_inner` 内 `.map_err(\|d\| format!(...))` | 惰性 |
| `update_status` | `report_json_frame` 已记忆化 |

---

## 7. 验证规程

### 7.1 环境（与 AGENTS.md §2 一致）

```bash
export TEMP="<side-repo>/workbench/tmp" TMP="$TEMP"
export CL='-utf-8' CFLAGS='-utf-8' CXXFLAGS='-utf-8'
export YURIS_SAMPLE_ERIS_YSBIN="<主数据入口>/data/ysbin"
export YURIS_SAMPLE_ERIS_ROOT="<主数据入口>"
export YURIS_SAMPLE_SDK0479="<side-repo>/target/sdk/0.479/…/システム"
export YURIS_SAMPLE_SDK0495="<side-repo>/target/sdk/0.495/…/システム"
```

### 7.2 每批必过

1. **编译**：`cargo build --workspace --all-targets`（零新警告）。
2. **单测**：`cargo test --workspace --no-fail-fast`，与 Z0 基线逐条比对——
   **不允许新增红项**；既有红项按 Z0 标注处理。
3. **golden**：`yu-cli case <project-eris> --all` 全绿。
4. **像素**（Z4/Z5/Z6 必加）：soft/wgpu 哈希对拍；`CGACT` 系列像素用例。
5. **帧率**（Z1/Z2/Z3 必加）：
   `yu-cli run <project> --mode desktop --frames 600 --frame-stats <csv>`，
   验收口径沿用 `perf-audit-2026-11-25.md` §5.2：
   - 稳态 p50 ≤ 16.7ms；
   - v555 正文 avg fps ≥ 58，长帧（>20ms）数量减半；
   - 菜单开关帧 `CGINFO` 单价 < 2µs、`CGEND` 单价 < 2µs。

### 7.3 提交纪律

- 一个批次一个提交（或按 crate 拆）；提交信息按 AGENTS.md §4.1
  `<type>(<scope>): <改了什么>`，不带任务名/批次号/成果号。
- 被依赖的底层先提交（如 `yu-render/core` 的 derive 先于 `soft` 的 take）。
- 若某条目落地后回归失败：**按 §1 标 `撤回` 并记录原因**，不留半成品。

---

## 8. 待主人裁定的问题（评审时请逐条给结论）

1. **Z1-01 是否接受「跳过单条 0x56/0x76 窗口」的等价性论证？** 该策略依赖
   `expr.rs` 既有契约（引用窗口在参数记录时必失败、不产生 arg）。若主人认为
   「必失败也要求值一次作为防御」，本条应改为「只省 `Diag` 构造」的保守版本。
2. **Z5-03 的容量上界**：`snap.data.len() * 8` 是否需要 `saturating_mul`？
   需要主人确认存档块的元素数上界（受 `.sd` 文件大小约束到什么程度）。
3. **Z6-01 的 `OnceLock` 风险**：是否接受「进程内环境不可变」这一前提？
   若存在运行期 `set_var` 的调试工作流，本条应改为「每 60 帧重查一次」。
4. **批次划分粒度**：Z1 是否要拆成 Z1a（Z1-01/02，v555 面）与 Z1b（Z1-03/04/05，
   全版本面）分别回归？
5. **§5 的哪几项有资格进入下一本任务书**？建议优先级：
   soft `OwnedImage` → `Rc` > `apply_layer_text` 分桶 > `CGEND` 批量化 > wgpu 顶点复用。

---

## 9. 附录 · 本文件的证据来源与局限

**证据层级**（按 AGENTS.md §0）：

1. 本任务书全部条目属**静态等价改写**，不涉及官方行为判定——不需要 trace/REA 锚点，
   论证方式是「代码语义等价性证明」（每条已给出）。
2. 收益量级来自：
   - 已实测单价：`perf-audit-2026-11-25.md` §1.3；
   - 代码自证频次：源码注释中已有的实测数字（如 `VARINFO` 2 万次/帧、`CG` 41281 次/帧）；
   - 结构性推算（如 YCG 一图 8.3MB、WAV 22 万样本）。

**局限（必须承认）**：

- 收益数字**未经本轮实测**，是量级估计；落地后以 `--frame-stats`/`--profile-commands` 为准。
- 行号锚点会随工作树并行改动漂移。
- 各条目之间的收益**不可简单相加**（同一帧内叠加后受内存带宽/分支预测影响）。
- Z6-02 的 A/B 两种改法只选一种，不同时落地。
