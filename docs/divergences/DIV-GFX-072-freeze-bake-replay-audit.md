# DIV-GFX-072 · 命令时刻冻结/烘焙/重放路径审计(677d4bc、934f73f)

> 状态: 部分 fixed、部分 open(逐项标注)。发现日期: 2026-11-02/03。
> 触发: 主人复查存档缩略图与读档后发现“上一帧/上一场景”语义残留,要求按官方语义
> 逐项核对两个提交,不许“为修而修”的补丁。

## 0. 审计范围

- `677d4bc` fix(render): 主窗口截屏改为命令时刻快照,修复菜单消去隐藏与快照字体空白
- `934f73f` fix(render): 收敛已删临时层加工重放与整面清空后的整链重烘

真值口径: 官方引擎可观测行为 > SDK 0.495 二进制(REA) > 原版数据自证 > 旧线 golden。

## 1. 逐项结论

| # | 机制 | 官方语义/证据 | 我们的实现 | 状态 |
|---|---|---|---|---|
| 1 | 存档缩略图取哪一份像素 | `SAVE[DNO=3 ID=SLTEMP2]`;`SLTEMP2 ← COPY2(SLTEMP)`,`SLTEMP` 是场景绘制时 `COPY2 ID=":/W0/"` 的冻结层(`es_Scenario.yst:1175`)。官方 `SAVE[ID]` 读层表面(REA `0x4477c0`),不读“当前呈现帧” | `GfxCapture` 曾把 `screen_frame_for`(上一帧呈现面)放在解析链**最前**;玩家在存档界面停留后,上一帧已是存档 UI,缩略图存成 UI 截图 | **fixed**: 顺序改为 已物化镜像 → 复制链裁剪 → 上一帧 → 软回放。真机日志 `[caption] SAVE[ID] snapshot=… branch=chain frozen_snapshots=6` ×7 |
| 2 | 已烘焙前缀的失效条件 | 后端 `base_token` = 图层 id + **alpha** + 基图身份;alpha 变化会清表面(`surfaces.rs::sync_base`) | `baked_images` 只比较 `layer.texture`,alpha 淡入淡出时旧加工前缀被误判“已烘焙”而跳过 ⇒ 淡出期层上加工结果丢失 | **fixed**: 烘焙键加入 `layer.alpha`;`create_image` 重建同名层时清烘焙记录(注释此前声称会清,代码没清) |
| 3 | 主窗口截屏的合成底色 | `COPY2 ID=:/W0/` 复制窗口后缓冲,**包含清屏色**;官方黑屏场景缩略图 alpha=254/255 | wgpu `screen_snapshot_content` 给合成 offscreen 的 `clear: None` ⇒ 透明底;没有满屏图层的场景(选择支/黑背景)缩略图 alpha=0,存档界面显示“没有背景” | **fixed**: 合成 pass 用 `frame.clear`(黑底不透明)。实测数据:ours 6970/7500 像素 alpha=0,官方同场景 0 个 |
| 4 | 按需合成组里的过渡 draw | `TRANSITION` 的 `bind_index` 必须指向最终 `FramePlan` 的 bind group 表 | `compose_group_content` 用临时 `Vec::new()`,组内 TRANSITION 的索引指向被丢弃的表 ⇒ 渲染 encode 越界 panic(`wgpu/src/lib.rs:1031`) | **fixed**: `SurfaceWrites` 携带全局 transition bind/buffer 表,组内 plan 直接写入最终表 |
| 5 | 整面填充后的历史项 | 官方整面写会覆盖目标全部旧像素;被覆盖的旧写入不再可观测 | 934f73f 只折叠“同参重复填充”,历史 `MERGE2` 仍留在日志;烘焙记录被清后整链重烘(β4 演出1 单帧 275 条) | **fixed**: 整面填充前删除该目标全部更早写项;保留“填充前被当源读过”的中间项 |
| 6 | 已删临时层加工链的发射 | 官方命令时刻就地写层;链的最终消费者一旦烘焙,临时层中间像素不再被读 | 934f73f 改为“仅当本帧存在未烘焙消费者才重放”;外字 `TX.GAIJI*` 每字 +3 条永久 pass 的爬升消失 | **fixed**(像素抽样 7 帧 SHA 与修复前逐位相同) |
| 7 | 屏幕快照契约未变时跳过合成 | 契约键相等 ⇒ 命令时刻画面 == 上一呈现帧,可直接用后缓冲 | 返回 `None`,由渲染器用 backbuffer;上游 `screen_frame_for` 同样只用上一帧 | **sound**(有判定依据);保留 `last_screen` 仅作“契约未变/首帧”兜底 |
| 8 | 已删源层被 COPY 读取时的加工链 | 官方 COPY 在命令时刻取层当前像素(含此前加工) | 复制项的 `source_texture` 只记录**基图**;源层同帧被 `CGEND` 时,渲染端回放可能丢中间加工。934f73f 的消费者闭包只覆盖图像项,不覆盖 COPY 消费者 | **open / needs evidence**: 构造“加工链 → COPY → CGEND”用例,对齐 wgpu/soft 像素后再改 |
| 9 | 文字与图像的命令序 | `CGACT[TEXT]` 在命令位置写入层表面;后续 `MERGE2` 读取的是**含文字的层像素** | gfx 把所有图像/复制 pass 排在前,`apply_text` 统一在层末;`cgact_image` 执行后还会 `clear_layer_texts(target)` | **open**: 外字缺字段的直接嫌疑,见字体待办(不是 677/934 引入,但被这两条路径放大) |

## 2. 残留风险(明确不当作已修)

1. `GfxCapture::replay_snapshot` 仍用**当前帧契约**做软回放;冻结像素物化失败时会取到读档
   时刻的画面。`branch=chain` 正常时不会走到,但缺一条“按冻结快照回放”的渲染端能力。
2. `last_screen` 只对“契约键相等”成立;若上游把快照契约与呈现帧混用(例如跨 tick 的
   `SAVE[ID]`),必须先物化冻结快照(当前顺序已保证优先)。
3. `screen_snapshots` 的 `retain(live)` 以复制项存活为准;长会话里保留 6 份整屏契约属于
   预期(每份对应一个仍存活的 `SLTEMP` 类复制)。

## 3. 验证记录

```text
# 存档分支(真机,2026-11-02 23:09)
[caption] SAVE[ID] snapshot=363 branch=chain frozen_snapshots=6
[caption] SAVE[ID] snapshot=369 branch=chain frozen_snapshots=6
... (共 7 次,全部 chain)

# 演出1 发射/pass(真机 optrace)
frame=2040 images=314 emitted=45 emit_inplace_dead=45 skip_baked=224
# → 每个外字 +3 条临时层 pass;934f73f 后同场景 emitted=0(文本静止时)

# 黑背景缩略图(存档块 DNO=3)
ours  save00010: px=7500 avg(R,G,B)=(5,9,11) alpha=0..255 a255=241 a0=6970
official save00004: px=7500 avg(R,G,B)=(2,5,8) alpha=254..255 a255=300 a0=0
```

## 4. 2026-11-03 追补:屏幕快照必须在采集帧物化,`frame()` 必须是纯投影

真机复现「读档到立绘场景 → 点设定淡出 → 闪出当前场景不存在的立绘」后,
`YU_SNAP_TRACE` 抓到的两处语义断裂:

### 4.1 旧屏幕快照被延迟到「消费者出现」才物化

- 采集帧 `CGS100/CGS1` 的 `COPY2 :/W0/` 可能没有真实目标层持有(脚本先把
  `CopyId` 存进变量,若干帧后 CGS 栈才恢复层引用)。旧实现只在 copy pass
  被发射时才物化;`[snapuse] frame=418 copy=346 … holds_layer=true` 显示
  首次物化已远晚于采集帧,此时 `contents` 是**当前场景**,快照被重算成
  「当前不存在的画面」。
- 旧 `live_snapshots` 只看当前层/本帧 copy pass,不看
  `ScreenSnapshot.snapshot_textures`;内部快照纹理被当垃圾清掉后,下一次
  被 CGS 栈恢复时只能重算(`[snapcompose] frame=2401 copy=346`,
  `[snapprune] ids=[346,360,…]`)。
- **fixed**:
  1. `live_snapshots` 沿 `screen_snapshots[*].snapshot_textures` 做传递闭包;
  2. 采集帧给未因真实目标层发射的屏幕快照追加内部合成 copy
     (`__screen_snapshot_<id>`),在命令序原位置物化,不写真实层表面;
  3. 采集当帧的屏幕快照生产者/契约不被 `CGEND` 删除;
  4. 最终屏幕快照纹理长期保留(内部复制链只在采集帧 +1 帧保活,
     物化后释放);
  5. `screen_snapshot_content` 对同名层已换图的层不再补当前 `contents`/
     常驻表面像素。
- 验证:同一 600 帧标题→读档界面场景像素 SHA `b2c2911c…` 与修复前逐位
  相同;整段真机会话 `pin_snap_tex=0`、`copy_pinned=0`,旧 copy 不再重算。

### 4.2 `frame()` 不是纯投影(同批事件两次调用帧内容不同)

- `frame()` 里的烘焙判定会写 `baked_images`;soft 先取帧、wgpu 再取帧时,
  第二次会把第一次刚标记烘焙的 `COPY→SIZE2` 跳过。单测实测:
  软后端 3 个 pass / wgpu 2 个 pass,GPU 报 `asset.size.unsupported`,
  立绘整层消失。
- **fixed**:按事件批次缓存整帧投影;任何事件或 gfx 状态写入使缓存失效。
- 回归:`size_after_copy_in_same_frame_keeps_snapshot_dims`、全部 wgpu
  对拍恢复绿;区域 COPY 测试改为「真实 blit 或零拷贝 UV 别名均可,像素
  必须与 soft 一致」(2b53786/934f73f 的区域别名优化)。

### 4.3 投影 ≠ 执行:烘焙前缀必须等后端成功回执(2026-11-03 审计)

- 旧实现 `frame()` 在发射图像加工项时直接写 `baked_images`;后端因源缺失/
  纹理缺失/skip 时并未写层表面,但 gfx 已当作烘焙 ⇒ 该层加工链永久停止。
- **fixed**:`frame()` 只收集 `pending_baked` 候选;soft/wgpu 把执行失败的
  图像/复制目标原始字节回传 `RenderReport.failed_targets`;`Engine` 在渲染
  成功后调 `GfxState::commit_baked(failed_targets)`,失败目标不提交、下一帧
  重试。`--no-render`/未渲染路径 `discard_baked`。
- 回归:`baked_prefix_waits_for_backend_ack_and_retries_failed_target`。
  官方语义依据:`CGACT/COPY` 在命令处理体内成功执行才写层表面
  (`FUN_0042f809` 尾部),不存在“失败后视为已执行”。

### 4.4 soft 屏幕快照必须携带已加工层像素

- soft `compose_screen_snapshot` 曾把 `texture_overrides` 整表 take 走,
  快照里的 `RECTPAINT/MERGE` 层退回基图。微场景实测:蓝色底图 +
  `RECTPAINT` 红 + `COPY2 :/W0/` → soft 快照得到蓝色(应为红色)。
- **fixed**:快照合成只把“快照层且同名层未换图”的加工结果临时放回;
  新增 `screen_snapshot_carries_image_processed_layer`。

### 4.5 已冻结 COPY 不再逐帧重放(性能,2026-11-03)

- 官方 COPY 在命令时刻执行一次;结果冻结后,只要目标层仍持有该快照,
  下一帧重放日志是纯 no-op。旧实现仍把 343 条 `copy_live` 复制项发成 pass,
  后端只能 `has_snapshot` 早退。
- **fixed**:`baked_copies`/`pending_copies` 沿用同一套后端成功回执:
  `dest.texture == Snapshot(id)` 且已成功冻结 ⇒ 跳过发射;失败目标下一帧重试。
- 实测(读档界面 600 帧):`copy_emit 78 → 1`、`copy_live 77 → 0`,
  `pin_snap_tex=0`、`copy_pinned=0`;像素 SHA 在同一会话内可复现。
- 回归:`frozen_copy_is_not_replayed_every_frame`。

### 4.6 样本名诊断硬编码清理

- 删除 `YU_GAIJI_OFF` / `YU_GFX_TRACE_TEXT` 里按 `GAIJI`/`MESW` 层名改变
  行为或过滤 trace 的分支。脚本层名不是引擎语义;相关取证只留在注释。

### 4.7 残留

- 早期自动存档仍出现一次 `[caption] SAVE[ID] target SLTEMP2 is the 1x1
  black placeholder`(同一 tick 内 `SLTEMP` 尚未由场景循环创建);后续
  手动/快速存档缩略图正常(DNO=3 7500 px,alpha=255)。属于 VM 任务顺序/
  存档时序问题,不是快照像素冻结链。
- DIV-RT-050 的 CGS 栈可见性(官方 reserve vs 内核按普通层合成)仍挂账。

## 5. 复查命令

```bash
cargo build --release -p yu-cli
YU_OP_TRACE=1 YURIS_CAPTION_TIMING=1 ./target/release/yu-cli.exe run <project> --mode desktop
# 日志: target/tmp/optrace.log
```
