# 文本表面语义与 seq 重放规范

- 状态: **partially implemented(2026-11-24)**:路线 E 的 `MERGE` 降级已落地
  (`lower_glyph_chain` 同时接受 `MERGE/MERGE2`,夹具
  `merge_glyph_chain_accumulates_text_images`);P0–P4 的通用 seq/矩形失效
  仍按本文推进。
- 目标: 用一份统一的表面命令模型覆盖所有文本路线,消除「图像加工即清文字」这类
  按动作类型打补丁的近似;同一模型同时服务 soft / wgpu / 快照。
- 关联: DIV-GFX-058(自动文本表面)、DIV-GFX-072 §9、DIV-GFX-076(外字链)、
  DIV-GFX-055/056(布局度量)、DIV-GFX-094(CGFILE)。

## 1. 官方语义(唯一真值)

官方每个图层持有一块**像素表面**;`CG`/`CGACT` 在**命令时刻**立即执行:

| 命令 | 对表面做的事 | 对已有文字的影响 |
| --- | --- | --- |
| `CG[FILE=…]` / `CGEND`+`CG` | 重建表面,装入新基图 | 旧表面上的文字全部消失 |
| `CGACT[COPY/COPY2]` | 从源表面取区域,写到目标(区域外清空) | 目标文字只在**写入矩形**内被覆盖 |
| `CGACT[RECTPAINT/RECTPAINT2/3]` | 填矩形(通道范围不同) | 同上,按矩形 |
| `CGACT[MERGE/MERGE2/XOR/AND]` | 把上层按落点合成进下层 | 落点矩形内覆盖/混合,矩形外保留 |
| `CGACT[SIZE*/ROTATION/RESIZE/TRIM]` | 整面重采样/改写 | 整面失效(文字随之缩放/裁切) |
| `CGACT[TRANSITION*]` | 两源按 SET 插值写目标 | 目标整面重写 |
| `CGACT[TEXT]` | 在目标表面 `(X,Y)` **叠加**字形 | 只增加像素,不删除其它文字 |

关键点:

1. `CGACT[TEXT]` 是**表面绘制**,不是独立图层;它和图像加工共享同一个像素面。
2. 图像加工作用于**包含此前文字**的表面;文字在图像加工之后画则不受影响。
3. 复制/快照在**命令时刻**取像素,包含当时已画的文字。
4. 因此正确语义 = **按命令序(seq)重放每个表面的写/读操作**,而不是
   “层末统一画字 + 图像加工就清全部文字”。

## 2. 文本路线矩阵(必须全部走同一模型)

| 路线 | 触发 | 官方落点 | 现状实现 | 缺口 |
| --- | --- | --- | --- | --- |
| A 自动文本 | `_` / `WORD` / `TEXT[FILE]` → `FUN_00404914` | `TEXT[ID]` 表面,逐字提交 | VM `TextState.steps` → gfx `text_surface`(DIV-GFX-058) | 与图像加工的相对顺序丢失 |
| B 手动文本 | `CGACT[TEXT SETSTR=…]` | 目标层表面,命令时刻叠加 | gfx `text_draws`(跨帧持久+去重) | 图像加工一律清空,过度失效 |
| C 按钮标签 | `es.BT.FONT.CG.DRAW.SET` 等 | 标签层表面,逐字叠加 | 同 B | 同 B;0.500 正文/名牌走此路线 |
| D 图像字形(外字) | `CG`+`SIZE2`+`MEIDO`+`MERGE2` | 文本层表面,逐字图像合成 | gfx `glyph_chains` + `TextDraw.image` | 与 B 混排时前缀被清(DIV-GFX-076) |
| E 图像逐字(0.500) | `es.SJISCODE.DRAW.ONE`: `CG` 字形层 → `MERGE` 到 `$0b15` 文本层 | 文本层表面,`X/Y` 为字位 | 纯 `ImageOp` 日志 | 目标层若被重绘/重建,历史 MERGE 丢失;无文字按序失效规则 |
| F 快照 | `COPY/COPY2` 源=层 | 命令时刻表面 | `CopyOp.texts` 冻结 | 仅冻结 B,不含 D/E/C 的合成结果 |
| G 截图文本 | `SAVE[ID]` / 屏幕快照 | 命令时刻屏幕 | `screen_snapshots` | 文本/图像顺序未统一 |

结论:**不能再按“文本类型”分支修补**;A–E 都是“往某个表面写像素”,只是
像素来源不同(字形 or 图像)。F 是“读当时表面”。统一模型必须同时表达写与读。

## 3. 统一模型:`SurfaceOp` + seq

### 3.1 数据

```rust
/// 与 CopyOp/ImageOp 共用同一个单调命令序。
pub struct SurfaceOp {
    pub seq: u64,
    pub layer: LayerId,          // 被写的表面
    pub kind: SurfaceKind,
    /// 写入/读取的层局部矩形;None = 整面。
    pub bounds: Option<Rect>,
}

pub enum SurfaceKind {
    /// 字形绘制(A–D 的共同落点)。TextDraw 已含 x/y/style/bytes;光栅化结果缓存。
    Text(TextDraw),
    /// 图像加工(含 MERGE/COPY2/Paint/Scale…),引用现有 ImageOp/CopyOp 记录。
    Image(ImageOpRef),
    Copy(CopyId),
    /// 重建/清空表面(CG[FILE]、CGEND、整面 RECTPAINT3、TRIM 等)。
    Reset,
}
```

- `TextDraw` 增加 `seq` 与 `bounds`(`x,y,sx,sy` 或逐 glyph 度量)。
- A 路线的 `TextStep::Glyph` 在 `apply_text_frame` 时转成 `SurfaceOp::Text`,
  `seq` 使用 VM 命令序(与 CopyOp/ImageOp 同源);B–D 在 gfx 命令处理时直接入日志。
- 每个层维护 `Vec<SurfaceOp>`(按 seq 排序;全局日志可继续用 `image_log`,
  但文本项必须与图像项同序)。

### 3.2 规则

1. **写矩形失效**:新写 op 只让 `bounds` 内、`seq` 更小的文字失效;
   矩形外文字保留。`Reset` 使该层全部旧文字失效。
2. **读包含前缀**:`COPY/COPY2`/快照读取该层时,取 `seq < copy.seq` 的全部
   SurfaceOp 结果(含文字、含 D/E 的图像字形)。
3. **顺序不可重排**:同一层内按 seq 严格排序;不同层可并行。
4. **文字不改层属性**:`CGACT[TEXT]` 只画像素,不改尺寸/z/alpha/显示矩形。
5. **未取证动作**:仍记录 op,但标记 `Unimplemented`;不得因此清掉其它 op。

### 3.3 渲染重放(抽象)

```text
surface(layer, upto_seq):
    base = layer 的命令时刻基图 / 空
    for op in ops(layer) where op.seq <= upto_seq:
        match op:
          Reset      => surface = new transparent(size)
          Image(op)  => apply_image(surface, op)          // 现有像素算法
          Copy(copy) => composite(source_surface(copy.seq), region)
          Text(draw) => draw_glyphs(surface, draw)        // 字形缓存
    return surface
```

`surface(layer, ∞)` 即帧末可见结果;`surface(layer, copy.seq)` 即快照结果。
结果按 `(layer, upto_seq)` 缓存;同前缀复用,避免每帧重烤。

## 4. 迁移阶段(每阶段可独立回归)

### P0 · 记录补全(无行为变化)

- `TextDraw` 增加 `seq`(取自 gfx 命令序)与 `bounds`;
- `gfx.image_log` 与 `text_draws` 合并到同一 `SurfaceOp` 序列(或提供
  `surface_ops(layer)` 视图,保持现有输出不变);
- `CopyOp.texts` 改为“按 seq 过滤的 TextDraw 视图”;
- 新增诊断 `gfx.surface.ops`(layer/seq/kind/bounds),供对拍。

### P1 · 失效从“按类型”改为“按矩形+seq”

- `clear_layer_texts` → `invalidate_texts(layer, upto_seq, bounds)`:
  - `CG[FILE]`/`CGEND`/整面 `RECTPAINT3`/`TRIM`/缩放族:整面;
  - `COPY/COPY2`:写入矩形(区域外不清);
  - `RECTPAINT*`/`MERGE*`:落点/填充矩形;
  - 未知动作:整面(保守,但记录诊断)。
- 只失效 `seq < op.seq` 的文字;**op 之后**画的文字永不受影响。
- 这一步即可修复 0.500 逐字 `MERGE`/`COPY2` 被整层清空、外字链前缀丢失。

### P2 · soft 后端按 seq 重放

- `emit_image_subtree` 不再只按“层末文本”合成;把 `SurfaceOp::Text` 插入
  与其 seq 对应的位置,基于当前 surface 合成;
- 保留 `compose_layer_cached` 的整层缓存,key 加“前缀摘要”;
- 快照使用 `surface(layer, copy.seq)`,删除 `CopyOp.texts` 的特殊冻结。

### P3 · wgpu 后端对齐

- `plan.rs` 的 pass 构建按同一 `SurfaceOp` 序插入文本 pass;
- 文本 pass 的源=当前目标纹理,目标=新纹理(或复用 ping-pong);
- 缓存 key 与 soft 同构,保证两端像素口径一致。

### P4 · 清理近似与回归

- 删除 `supersede_full_paint` 的“重复整面填充折叠”与文本相关的特殊分支
  (保留纯写图像折叠,但必须保留其读前缀);
- 删除 `clear_layer_texts` 的所有调用点,统一到 `invalidate_texts`;
- 全量跑 ERIS 语料 + `<v500-relirium>` + 存档缩略图/按钮 golden。

## 5. 验收用例

### 5.1 合成(`yu-gfx` 单测)

1. `base` → `Text("A",x=0)` → `Text("B",x=40)` → 两者都在;
2. `base` → `Text("A",x=0)` → `RECTPAINT3(0..40)` → A 消失,B 保留;
3. `base` → `Text("A",x=0)` → `CG[FILE]` → A 消失;
4. `base` → `Image(MERGE glyph,x=0)` → `Image(MERGE glyph,x=40)` → 两个字形都在;
5. `base` → `Text("A")` → `COPY2` 源=该层 → 快照含 A;之后再画 B,快照不含 B;
6. `Text` 画在 `RECTPAINT3` 之后 → 不受 paint 影响(seq 顺序)。

### 5.2 真实样本

- `<v500-relirium>` `es.SJISCODE.DRAW.ONE`:一行 N 字全部可见;换行/换标签时
  旧行在重绘矩形内被清、矩形外保留;
- ERIS 外字链:`」` 前缀 + 外字 + 后缀同屏(DIV-GFX-076);
- 0.495 自动文本:现有 `text_surface` 用例逐字推进不变;
- 存档缩略图:`COPY2` 快照含命令时刻文字,后续文字不串入。

### 5.3 性能

- 每层 `SurfaceOp` 数上限与现有 `MAX_OP_LOG` 同量级;
- 文本光栅化按 `(bytes,style)` 缓存,重放不重复光栅化;
- 层表面缓存 key = `(layer, prefix_digest)`;菜单帧不因文本重放增加 pass。

## 6. 风险

- **wgpu 文本中间合成**:需要把文本 pass 插入图像 pass 链;若代价过高,
  允许该层回退 CPU 合成(仅文本层),但必须保持像素语义与 soft 一致。
- **未取证图像动作**:矩形未知时只能整面失效;必须在诊断里显式记录,
  不能静默近似。
- **历史日志上限**:`MAX_OP_LOG` 淘汰最旧项会破坏 `seq` 前缀;淘汰时必须
  同时作废依赖该前缀的层缓存,并显式报 `gfx.log.truncated`。
