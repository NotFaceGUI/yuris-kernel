# DIV-GFX-077 · 環境設定界面:局部 COPY2 表面尺寸与 1×1 黑文本基面

- 状态: **fixed**(2026-11-05)
- 影响面: `crates/yu-render/soft/src/{render,text}.rs`、
  `crates/yu-render/wgpu/src/plan.rs`
- 关联: `DIV-GFX-058`(COPY 携带文本)、`EVID-T-11`(COPY2 区域)、
  `DIV-GFX-055/075`(文本合成基面)
- 样本: 商業包 `<game-a>` 同目录的 `<game-481>.exe` 環境設定
  (`CONFIGURATION`);脚本 `data/script/eris/scene/config/es_config.yst`
  (s36)、`es_button.yst`(s0)

## 1. 症状(主人截图,红框)

1. 右侧「プレイ時に利用する機能」各行的 **on/off 标签被一块横向拉长的
   暗色标记盖住**;左侧 `full/window` 与字体区的复选标记同样被拉宽。
2. 左下方「フォント設定」预览窗整块黑:字体名 `ＭＳ ゴシック` 与
   「文章の表示サンプルです。」两行样例文字均不显示。

## 2. 取证

### 2.1 on/off 标签不是文本,是背景图

`cgsys/config/back_easy.png`(控制台提取)在每行按钮位置已经烤好
白色圆点与 `on/off` 文字。按钮层只负责叠一个状态标记:

- `es_button.yst` s0 pc1951:`CG[ID=BT.OFF.TMP FILE=$BT.OFF]`
- s0 pc1961:`CGACT[ID=$BT.OFF.TMP ID2=$state COPY2=1 X=36 X2=53]`
  —— 从 54×18 状态表切出 **18×18** 的 on 标记;
- 随后通用按钮代码对状态层执行
  `CGACT[ID=$state SIZE=1 X=0 X2=@BT.SRCSX SX=@BT.SRCSX SY=@BT.SRCSY]`
  (SDK 0.479 `es_Button.yst:2413` 同形),本包 `@BT.SRCSX=50`.

官方 `COPY2` 把区域块写到目标 `(0,0)`、**其余保持源表面尺寸并清空**
(EVID-T-11)。因此 `SIZE` 的 `X=0 X2=50` 在 54×18 表面上是一次近似
无缩放的裁剪。本内核旧实现把快照做成 18×18;`SIZE` 为对齐 50×18
目标把标记横向拉伸 ≈2.8 倍,遮住背景图里的 `on/off`。

### 2.2 字体预览:1×1 黑层的文本基面

`s36` 创建字体样例窗:

- `CG[ID=SMTX_BG SX=600 SY=180 FILE=""]`
- `CG[ID=SMTX SX=600 SY=180 FILE=""]`
- `FONT.DRAW("SMTX", "文章の表示サンプルです。")`
- `FONTL1` 同样以 `FILE=""`、`SX=210 SY=19` 显示字体名。

`CG[FILE=""]` 在 gfx 里表示为 `TextureRef::SolidBlack1x1` + 显式
`SX/SY`。像素阶段会把它铺成 600×180;但文本合成取基面时旧实现只看
纹理种类,拿到 1×1 基面,`CGACT[TEXT]` 的 `x=50/y=50` 等坐标全部
落到图外 ⇒ 预览窗有黑底、无文字。

### 2.3 补充(2026-11-11):`CGINFO[SX/SY]` 是显示矩形,不是快照表面尺寸

局部 `COPY/COPY2` 的目标显示矩形 = **复制块尺寸**(官方
`FUN_0042f809` 在动作分派后无条件写 `+0x78/+0x7c = X2-X+1/Y2-Y+1`,
COPY/COPY2 的属性块不覆盖尺寸字段),快照表面仍保持源尺寸(§2.1)。
ERIS 按钮用 `CGINFO[ID=$off SX=1 LET=@BT.SX]`(`es_Button.yst:2657`)
取按钮宽;旧实现把源合图宽(按钮宽 × 4)当快照尺寸,
`es.BT.MOUSECHECK` 的命中矩形覆盖四个按钮,置灰按钮被 `@BT.N` 跳过时
相邻按钮接住 hover(<game-b> 2026-11-11 实测)。修复:`snapshot_sizes`
记区域尺寸、`snapshot_origins` 记区域原点,`CGINFO`/`ONMOUSE*` 按显示
矩形查询;绘制仍按源尺寸快照(区域在 `(0,0)`,其余透明)。见 PROGRESS
「成果 BT-HOVER」。

### 2.4 最终口径(2026-11-17):显示/命中矩形与表面尺寸分离

0.495 REA 复核(`FUN_0042f809`,0x42f809):

- 入口无条件算区域 `local_2c = X2-X+1`、`local_28 = Y2-Y+1`
  (0x42f85f–0x42f876);
- 双层路径(COPY/COPY2)动作后把**目标层** `+0x78/+0x7c` 写成
  `local_2c/local_28`(0x4305f4/0x4305fe);COPY 的属性块
  (0x08,0x43060d 起)不复制尺寸字段;
- 单层路径同型写 `+0x78/+0x7c`(0x42fa0a/0x42fa14);
- 例外:RECTPAINT/2/3 家族在双层路径改用源层尺寸
  (0x43041e–0x430433),与 COPY 无关;
- `CGINFO[SX/SY]` 直接读 `+0x78/+0x7c`(0x43d86d/0x43d87d);
- 全幅复制由调用方把 `X2/Y2` 填成源宽高−1 ⇒ 矩形 = 源表面尺寸
  (与 EVID-T-11 微场景一致)。

内核最终口径:

- `Layer.display_rect: Option<CopyId>` 只记**来源**(最近一次 COPY/COPY2
  的快照 id),查询期用 `snapshot_display_size` 动态解析区域矩形;
  `LayerKind::Image.size` 仍是后端绘制缩放尺寸,二者不合并。
- 查询优先级:组尺寸 → `display_rect` → `Image.size` → 纹理/资产尺寸;
  `CGINFO[ONMOUSE*/COLOR]` 同序。解析失败(0×0 源)返回权威 0/−1,
  不回落到 COPY 前的旧 `Image.size`。
- 失效:`CG[ID SX/SY]`(含单轴)、`CGACT[SIZE/RESIZE]`、`CG[FILE]` 换图、
  `YMV` 表面重写与层重建都会清 `display_rect`;`SWAP` 不交换矩形
  (官方只换纹理/位置/属性),保持原值。
- 单轴 `CG[SX/SY]`:缺省轴取**当前显示矩形**(COPY 矩形优先),不取
  COPY 前的旧 `Image.size`(官方只写 `+0x78`/`+0x7c` 中给定的那一轴)。
  回归 `single_axis_cg_size_after_copy_keeps_other_axis_from_copy_rect`。
- **纯属性 `CG`(无 `SX/SY`)不得改矩形**。`CG[ID=$off E=0 X=… Y=…
  MODE=…]` 这类按钮状态层更新只改属性;若把 COPY 区域尺寸当作缺省轴
  写进 `Image.size`,会把源尺寸快照压扁(<game-b> 底部按钮行实测
  回归)。回归 `pure_attribute_cg_after_copy_keeps_draw_size_untouched`。
- **绘制仍按源尺寸快照**:局部 COPY 的块在目标 `(0,0)`,其余透明;
  不把区域矩形写进 `Image.size`,避免把源合图压扁(§2.1 的 18×18
  状态标 + 后续 `SIZE X=0 X2=50` 仍按 54×18 表面近似无缩放裁剪)。
- **COPY/COPY2 清掉复制前的显式矩形**(2026-11-19):官方
  `FUN_0042f809` 在动作后释放目标旧表面、挂上区域尺寸新表面并写
  `+0x78/+0x7c = 区域尺寸`,所以 `CG[FILE=""]` 的 1×1 或 COPY 前的
  `SX/SY` 不再代表目标层;内核把 `Image.size` 清成 `None`,后端按快照
  自然尺寸 1:1 绘制,显示/命中矩形仍由 `display_rect` 回答
  (`DIV-GFX-089` §9;读档 CGS 幕布回归)。
- 残余:官方同一执行体按区域尺寸重新分配目标表面;内核保留源尺寸
  表面。若脚本把区域坐标写到“超出显示矩形但仍在源表面内”的边界
  (当前语料未见),内核按显示矩形夹取,与官方按区域表面夹取不同。
  该边界用法待实际样本再取证,不在本次混改。

## 3. 修复

1. **表面实色模型**:`Layer.fill: Option<[u8;4]>`。整层
   `RECTPAINT`/`2`/`3`(无区域、原地、基面为 `SolidBlack1x1`)在命令
   时刻把结果记进 `fill`(EVID-J-02..04 的 RGB/alpha/ARGB 映射);
   渲染器绘制、复制源、文本基面统一用它,不再依赖“加工结果是否烘焙在
   某个后端表面”。soft/wgpu 都改了。
2. **soft**: `layer_base` / `base_texture` / `layer_surface` /
   直接绘制对 `SolidBlack1x1` 使用 `fill` 颜色,并按 `SX/SY` 生成
   显式尺寸表面;无填充时仍是官方 1×1 黑。
3. **wgpu**: `TextureCache::ensure_solid_color` 按 RGBA 缓存 1×1
   实色纹理;`resolve_source`/`resolve_ref`/`plan_layer` 使用它,
   `base_token`/缓存键包含 `fill`;`apply_text` 的文本基面按图层
   声明尺寸放大。
4. **soft/wgpu COPY2**: 图层源的局部复制快照保持**源表面尺寸**,
   区域块写到 `(0,0)`,区域外清空;组源(主窗口截屏)保持区域尺寸,
   不改变既有截图裁剪语义。wgpu 图层源局部复制因此不再走 UV 零拷贝
   别名(别名只有区域尺寸)。
5. **不采用**: 每帧重放 `RECTPAINT3 SET=0`。实测会让屏幕契约帧间
   翻转(频闪),并清掉快照里一次性写入的底栏说明文字;已撤除。

## 4. 验证

- 主人提供官方截图后逐项对照:预览区是页面背景+样例文字(无黑幕),
  `on/off` 标签完整、状态点落在背景白圆;字体名/复选框/底栏说明可见。
- `cargo test -p yu-backend-soft --test render` → 25 passed
  (新增 `cgact_text_on_fileless_solid_layer_uses_explicit_surface_size`、
  `partial_copy_keeps_source_surface_size`、
  `cgact_text_on_painted_transparent_solid_layer_is_visible`)。
- `cargo test -p yu-gfx --test parity_fixes` → 32 passed
  (新增 `full_layer_rectpaint_updates_layer_fill`)。
- `cargo test -p yu-backend-wgpu` → 39 passed。
- `cargo build --workspace --all-targets` → exit 0。
- 帧稳定性:soft/wgpu 各在 1150/1200/1250 帧取整屏 SHA-256,
  同后端三帧完全相同(无频闪);复测前确认无残留 `yu-cli.exe`
  叠窗。

## 5. 残留

- `SIZE` 的 `X2` 端点语义:本包 `X2=SRCX+SRCSX`、`SX=SRCSX`,按含
  端点算是 51→50 的微小缩放;按排他端点是精确裁剪。两种都只差
  1 px,当前视觉无差异;待官方 `SIZE` 处理体逐行确认后再定。
- 主窗口截屏的局部区域仍按区域尺寸;若后续发现官方保留整屏尺寸,
  再单独开档案。
- `Layer.fill` 只记录**整层** `RECTPAINT*`。局部区域填充仍走 `ImageOp`;
  soft 没有持久层表面,后续帧若该局部加工被烘焙省略,可能丢失(本包
  環境設定未用局部填充)。需要持久表面/`SolidSurface` 模型再收口。
- 过渡/遮罩取样里,soft `resolve_source` 与 wgpu `image_ref` 对
  `SolidBlack1x1` 仍返回 1×1 黑,不读 `fill`;若实色层被用作
  transition/mask 源会偏黑。当前语料未观察到,保留挂账。
- display JSON 暂不输出 `fill`(避免 golden 漂移);诊断里看到
  `solid_black_1x1` 但实际表面可能是透明/实色,需结合脚本
  `RECTPAINT3` 判断。
- 组源局部 `COPY2` 仍按区域尺寸;若官方同样“其余清空”保留源尺寸,
  需另开档案。
