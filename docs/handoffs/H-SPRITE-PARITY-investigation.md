# 交接:立绘（sprite）位置/眨眼/卡顿三症状的取证与收口

- 日期:2026-10-26
- 状态:**部分收口**。已推送(`dfa7b0b`),工作区干净。立绘位置与卡顿已修;
  眨眼/换表情的「归位空白帧」**未修完**,§6 有完整机制与未闭环点。
- 语料:`<project-eris>`(data.ypf,官方 0.495 全量编译)
- 断言口径:✅ = 有 REA / trace / 逐帧像素证据;⚠️ = 判断但未闭环;❌ = 已排除

---

## 1. 一句话结论

用户报的三个症状是**三个独立根因**,不是同一个:

| 症状 | 根因 | 状态 |
|---|---|---|
| 立绘死在屏幕左上角 | `CG[… X+=…]` 的增量语义被当成赋值 | ✅ fixed |
| 移动像走像素网格 | quad 目标尺寸被取整(引擎是 16.16 定点) | ✅ fixed |
| 越玩越卡 | `image_log` 里同目标 op 逐帧累积,`frame()` 每项产一个 pass | ✅ fixed |
| 眨眼/换表情闪空白 | 过渡结束时 `CGEND` 删源层,`SIZE` 仍引用它 | ⚠️ **未修完** |

---

## 2. 复现方式

```bash
export TEMP="D:/Art/ai/projects/YurisDroid/workbench/tmp"; export TMP="$TEMP"
cargo build --release -p yu-host-desktop --example scenario_probe
cargo build --release -p yu-host-desktop --example probe_layers
```

**进场景**(标题 → β4機能紹介 → 演出機能 → 演出１):

```bash
cat > /tmp/sprite.json <<'EOF'
{"name":"sprite","entry":"SYSTEM_START","frames":2900,"backend":"wgpu",
 "inputs":[
   {"frame":300,"button":"MOUSE_L","x":217,"y":281},
   {"frame":500,"button":"MOUSE_L","x":400,"y":121},
   {"frame":700,"button":"MOUSE_L","x":400,"y":65},
   {"frame":900,"button":"MOUSE_L","x":400,"y":89}],
 "shots":[]}
EOF
python - <<'PY'
import json
d=json.load(open('/tmp/sprite.json'))
f=1050
while f<=2880:
    d['inputs'].append({"frame":f,"button":"MOUSE_L","x":400,"y":300}); f+=50
json.dump(d,open('/tmp/sprite.json','w'))
PY
./target/release/examples/scenario_probe.exe "<project-eris>" /tmp/sprite.json \
    --out target/tmp/sprite --frame-csv target/tmp/sprite/frames.csv
```

**看图层状态与 skip 原因**(本轮新增的取证工具):

```bash
./target/release/examples/probe_layers.exe "<project-eris>" 2860 2872 SPL601 \
    --click 300,217,281,0 --click 500,400,121,0 --click 700,400,65,0 \
    --click 900,400,89,0 --click 1050,400,300,50
```

`probe_layers <root> <from> <to> <id子串> [--click f,x,y,interval]`
逐帧倾倒:图层 `E/A/Z/xy/size/tex/status`、`SKIP <kind> <reason> <target>`、
`DIAG <severity> <code> <where> <detail>`。环境变量 `YU_SAMPLE_STEP` 控制步长。

---

## 3. 已修(均有证据)

### 3.1 立绘钉左上 —— `X+=` 是增量,不是赋值

**证据(REA,官方 0.495 `4f5eefa3…`)**

窗口记录 `+2` 的 u16 高字节就是官方**参数类型字节**:

```text
CG[… X=…]   窗口 slot = 1     (0x0001,种类 0 = 普通值)
CG[… X+=…]  窗口 slot = 257   (0x0101,种类 2 = 增量)
```

原实现用 `ParamKind::from_code(w.rec.kind)` 比对 —— `w.rec.kind` 是**槽号**
不是种类码,所以永远拿不到 `NumericOffset`,`offset` 恒 false。

**修法**:`crates/yu-rt/src/vm/handlers.rs:350`

```rust
arg.offset = w.rec.slot & 0xff00 != 0;
```

`crates/yu-gfx/src/state.rs` 的 `apply_layer_params` 按 `offset` 做 `当前值 + v`
(`X`/`Y`/`Z`/`A` 四个槽)。

**影响面**:`es_Sprite.yst:8173` 的
`CG[ID=$L1 X+=(@es.GSX/2)-(@WSX/2) Y+=(@es.GSY/2)-(@WSY/2)]` 是旋转画布补偿,
补偿量 0 时若按赋值就把位置清成 0。`ES_Title.yst:104` 的 `CG[ID=FOUT_B A+=4]`
同理(64 次循环淡入会恒为 4)。

**回归**:`crates/yu-gfx/tests/parity_fixes.rs::cg_numeric_offset_params_accumulate`
(零增量不清零 + 非零增量累加 + 负增量)。

> ⚠️ **文档冲突未解决**:`docs/divergences/DIV-GFX-064-cg-float-coordinates.md` §4
> 与 `PROGRESS.md` 的 MOTION-SPIKE-2 §4 都写着「种类 2 不是累加,按赋值处理」。
> 那是上一轮基于 `FUN_00429fd1` 参数序言的结论 —— 序言确实不写数值槽,
> 但**消费点**按 `0x683e20[i]` 这个偏移表读脚本体,再叠加到目标属性上;
> 「序言不写数值槽」推不出「语义不是增量」。本轮以实机行为
> (位置被清零、淡入失效)与窗口 slot 编码为准,两份文档需按本文勘误。

### 3.2 移动阶梯 —— 目标尺寸被取整

**证据**:引擎把 quad 尺寸按 16.16 定点保存(`docs/render-parity.md` §7:
`ROUND(v*65536)`),立绘缩放每帧只变 0.2–0.3px。

**修法**:
- `crates/yu-render/wgpu/src/passes.rs`:`Override::Uv.width/height` 由 `u32` 改
  `f64`;`push_quad_uv` 的宽高参数改 `f64`;新增 `Override::draw_dims()` 返回
  `(f64, f64)`,与取整版 `dims()` 并存(后者只用于按目标尺寸反查源)。
- 缩放分支不再 `dw.round() as u32`,只判 `dw < 1.0`。

**验证**:逐帧渲染 2650–2700 并做亚像素相关性分析,位移序列
`2.75 → 2.50 → 2.25 → 2.00 → 1.75 → 1.00 → 0.75`,连续无重复帧。

### 3.3 越玩越卡 —— `image_log` 同目标 op 累积

**证据(trace)**:β4 演出「ちなみに擬似的な３Ｄ空間を…」那句期间,日志
`[render spike] frame 1624..1638 render=30.5→40.1ms ops=150→164 passes=125→139`
—— passes 每帧 +1,layers/visible 不变。

**机制**:官方 `CGACT` 在命令时刻**就地执行**,结果写进层表面
(`FUN_0042f809` 尾部 `*(iVar4+0x90) = local_68`);本内核把像素工作延迟到帧末
回放,`image_log` 逐帧追加,`frame()` 给每条产一个 pass。

**修法(`crates/yu-gfx/src/state.rs`)**:

1. `supersede_image_op`:纯写动作(缩放族 / `HORIZONTAL` / `VERTICAL` /
   `TRANSITION`)按 `(动作, 目标)` 折叠;`TRANSITION` 额外不比 `source2`/`SET`
   (立绘淡入同帧连发数十条,像素只由最后一条决定)。原地过渡(源=目标)不再被
   「源≠目标」规则挡掉。
2. `supersede_full_paint`:整面 `RECTPAINT/2/3`(无区域)覆盖旧项;若目标被
   当源读过,只保留读取之后的最新一条。
3. `read_later` 判定:折叠项是 `TRANSITION` 时,同帧紧随的缩放族是它的**消费者**
   (`es_Sprite.yst:7978` 的 `TRANSITION` 之后立刻 `GOSUB es.CGACT_SIZE`),
   不构成「中间像素被引用」—— 这一条是修好卡顿的关键,漏了它单帧堆 29 条过渡 pass。

**验证(原始结果)**

```text
                        修前            修后
passes(2770→2860)   102→190(爬升)    102→101(平)
render_ms 峰值      105.8            4.2–6.4
frames >=16.7ms     p95 39.6ms       p95 11.1ms
```

### 3.4 `SIZE2` 定点源区域 —— `FX/100` 是补偿量不是坐标

`es.CGACT_SIZE` 只发定点键 `FX=@X1*100+@FLX`,其中
`@FLX = -(@LEFT_X*100 % 100)`(脚本注释:「@LEFT_X の小数点値を掃い」)。

⇒ `FX/100 = X1 - frac(LEFT_X)`,**真实源起点是 `LEFT_X + FX/100`**。
原实现直接当源坐标,UV 窗口整体偏移 `frac(LEFT_X)` 且逐帧变化 = 采样窗口在
纹理上来回滑动。

**修法**:`ImageOp::region_from_fixed` 标志 + `frame()` 输出前按目标层
`transform.x/y` 的 `fract()` 还原;位置可能在命令之后被 `CG[... X=...]` 改写,
所以在输出期解析而不用命令时刻快照。

### 3.5 `MERGE` 走 GPU

**证据**:REA `FUN_004405a4` 调通配绘制原语
`DAT_00801458(surface, …, X, Y, ratio, 0)` —— 一次带纹理 quad,不是逐像素循环;
`X/Y` 是**绘制偏移**,不是源区域(所以只给 `X/Y` 是合法的,不该判 region.partial)。

**修法**:
- `crates/yu-render/wgpu/src/pipelines.rs`:新增 `ensure_merge_pipeline(write_alpha)`。
  `MERGE` 用 `Normal` 混合 + alpha 分量 `One/Zero` 恒等 + `ColorWrites::COLOR`
  (只写 RGB);`MERGE2` 写全部。
- `passes.rs`:`PaintCmd.kind` 由 `alpha_only/color` 扩成枚举
  `PaintKind::{Solid, Texture}`;快照目标上的 `MERGE` 在 `plan_paint` 里产 GPU quad。
- 只有 `RECTPAINT` 族推迟到命令段(`is_paint` 判断),其余动作走 CPU 路径并要求
  快照基面。
- `resolve_base_texture` 新增 `Snapshot` 分支(`snapshot_pixels()`:优先冻结镜像,
  否则按产生它的 `CopyOp` 重建)。

**回归**:`crates/yu-render/wgpu/tests/wgpu.rs::merge_on_snapshot_layer_matches_soft`
(与 soft 逐像素一致 + RGB 被上层覆盖)。⚠️ 该测试只断言 RGB 与 soft 对齐,
**不断言绝对 alpha** —— 屏幕呈现会 `force_opaque` 把整帧 alpha 规范成 255,
alpha 语义只在快照层面可观测。

---

## 4. 已排除的方向(别再试)

- ❌ **不是 CPU 全帧渲染**。桌面宿主是 wgpu 在画;`readback` 中位 2.1ms 是
  `Renderer::render` 必须返回 CPU `Surface` 的接口约定(`RenderedFrame.surface`
  要给调用方写 PNG/SHA),不是浪费。
- ❌ **不是后端管线差异**。同一闪烁窗口 soft 与 wgpu 逐帧 diff 完全一致
  (`2724 diff=28.15` vs `28.24`)—— 数据层面就空,不是 GPU 画错。
- ❌ **不是 `MATH3D` 投影错**。逐帧 trace `lno=6` 输出
  `out_z=57.43→100.00` 单调、`y=167.90→70.00` 连续,透视正确。
- ❌ **不是 `@A *= (@S>=128)` 把 alpha 乘成 0**。`@L.H` 默认 256
  (`es_Sprite.yst:4483`),实测 alpha 在过渡期是 256。
- ❌ **不是 `X+=` 累加导致逐帧漂移**。位置序列 `240.9→240.93` 收敛且稳定。

---

## 5. ✅ 已收口(2026-10-27):归位那一帧立绘整层消失

> **本轮已修**。真正的断点不是「快照首建为空」,而是**规划阶段顺序倒置**:
> 图像加工预执行排在命令段之前,`SIZE2` 向尚未创建的 GPU 快照目标要尺寸,
> 报 `gfx.imageop.source.snapshot_missing` 并跳过整层。把快照纹理读回来
> 可以看到内容从第一帧起就是对的(创建帧与之后两帧哈希一致)。
> 修法:按命令序预扫描 `SnapshotDims`,`image_source_dims`/`plan_copy`/
> 镜像捕获共用同一份尺寸。详见 `PROGRESS.md` 成果 `SPRITE-SNAP-ORDER`。
> 下面的原始记录保留作取证留痕,**结论段落(第 3 点与"未闭环点")已过期**。

### 原始记录

**现象**:过渡结束的 1–2 帧,`SPL601` 状态正常(`E=1 A=256 size=295x531`),
但屏幕上是背景+文本框、**没有立绘**;下一帧恢复。

**逐帧证据**

```text
== host_frame 2865
  :/W0/SPL601  E=1 A=256 Z=8870035 xy=(240.866,69.945) size=Some(295x531) tex=SolidBlack1x1
  SKIP image gfx.imageop.source.missing          :/W0/SPL601 x1
  SKIP image gfx.imageop.source.snapshot_missing :/W0/SPL601 x1
== host_frame 2866   (同上,单条 source.missing)
== host_frame 2867   无 skip,画面恢复
```

**已确认的机制**:

1. 过渡期脚本走 `@L.ST != 0` 分支
   (`es_Sprite.yst:7972-7980`):`CGACT[TRANSITION=1 … ID=$LB1 ID2=$LB2 ID3=$LB3]`
   → `GOSUB es.CGACT_SIZE PSTR=$LB3 …` → `CG[ID=$L1 …]`。
   此时 `SIZE2` 的源是**临时层** `SPLBUFb601`。
2. 过渡完成(`@e_F2` 任务到 999,`es_Sprite.yst:2454-2485`)时脚本做
   `CGACT[COPY2=1 ID=$LIDBUF2+… ID2=$LIDBUF+…]`(把新图搬进数据缓冲)、
   `CGEND[SPLBUFa601]`、`CGEND[SPLBUFb601]`。
3. 检查项 `image_log` 里仍留着以 `SPLBUFb601` 为源的 `SIZE2`。该源已被删 ⇒
   `frame()` 输出这条 pass 时后端报 `gfx.imageop.source.missing` ⇒ 该帧整层跳掉。

**未闭环点(为什么折叠没兜住)**

`supersede_image_op` 已实现「同目标同动作的缩放族不比对 `source`、只保留最后一条」。
理论上稳态分支随后发的 `SIZE2`(源 `SPLBUF601`)应当顶掉过渡那条,但现场仍报错。
需要判定的是:

- 稳态分支在同一帧是否真的发 `SIZE2`?(2726 帧的 dump 里 `SPL601` 同时有
  `size2`(源 `SPLBUF601`)和 `size`(源 `SPLBUF601`)两条 —— 说明稳态确实发
  `SIZE2`,那么旧项应被顶掉);
- 还是说报错的那条 `SIZE` 属于**另一个目标**(`SPL601` 的 `source` 恰好也叫
  `SPL601`,`SKIP` 行只打 target,需要按 `source` 再区分一次)。

**下一步建议(按代价排序)**

1. 给 `probe_layers` 的 `SKIP` 行加上 `source`,一条命令就能区分上面两种可能。
2. 若属第一种:检查 `supersede_image_op` 的 `rposition` 是否被
   `push_image_log` 里的 `supersede_full_paint` 提前删掉条目导致位置错乱。
3. 若属第二种:真正的模型缺口是「图层表面身份」—— 引擎里加工结果直接成为层表面
   (`+0x90`),本内核的 `Layer.texture` 只是建层基图,加工结果只在 `image_log` 里。
   需要给 `Layer` 加「当前表面由哪条加工产出」的显式引用,`SIZE`/`COPY` 解析源时
   优先用它。**不要再用折叠/失效绕**(本轮试过 `drop_ops_for` 源失效、`SIZE` 跨源
   折叠、`TRANSITION` 产出快照身份,都没解决,已随提交带入但无实测收益,
   可在下一次收口时一并清理或补完)。

---

## 6. 本轮新增/改动的取证工具

| 文件 | 用途 |
|---|---|
| `crates/yu-platform/host-desktop/examples/probe_layers.rs` | 按 id 子串逐帧倾倒图层状态 + SKIP 原因 + DIAG 位置 |
| `crates/yu-gfx/examples/probe_cgact_params.rs` | 加了 `kind=` 输出(槽位→参数名→`ParamKind`) |
| `scenario_probe --frame-csv` | 逐帧 `layers/visible/passes/ops/executed/skipped/tick_ms/render_ms` |

---

## 7. 构建与测试状态(HEAD `dfa7b0b`)

```text
cargo build --workspace --all-targets    通过(无 error)
cargo test  -p yu-gfx -p yu-backend-soft -p yu-backend-wgpu --all-targets
                                         37 套件全绿(含本轮新增 2 条)
cargo test  --workspace --all-targets    在 yu-cli::logging_guards 失败
```

**失败的是一条白名单计数,不是功能**:

```text
crates/yu-rt/src/vm/handlers.rs: 未标注 103 > 白名单 102(新增未分类 unwrap_or)
```

`handlers.rs` 本轮(并行会话的 `TASKINFO` 9..13 槽实现)多了一处未标注
`unwrap_or`,但 `logging_guards.rs` 的白名单没同步。修法二选一:

- 把 `("crates/yu-rt/src/vm/handlers.rs", 102)` 改成 `103`(承认新增,
  仅当那处确实是良性兜底);
- 或给那处加 `// 良性:` 注释(推荐,保持白名单在收紧方向)。

---

## 8. 相关文档需同步

| 文档 | 要改什么 |
|---|---|
| `docs/divergences/DIV-GFX-064-cg-float-coordinates.md` §4 | `X+=` 结论由「不是累加」改为「增量」,补窗口 slot 编码证据(§3.1) |
| `PROGRESS.md` MOTION-SPIKE-2 §4 | 同上勘误 |
| `PROGRESS.md` | 补本轮成果(立绘三症状 + MERGE 上 GPU + 归位残留) |
| `docs/render-parity.md` §7 | `SIZE` 的 `FX/FY` 语义应写成「源坐标 − 位置小数」 |
