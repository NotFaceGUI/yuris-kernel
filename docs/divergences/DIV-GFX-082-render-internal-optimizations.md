# DIV-GFX-082 · rw 口径移除与渲染侧内部优化（非语义分歧）

> 结论：**不是与官方行为的分歧**。`rw`（目标旧表面原地复用）从未命中，
> 已删除；本轮回退到“写新表面 / 池取”的模型，并新增三项纯内部优化：
> 资产异步预取、COPY2 去重/延迟物化、跨层字形图集。像素与官方路径不变。

## 1. 为什么删除 rw

- 真机两轮采样（61 + 36 帧）`rw=0`。
- 根因：`SIZE2` 在 `plan.rs` 用同一 `Rc<TexRef>` 零拷贝别名可见层和缓冲层，
  目标表面 `Rc::strong_count >= 2`，`take_for_rewrite` 的守卫必然拒绝。
- 守卫是对的；把“官方 `CGACT` 原地写”硬塞进“每帧回放 + 不可变内容”的模型
  只会越加越多特判。**保留表面池**（`acquire`/`release`，单张 ≤24MB、
  池 ≤64MB）作为跨帧轮换手段：实测 `reused` 1/帧、`pooled` 峰值 2 张
  （小窗样本），过渡链的大表面仍按 24MB 上限入池。

## 2. 口径变化（诊断字段）

| 字段 | 处置 | 说明 |
|---|---|---|
| `rw=` | **删除** | 原地复用不再存在；脚本/文档不应再读取。 |
| `dd=` | 新增 | 同帧同源同区域快照去重次数（共享冻结纹理）。 |
| `dfr=` | 新增 | 本帧登记为延迟物化的快照数（当前帧无读者）。 |
| `pf=` | 新增 | 预取池：`命中h/解码失败e/占用MB/挂起p`。 |
| `atlas=` | 新增 | 字形图集：`页p/字形g/命中h/未命中m/淘汰e`。 |

关闭新开关（`YURIS_PREFETCH`、`YURIS_SNAPSHOT_DEFER` 不设）时：
预取 worker 不存在、延迟物化不登记；`dd` 仅影响相同不可变源纹理的共享
（像素逐位相同）。`YURIS_PREFETCH_WINDOW` / `YURIS_PREFETCH_MB`
只在 `YURIS_PREFETCH=1` 时有意义。

## 3. 正确性锚点

- 所有表面仍是“写入不原地”（`surfaces.rs` 不变量 1），快照/别名/文本基面
  的冻结语义不变。
- 快照延迟物化**版本固定**：登记时持有源 `Content`（`Rc<TexRef>` + `hash`），
  纹理不可变 ⇒ 后续帧补物化仍是 COPY2 命令时刻像素。
- `COPY2 → SAVE` 同 tick 与缩略图路径未改动；`capture_snapshot.rs` 4 项
  用例通过。
- 字形图集键 = 已合成字形的像素内容身份（宽/高/像素指纹），字体/字号/
  样式/亚像素/描边/阴影/旋转/配色均已烘进像素；`Rgba8Uint` + `textureLoad`
  整数读取，避免 UNORM 往返误差。

## 4. 验收证据

```text
cargo build --workspace --all-targets                  → 0 error
cargo test -p yu-backend-wgpu                          → 13+24+39+1+1 passed
cargo test -p yu-render-core                           → 23 passed
cargo test -p yu-engine                                → 32+4+1+2 passed
cargo test -p yu-rt (YURIS_SAMPLE_ERIS_YSBIN 已设)      → 91+4+11+4+115+3 passed
桌面 koto 2100 帧（默认/PREFETCH=1/DEFER=1 各一轮）     → 0 OOM
  frame 77（60 次 COPY2 构造帧）:
    默认       blits=42 dd=18 dfr=0
    DEFER=1    blits=2  dd=4  dfr=54
    渲染报告 JSON 与默认配置逐字节相同（77/120/500）
  PREFETCH=1   pf=16h/0e/19MB，报告 JSON 与默认逐字节相同
```

## 5. 残留

- 单表面模型（`task-surface-single-model-plan.md`）未执行；本轮不改变
  “每层一张当前表面”与“写新表面”的架构。
- 预取的脚本声明链扫描在 koto 样本上多取不到 `CG[FILE]`（场景在
  `es_scenario.yst` 的运行期子任务里），实际生效路径是 tick 末对
  **已声明 key** 的异步解码；静态前瞻保留为通用能力，默认关闭。
