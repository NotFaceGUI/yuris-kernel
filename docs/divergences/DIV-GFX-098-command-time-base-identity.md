# DIV-GFX-098 · 中间层同帧 `CGEND` 后 MERGE 链基面身份不一致,合成结果被丢弃

- 状态: fixed
- 优先级: 高(立绘换表情/耳朵动作时整帧露出旧差分:腮红消失、眼睛开合错误)
- 发现日期 / 发现方式: 2026-11-25 / `YU_GFX_CMD_TRACE` 连续命令流 +
  `YU_PASS_TRACE` 逐帧回放,`<game-c>` 实机

## 复现

- 游戏位置: `<game-c>` 任意对话场景,角色待机、无输入
- 锚点: 耳朵动作切显示层(`CG SPL602 E=1 A=256`)的那一帧,角色露出
  “睁眼、无腮红”的旧差分;动作结束回 `SPL601` 后恢复

## 命令流(`<game-c>`,s159 `es_sprite_cgl.yst`)

```text
pc51  CG      ID=SPLBUF602 FILE="" Z=1            ← 清目标
pc52  CGACT   ID=sp.tmpA  ID2=SPLBUF602 COPY2=1
pc54  CG      ID=sp.tmpB  FILE=...a_tou_4a_01     ← 眼睛差分基图
pc58  CG      ID=sp.tmpC  FILE=...a_tou_4a1701    ← 腮红差分
pc59  CGACT   ID=sp.tmpC  ID2=sp.tmpB X=0 Y=0 MERGE=1
pc60  CGEND   ID=sp.tmpC
pc63  GOSUB   es.SP.PRO.LOADCG.MERGE (ID=SPLBUF602, PSTR2=sp.tmpB)
      → CGACT ID=sp.tmpB ID2=SPLBUF602 X=0 Y=0 MERGE=1
pc64  CGEND   ID=sp.tmpB                          ← 消费者命令之后才删
```

`YU_PASS_TRACE` 同一帧全部发射:

```text
seq 2903 COPY  sp.tmpA → SPLBUF602
seq 2904 IMG   sp.tmpC → sp.tmpB   Merge
seq 2905 IMG   SPLBUF602 → SPLBUF602 RectPaint3
seq 2906 IMG   sp.tmpB → SPLBUF602 Merge
```

## 官方行为

`CGACT` 在命令体内立即执行(`FUN_0042f809`,EVID-T-15):`MERGE` 读上层
`sp.tmpC` 当前表面、写入下层 `sp.tmpB` 表面;随后 `CGEND sp.tmpC` 只释放
对象,不撤销已经写进 `sp.tmpB` 的像素。`MERGE sp.tmpB → SPLBUF602`
读到的是**合成后的** `sp.tmpB`。

## 内核行为(修前)

1. 生产者 `MERGE tmpC→tmpB` 的回放发生在帧末,此时 `tmpB` 已被 `CGEND`,
   `frame.layers` 里没有它;
2. wgpu `apply_image` 计算目标基面 token 时,只对**原地**动作使用命令时刻
   纹理,非原地且活层缺失时退化成 `None`(hash 常量 5);
3. 消费者 `MERGE tmpB→SPLBUF602` 的源 token 使用命令时刻 `source_texture`
   (`Asset(..._01)`,hash 1+名字);
4. 消费者在 `apply_image` 开头 `sync(src)`:`sync_base` 发现两者 token
   不同 ⇒ **清掉 `contents[tmpB]`**(刚合好的腮红结果);
5. `resolve_source(tmpB)` 退回命令时刻基图资产 `_01` —— 只有眼睛差分,
   没有腮红。

软后端没有这个分歧:它按帧内 `texture_overrides` 链式传递,同一序列输出
正确(回归测试的 soft 像素即参考值)。

## 根因

帧末回放下「命令时刻基面身份」在**生产端与消费端不对称**:消费端按
`ImageOp.source_texture`(命令时刻纹理)算 token,生产端对已删目标按
`None` 算 token。同一份层基面被哈希成两个值,内容被误判为“基面变了”。

## 修法

`crates/yu-render/wgpu/src/plan.rs::apply_image`:非原地动作的目标基面
token 不再退化成 `None`,改用 `ImageOp.target_base.texture`(命令时刻
记录的基面,与消费者 `source_texture` 同源)。原地动作仍用
`source_texture`。

## 回归

- `crates/yu-render/wgpu/tests/wgpu.rs::
  merge_chain_keeps_command_time_base_when_intermediate_is_deleted`
  - 修前: `gpu=[0,255,0,255] soft=[255,0,0,255] skips=[]`
    (GPU 取到中间层裸基图,软后端正确);
  - 修后: 通过;同一采样点 GPU/soft 误差 ≤2(alpha 混合量化容差)。
- `cargo test -p yu-backend-wgpu --test wgpu` → 40 passed;
- `cargo test -p yu-gfx --tests` → 各套件全绿。

## 取证工具(可保留)

- `YU_GFX_CMD_TRACE=1`:逐条打印 Graphics/Window/Movie 域命令
  (`[gfxcmd] f= s= pc= 名称 槽:值 …`),默认关闭、OnceLock 只读一次;
- `YU_PASS_TRACE=1`(既有):打印实际发射的 `[texop]` 回放序;
- 两者组合可在不对帧号、不依赖 F9 的情况下复现“随机动作”前后的完整
  写入顺序。

## 2026-11-27 · 活暂存层被 `COPY2` 改指后仍需回放旧快照

- `<game-sv>` 实机 gfx trace `f177 s26 pc657..680`:整图 `COPY2` 到暂存层
  (`pc658/664`),描边、填充分别执行 `MEIDO` (`pc665/678`),再按脚本给的
  坐标执行 `MERGE` (`pc670/680`)。截图所示已读行每字 `pc678 SET=0x34307C`,
  不是脚本把末字单独改色。同期 host trace `layout=0 draws=0`，本链全是图像命令。
- 官方锚点: `FUN_0042f809` 的即时表面语义与 `MERGE` (`0x43b33c`);
  该组脚本复用链无单独手册页。
- 修正: 替换暂存快照时保留已被下游图像命令读取的旧加工前缀。Soft/WGPU
  允许读取与命令时刻快照匹配的当前回放结果，不误用帧末暂存层的裸基图。
  不改变裁格外字、尺寸或着色公式。
- 像素回归 `reused_copy_surface_preserves_each_merge_sources_color`：修前首字
  `[200,160,120,255]`，预期 `[100,160,60,255]`；修后同帧复用、跨帧追加、
  重放三步 Soft/WGPU 均通过。已读/未读实机视觉复测待确认。

## 2026-11-27 · 已确认逐字表面的后缀回放

- 同一 `s26` 逐字命令依赖目标正文层保留上一次 `MERGE` 的结果。真实桌面
  帧在成功后确认已写前缀,下一帧只发后缀;WGPU 将缺失的前缀当成结构重写,
  清掉常驻正文层。修正为帧显式携带已确认终点,后端验证终点再续写。
- 旧暂存快照的原地加工不能自保活;只在仍有未执行读者或屏幕快照需要时发射。
  Soft 不持有常驻图像加工结果,仍接收全量回放。
- 同一像素回归加入真实 `commit_baked`:修前追加帧首字为黑;修后保留原像素。
  新增单字只发 `COPY2/MEIDO/MERGE` 三条,下一空闲帧零条。游戏实机耗时
  单独用 Release `--frame-stats` 核对。

## 2026-11-27 · 缺源复制不能阻塞其余字形确认

- `<game-sv>` f925 只清正文一次;f926–939 无正文重置却追加 119 个字形,
  其中 20 次读未创建的 `.sp` 源。`s26 pc508/509` 映射空格,`pc657/663`
  整面清透明,`pc658/664` 整图复制。缺资产 `CG[FILE]` 的无像素语义见
  DIV-GFX-080;这里依据显式清空,不把一般失败复制解释成透明。
- Release profile engine f3872:630 个纹理 pass,正文与两暂存层共 620 个,
  跳过 91 个操作,有 `copy:asset.missing`。缺源会让目标确认失败,导致已画
  像素被卷入后续重放。只给这个已知透明全复制生成可完成的快照;资产缺失
  诊断仍保留。裁格复制、解码失败和丢失的历史快照行为未改。
- `reused_copy_surface_preserves_each_merge_sources_color` 扩展为有色、缺资产、
  有色、未建源、下一帧有色。关闭规则时首字掉为黑底;恢复后 Soft 全量及
  WGPU 确认后缀逐像素通过,空闲帧零纹理操作。修后实机耗时待同场景复测。
- **勘误(2026-11-27 复核)**:“缺源读到无像素”仍是推断,不是官方 trace。
  可复现的只有内核自身缺陷:缺源 COPY2 会把源 ID 和父组凭空写进层表
  (`CGINFO[EXIST]` 由 0 变 1),以及把“清空后又写文字/又复制”的层误当空白。
  现在缺源不再建层;仅“最近一次全透明写之后无复制、无文字”的整图 COPY2
  才冻结 `source_texture = Some(TextureRef::None)` 并在两端按 1×1 透明完成。
  其它缺源保持失败重试。`crates/yu-gfx/tests/copy_missing.rs` 为反证:
  修复前三项全红,修复后全绿。部分区域缺源仍不按空白处理。
- **勘误(2026-11-27 复核)**:确认点重试。上一帧后缀部分失败时,后端会把
  常驻表面推进到更远的命令号,WGPU 下一帧清掉常驻表面重建,已确认像素
  丢失。现在只对 `failed_targets` 保留一份确认点纹理,下一帧从该点重写;
  成功即释放。回归 `acknowledged_prefix_survives_partial_suffix_failure`
  覆盖三个失败位置;Soft 侧“先取走上层/下层再解析源”也已改为先解析源,
  避免失败命令丢弃已合成像素。软/GPU 40 项表面用例通过。

## 2026-11-27 · `CG[FILE]` 改指后的逐字合成

- 状态: `fixed(no-manual)`（回放语义）；主人已确认桌面缺字修复。官方锚点:
  `FUN_0042f809` / EVID-T-15 即时写表面、`MERGE` `FUN_0043b33c`，整条字形链手册无页。
- `<v555-pc-es>` 原脚本 `yst00181.ybn pc7` 为 ASCII 正文，含三个显式 `\n`。
  s24 `pc717` 为 `es.GET.FONTCG.CXY`；`pc1179–1182` 为 `CG[FILE=""]` 重置
  暂存层、整图 `COPY2`、`MEIDO`、带 `X/Y` 的 `MERGE`。旧 `frames.jsonl:2`
  (`f2190`) 仅保留全角 `！` 的 `COPY2`、着色和 `(282,10)` 的合并；`f2625`
  也只保留正文层的末次合并。`f2661` 回看直接合并全角字形，正常可见。
- 换图原先同时删掉写入和读出同名暂存层的操作，导致较早的 `MERGE` 在帧末
  消失。与 `COPY2` 共用已有的 `CopyId` 依赖清理，保留被跨层消费者读取的
  旧快照及着色前缀；目标层自身换图仍清空原有合并，回收没有引用的生产者。
  不调整脚本坐标、资源路径、补丁编码或 TTF。日志中裸 `FONTCG.M2.` 缺源
  晚于上述可见缺字，未把它定性为字形映射问题。
- `scratch_file_reset_does_not_undo_finished_glyph_merges` 修前首字符位置为
  `[0,0,0,255]`，预期 `[100,160,60,255]`。修后 Soft 全量回放、WGPU
  提交后增量回放分别通过，覆盖同帧与跨帧、两行坐标、缺源和目标重置。

## 残余：待样本触发，不启动 LayerEpoch

本节是后续排查索引，不是已复现故障或频率统计。收到可复现的用户报告或取得新样本后，
先保留脚本 PC、命令顺序、分帧和像素证据，再决定修改范围。

| 命令形态 | 当前证据 / 可能症状 | 复现时优先检查 |
|---|---|---|
| 普通资产层直接 `MERGE` 到目标，帧末前对源层发 `CG[FILE]` | `yu-gfx::prune_rebound_image_ops` 源码确认：跨层 `ImageOp` 没有 `Snapshot(CopyId)` 时仍被删除。未做样本或像素复现；可能缺一笔合成。 | 两张纯色图，先合并、再换源，目标不换；核对同帧、跨帧和目标自身重置。 |
| `COPY2 → MEIDO → COPY/COPY2` 读走加工结果，随后重置原暂存层 | 当前保留旧加工前缀主要看跨层图像命令，普通复制作为唯一读者的分支未纳入本轮回归；可能读回未着色快照。源码风险，未复现。 | 隐藏暂存层调色后复制到可见层，再重置暂存层；检查可见层是否仍保留着色。 |
| WGPU 已确认加工结果，下一帧同名层、同一 `FILE` 和相同 `A` 再绑定，且无后续加工 | `wgpu::plan::base_token_of` 不记录换图代次；同一基图 token 可能沿用上一帧的加工像素。源码风险，未复现。 | 前一帧着色并确认，下一帧只重发相同 `CG[FILE]`，对比 Soft/WGPU 是否恢复裸基图。 |
| 过渡的 `to` 或遮罩输入在消费命令后同帧被替换或 `CGEND` | `ImageOp` 的 `source2/source3` 未记录与 `source_texture` 同等的命令时刻纹理身份。源码风险，未复现；可能读到新图或找不到图。 | 固定过渡中点和非对称遮罩，核对是否仍使用命令到达时的输入。 |

官方语义锚点仍为 EVID-T-15；回归先按命令形态写，不按语言列特判。若再次确认多个
分支同因，再评估 `docs/tasks/silent-gaps/task-04-layer-epoch-refactor.md`。
