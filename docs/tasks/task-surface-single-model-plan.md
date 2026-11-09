# 单表面渲染模型重构计划（初版 / 待审）

> 状态：**计划书，未动代码**。先给主人审查；确认范围后再开分支。
> 目标：把渲染器的“冻结内容 + 每帧回放 + 新建表面”改成接近官方的
> “每层一张当前表面 + 命令序就地写”，在**帧边界**上与官方等价；
> 同时保住存档缩略图、COPY2 快照、文本基面、资产别名等显式冻结语义。

---

## 0. 名词

| 名词 | 含义 |
|---|---|
| 当前表面 | 某层此刻的像素表面；写入会改变它，读它看到最新像素 |
| 冻结内容 | 显式拷贝出来的不可变像素（COPY2、文本基面、存档读回） |
| 层引用 | `(layer_id, version)`，读的时候解析到该层当前表面；用于 SIZE2/镜像/缩放 |
| 读集合 / 写集合 | 一帧内每个 op 实际读/写的层与版本，用于 hazard 判定 |
| retire | 旧表面退出使用，但可能还有本帧读者；帧末统一判定是否可复用/释放 |
| 帧边界等价 | 只要求一帧结束时的层状态和屏幕像素与官方一致；帧内某条命令执行时刻的中间态不作为契约 |

---

## 1. 背景：为什么现在要改

当前模型（`crates/yu-render/wgpu`）：

- 像素动作**永不原地写**，每个写产出一张新纹理；旧内容以
  `Content { tex: Rc<TexRef> }` 冻结，保证“命令序第 N 步的读看到第 N 步的像素”
  （EVID-T-15）。
- 层状态常驻，每帧按 `CompositorFrame.passes` 回放；`applied` 前缀 +
  `Content.hash` 跳过已执行前缀。
- 为了避免每帧分配，后加了表面池（`release`/`acquire`）和原地复用
  `take_for_rewrite`（`surfaces.rs:518`；TRANSITION 在 `plan.rs:1166`，
  整层 RECTPAINT3 在 `plan.rs:1371`）。

问题：

1. **原地复用一次都没命中**。真机两把 `rw=0`；原因不是分支没走到，而是
   `SIZE2` 在 `plan.rs:1086` 用 `Content { tex: source.tex, .. }` 零拷贝别名，
   同一张 `TexRef` 被可见层和缓冲层同时持有，`Rc::strong_count >= 2`，
   `take_for_rewrite` 的守卫必然拒绝。守卫是对的，方向错了。
2. **补丁上打补丁**。每多一种读者（SIZE2 别名、COPY2 快照、文本 raw、
   bases/keepalive、MERGE 中间层），就多一条守卫；守卫越多，命中越少。
3. **分配 churn 仍在**。池只回收显式 `release` 的表面；没有别名接手的
   旧表面（部分 MERGE/滤镜目标）在 `set_content` 里直接 drop，下一帧重新建。
4. **峰值显存偏高**。真机 `live` 最高 114MB、`vram` 峰值 301MB，其中包含
   “旧表面为读者保活”的重复。

根因：我们在**回放模型**里用了**函数式不可变像素**。官方是即时执行 +
表面指针语义（`CGACT` 尾部 `*(iVar4+0x90) = local_68`），写完那一刻旧表面
没有未消费的读者，所以一层一张当前表面就够。

---

## 2. 证据与未决事实

已确认（可引用）：

- 官方 `CGACT` 在命令处理器里改目标层表面指针（`FUN_0042f809` 尾部）。
- 官方 `COPY2` 在命令处理器里立即写层表面；同 tick 的 `COPY2 → SAVE[ID]`
  能读到像素（`crates/yu-engine/src/capture.rs:293` 注释，`es_saveload`
  pc313/pc319）。
- 我们的 `SIZE2` 别名是纹理 Rc 共享（`plan.rs:1086`），不是层引用。
- 真机数据（`target/tmp/ktn/frames2.csv`，2177 帧）：
  - 1494/2177 帧有表面写；单帧最多 13 个 pass；
  - 立绘过渡链每帧 2 张：清色 2.1MB + lerp 17.8MB；
  - lerp 每帧约 53MB 读写流量，60fps ≈ 3.2GB/s；
  - `[res]` 采样 `created` 0–5/帧、池峰值 35–62MB、`vram` 峰值 301MB；
  - `rw=0`（全部采样帧）。

未决、必须先用 RE/trace 定死的语义：

| # | 问题 | 影响 |
|---|---|---|
| Q1 | `SIZE2` 在官方是“层当前表面指针”还是“生成时冻结”？后续写源层，别名是否立即看到新像素？ | 决定层引用 vs 冻结；错则偶发新旧帧像素差 |
| Q2 | 同一帧内 `COPY2` 与源层写入的先后，官方是否保证拷贝到写前像素？ | 存档缩略图/快照正确性 |
| Q3 | MERGE2/滤镜读的是目标旧像素还是源？读发生在写前还是写后？ | 能否就地写、是否要拆 pass/拷贝 |
| Q4 | 文本合成后，官方是否保留 pre-text 基面？重烘时从哪来？ | 文本层能否就地合成 |
| Q5 | 存档缩略图是命令时刻读层表面，还是帧末读？读的是当前表面还是冻结快照？ | 存档路径的正确性 |
| Q6 | 官方 `CGACT` 是否每次都分配新表面（`local_68` 的来源）？若分配，单表面只是“旧表面立刻释放”，不是零分配 | 收益预期与实现方式 |

**Q1–Q6 未定之前，不允许把对应路径切到就地写。** 这是本计划的硬门禁。

---

## 3. 目标模型

### 3.1 数据结构

```text
LayerSurface {
    tex: Rc<TexRef>,          // 当前表面（own，非资产）
    width, height,
    version: u64,             // 每次内容变化 +1
    usage: RENDER_ATTACHMENT | TEXTURE_BINDING,
}

LayerState {
    current: Option<LayerSurface>,
    frozen:  Option<FrozenContent>,   // 仅供文本基面/显式快照
    applied: Vec<AppliedOp>,          // (seq, 源版本快照)
}

enum ContentRef {
    Current(LayerId),         // SIZE2/镜像/缩放：读时解析当前表面
    Frozen(Rc<FrozenContent>),// COPY2/文本/存档：不可变拷贝
    Asset(TextureRef),        // 游戏资产：永不写
}
```

要点：

- 每层同一时刻只有一张当前表面；写入就是写它。
- `SIZE2` 不再克隆纹理 Rc，而是记 `Current(layer_id)` + UV。
- `AppliedOp` 记录该项依赖的源层版本；源版本变化 ⇒ 该项及其下游失效。
- `FrozenContent` 只由显式拷贝产生，不参与就地写。

### 3.2 动作分类（先按证据收窄）

| 类别 | 动作 | 处理 |
|---|---|---|
| 纯整层写 | TRANSITION、整层 RECTPAINT3 | 可就地写当前表面；若目标同时是源则禁止（自读自写） |
| 区域/alpha 写 | 区域 RECTPAINT、RECTPAINT2 | 需读旧基面：先拷贝或保证读 pass 在写 pass 之前；Q3 未定前保持新建 |
| 滤镜/模糊 | blur/filter | 通常读源写新；目标旧像素是否参与待 Q3；先保持新建 |
| MERGE/MERGE2 | 多源合成 | 读多源、整层写；是否可就地取决于 Q3；先保持新建 |
| 别名 | SIZE2、镜像、缩放 | 零像素写；`Current(layer)` 解析（Q1 门禁） |
| 显式拷贝 | COPY/COPY2、文本、存档 | 冻结；顺序必须早于后续就地写 |

**第一阶段只开 TRANSITION**，其余继续走现有路径。不要一次全切。

### 3.3 调度与 hazard

- 规划阶段为每个 op 记录 `reads: [(layer, version)]`、`writes: [layer]`。
- 编码严格按命令序；读 pass 必须先于会覆盖同一表面的写 pass。
- 同一 render pass 内禁止既采样又写同一纹理；冲突时拆 pass（wgpu 自动
  插屏障；拆 pass 的性能代价要实测）。
- 屏幕合成 pass 永远最后，读各层当前表面。
- 需要显式拷贝的地方（COPY2/存档读回/文本基面）在写之前生成冻结副本。

### 3.4 版本与前缀失效

- 每次就地写 `version += 1`。
- `applied` 前缀条目带源版本；任一源版本变化，下游已应用结果失效。
- `Content.hash` 不再混纹理池 id（池可能移除）；用 `(layer, version, uv, display)`。
- 前缀折叠（`supersede_*`）后，旧版本结果一律失效。

### 3.5 等价性声明

**契约：帧边界等价。** 不承诺帧内某条命令执行时刻的中间态；只承诺：

1. 一帧所有命令执行完后，每层当前表面像素与官方一致；
2. 屏幕合成结果逐像素一致；
3. 显式冻结路径（COPY2/存档/文本）在命令时刻取到的像素一致。

---

## 4. 影响面

| 文件 | 现状 | 改动 |
|---|---|---|
| `crates/yu-render/wgpu/src/surfaces.rs` | `Content` 冻结、`take_for_rewrite`、池、owned 集合 | 当前表面 + 版本 + frozen；删除原地守卫；池缩为“显式拷贝/快照”服务 |
| `crates/yu-render/wgpu/src/plan.rs`（3339 行） | 每 op 新建、`contents` Rc 视图、别名克隆 Rc | 读绑定当前表面、纯写就地、读写集合、版本失效；TRANSITION→RECTPAINT→MERGE/滤镜分阶段 |
| `crates/yu-render/wgpu/src/passes.rs`（1369 行） | FramePlan/Draw/写记录 | hazard 拆分、pass 顺序、屏障；绑定组按纹理 id（同表面就地写不需要换 bind group） |
| `crates/yu-render/wgpu/src/textures.rs`（957 行） | TargetSet/snapshot 缓存 | 快照仍显式拷贝；版本入 key；别名快照必须拷 |
| `crates/yu-render/wgpu/src/lib.rs` | 帧末 `retain_textures`、resource_snapshot | retire 队列、诊断口径（rw/pool/created 重定义） |
| `crates/yu-gfx/src/state.rs` | 只给 op 状态/参数 | 可选：给 op 补读/写层元数据；不改命令语义 |
| `crates/yu-engine/src/capture.rs` | 命令时刻 soft 回放/冻结镜像读缩略图 | 必须继续按命令时刻冻结；加“就地写前检查” |
| `crates/yu-save` | 存档块/缩略图存储 | 不改格式；验证缩略图字节与官方样本一致 |
| `crates/yu-render/soft` | 顺序执行，像素判据 | 不重写；共享类型若变，保持 API 兼容 |

不改：VM（`yu-rt`）、脚本/格式、声音、CLI、窗口宿主。

---

## 5. 专项风险清单（重点：会不会改坏存档/文本/快照）

### 5.1 存档缩略图（最容易坏）

现状：

- 游戏存档缩略图走 `COPY2` 快照 → `SAVE[ID]`；`capture.rs` 优先用冻结镜像，
  其次 `chain_snapshot`、上一帧呈现面、最后 soft 回放到命令时刻
  （`capture.rs:293`）。
- `capture.rs:334` 按层声明尺寸裁剪（`SLTEMP` 被 `SIZE2` 定成 100×75）。
- 历史 bug：存档界面停留后缩略图存成 UI 截图（2026-11-02），已由
  “冻结快照优先于呈现面”修正。

风险：

- 若 `COPY2` 的源是“当前表面”且随后被就地写，缩略图会读到写后像素；
- 若主渲染器的快照到帧末才物化，而 `SAVE` 在命令时刻读，必须保证 freeze
  发生在写之前；
- 若快照零拷贝别名当前表面（整帧未采样优化），就地写会直接改坏快照。

保护：

- COPY2/SAVE 路径强制 `FrozenContent`（真实拷贝），不允许 Current 别名；
- 在写集合里发现“本帧仍有未消费的冻结请求”时，先拷后写；
- 存档读回统一在命令时刻取冻结副本，GPU 侧 `poll`/flush 后再读；
- 增加字节级回归：官方样本缩略图 vs 我们写出的 `thumb.sd`，以及
  `COPY2→SAVE` 同 tick 用例。

### 5.2 快照缓存与零拷贝别名

- `TargetSet` 里可能把整帧/未采样的快照做别名；就地写前必须检查
  `snapshot` 是否引用该表面，引用则拷贝或延迟写。
- 快照 key 要带源版本；源变化后旧快照不能命中。

### 5.3 文本层与字形基面

- 文本合成需要 pre-text 基面（`raw`）；若基面就是当前表面且被就地写，
  重烘会丢底图。
- 保护：文本层始终保留一份冻结 raw；文本合成结果可以写当前表面，但
  raw 不参与就地写。
- 字形跨层复用（计划第 3 项）会共享字形图集；图集是只读资产，不参与
  当前表面写，安全。

### 5.4 资产别名

- 游戏资产（`TextureRef::Asset`）绝不入 owned 集合、绝不被就地写。
- 现有 `owned_textures` 守卫在重构后改成 `ContentRef::Asset` 分支显式拒绝；
  保留“去掉守卫测试必失败”的回归。

### 5.5 MERGE/滤镜的中间态

- MERGE2 可能把多个层的结果写到临时层，缩略图/存档会读这些层；
  就地写会让“已应用前缀”的下游结果过期。
- Q3 未定前保持新建；定了再按 op 逐个切。

### 5.6 离屏/合成组

- `TargetSet` 的 offscreen/composite 目标可能与层表面共享逻辑；
  就地写要确认没有别的 pass 还把它当输入。
- `prune_composites/prune_offscreen` 的生命周期要跟着版本走。

### 5.7 调试与取证

- 宿主帧步进器、`--capture-dir` 图层导出、`display_json`、`[res]` 快照
  都会持有表面引用；就地写可能让“事后导出”拿到新像素。
- 保护：导出路径在帧末/写前做冻结拷贝；诊断计数改为
  `created/released/retired/version-bumps`。

### 5.8 多帧过渡与在途 GPU

- 队列串行保证上一帧写完再提交下一帧；但读回/导出必须 `poll` 后再读。
- `SIZE2` 别名跨帧指向同一当前表面：下一帧就地写会让可见层立即更新，
  这是官方语义；要确认游戏没有依赖“旧别名保持到下一帧”的写法（Q1）。

### 5.9 引擎重启/换工程

- `LayerSurfaces::reset` 清空当前表面、版本、owned；`CopyId` 失效；
  重启后不得复用旧版本号（从 0 重新计）。

### 5.10 安卓

- 就地写要求表面 usage 同时含 `RENDER_ATTACHMENT | TEXTURE_BINDING`；
  创建时统一带上（当前若只带其一，会 validation 失败）。
- 屏障/拆 pass 可能增加 GPU 开销；收益要用 Android GPU 计时验证，
  不能只看 `created`。

---

## 6. 迁移阶段与门禁

| 阶段 | 内容 | 门禁 | 回滚 |
|---|---|---|---|
| P0 | 证据与审计：Q1–Q6；列全部读回/快照/存档路径；op 读写矩阵；补“当前行为”测试 | 文档 + 失败用例入库（不改行为） | 无代码 |
| P1 | 引入 `LayerSurface`/版本/frozen 结构，旧 `Content` 做适配层；不切行为 | 全量测试、像素零差异 | 开关关闭 |
| P2 | `SIZE2`/镜像/缩放改 `ContentRef::Current`（flag）；COPY2/文本仍冻结 | soft 全等 + 存档缩略图字节对拍 + koto trace | flag off |
| P3 | TRANSITION 就地写（flag）；读写集合 + pass hazard + 版本失效 | 23 surfaces + 39 wgpu + soft 全等 + koto 10 轮 | flag off |
| P4 | 整层 RECTPAINT3；Q3 通过后再 MERGE/滤镜 | 同上 + 安卓采样 | 逐 op flag |
| P5 | 删除 `take_for_rewrite`/owned 守卫/池的大部分；诊断与文档收尾 | 全量 + 真机长跑 | 分支保留 |

每阶段独立提交（本地，不推送），主分支可随时对照。

---

## 7. 测试与验收

功能：

- hazard：读后写、写后读、同 pass 冲突检测、自读自写拒绝；
- 别名：SIZE2→写源→可见层更新；SIZE2→快照→写源→快照不变；
- 存档：`COPY2→SAVE` 同 tick；存档界面停留后缩略图不是 UI；官方样本
  缩略图字节一致；
- 文本：写当前表面后重烘仍用 pre-text 基面；字形跨层复用不污染；
- 资产：资产别名永不被写（删守卫必失败）；
- 前缀：源版本变化后下游缓存失效，不出现“缩略图全是最后一张”。

回归：

- `cargo test -p yu-render-wgpu`（现 39 项）、`-p yu-render-wgpu --test surfaces`
  （现 23 项）；
- soft/wgpu 逐帧像素全等；koto 10 轮 22500 帧 0 OOM；
- 真机：标题循环、立绘差分、菜单、存档/读档、缩略图人工比对。

指标：

- 每帧 `created/released/retired/version_bumps`、`live`、`pooled`、`vram`；
- 过渡稳态 `created=0` 为目标；`vram` 峰值不高于当前 301MB；
- 桌面 render p99 不退化；安卓 GPU 时间/掉帧率改善（或至少不退化）。

---

## 8. 待主人决策

1. **范围**：只改游戏热链（TRANSITION/立绘/菜单）还是全 op 迁移？
   建议先热链，Q3 后再评估 MERGE/滤镜。
2. **存档缩略图口径**：是否要求命令时刻逐像素与官方一致（当前 capture
   已按此做 soft 回放）；这决定 COPY2 是否必须真拷贝。
3. **池的去留**：重构后保留小池服务显式拷贝/快照，还是全部按需分配？
4. **分支策略**：在 `spike/inplace-surfaces` 上做，每阶段本地提交；
   当前工作区未提交的第二轮改动先 WIP 提交或 stash。
5. **安卓目标机型/预算**：用于验收“收益是否成立”。

---

## 9. 粗估

- P0 1–2 天；P1 2–3 天；P2 3–5 天；P3 5–8 天；P4 5–10 天；P5 2–3 天。
- 合计约 3–5 周（单人、含真机验证），不含 Q1–Q6 逆向取证的不确定性。
- 最大不确定项：Q1（SIZE2 语义）与 Q3（MERGE/滤镜读写时机）。

---

## 10. 一句话

**帧边界等价 + 每层一张当前表面 + 只在官方也拷贝的地方冻结**；
先 TRANSITION 一条链验证，再扩 MERGE/滤镜，最后删掉双缓冲补丁。
存档/缩略图/文本/快照/资产是硬门禁，任何一条没对拍通过就不切。
