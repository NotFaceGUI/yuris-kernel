# DIV-GFX-092 · WebP 资产未解码导致精灵层全黑

- 状态: **fixed(no-manual,2026-11-23)**
- 影响面: `crates/yu-rt/src/image.rs`、
  `crates/yu-rt/src/vm/handlers/media_info.rs`、
  `crates/yu-gfx/src/layer.rs`、`crates/yu-render/core/{Cargo.toml,src/texture.rs}`
- 关联: `DIV-GFX-024`(`CGINFO[SX/SY]` 资产尺寸回退)、
  `DIV-GFX-091`(YCG 资产容器)
- 样本: `<v555-1>`、`<v555-pc>`

## 1. 现象

旧代 stdlib 游戏进入正文后:窗口 UI 正常,背景/立绘全黑。
脚本侧数据流正常:

- `SPLBUF###` 收到真实资产(`CG[FILE=cg/bg/...]`、角色 CG);
- `SPL###` 显示层存在、属性持续刷新;
- 期望链 `SPLBUF### --CGACT[SIZE=1 …]--> SPL###` 未发生。

帧层证据(`display.json` + SPL 层导出):

- `SPL###` 的 `texture = solid_black_1x1`,位置 `x=y=999999999`;
- `SPLBUF###` 只有 COPY2 计划,没有后续 SIZE 计划;
- `es.CGACT_SIZE` 的条件
  `((@SX源 != @SX目标) || (@SY源 != @SY目标)) && @SX源 > 0 && @SY源 > 0`
  不成立 —— 实测 `CGINFO[SX/SY]` 对 `SPLBUF###` 返回 **0×0**
  (`[cginfo] … SPLBUF1001 … -> 0`)。
- 0×0 ⇒ `es.CGACT_SIZE` 把目标位置写成 `999999999`(屏外),
  SIZE 与 `CG[TID]` 绑定全部跳过。

## 2. 根因:资产是 WebP,内核没有解码/探测能力

- 语料自证:`<v555-1>` 的 `pac/cg1.ypf`/`cg2.ypf` 共 **29,502** 个
  `.webp`(0 个 bmp/jpg/gif);失败资产实际路径
  `cg/tip/…/bc_a_3_back.webp`。
- `CG[FILE]` 的键不带扩展名,渲染端按 `yscfg.image_ext_order` 候选
  探测;旧候选只有 `.png/.bmp/.jpg/.gif/.psb`,全部 miss。
- 资产未装载 ⇒ 源层无像素/尺寸 ⇒ `COPY2` 目标快照无显示矩形
  (`snapshot_sizes` 无条目) ⇒ `CGINFO[SX/SY]` 返回 0
  (不是错误,VM 的资产头回退不会触发) ⇒ SIZE 链整条跳过。
- v555 官方探测函数 `FUN_0043d81c`(b1baf88a871)的扩展名分支为
  `1=.bmp / 2=.png / 3=.jpg / 4=.webp`,语料与实现一致。

## 3. 修法(通用能力,不做版本门控)

1. `image` crate 打开 `webp` feature(`yu-render-core`);
2. `yu-rt::image::pixel_size` 增加 WebP 头解析:
   `VP8`(有损)、`VP8L`(无损)、`VP8X`(扩展/动画画布);
3. 渲染端 `TextureStore::candidates` 与 VM `resolve_asset_size` 在
   `yscfg` 码表候选之后**通用追加一次 `.webp`**;显式扩展名的键不做追加;
4. `ImageFormat` 增加 `Webp` 变体(不加入任何一版 `yscfg` 码表),
   `known_extension_format` 识别 `.webp`。

这样 29,502 个 WebP 资产可正常解码并回报尺寸,SIZE 链恢复,
显示层拿到像素与位置。

## 4. 回归

- `<v555-1>` 交互进入正文:背景/立绘恢复绘制(主人实测通过);
- `<v555-pc>` 同一资产格式,走同一通用能力;
- `cargo test -p yu-rt --lib image::tests::webp_headers`;
- 既有 PNG/BMP/GIF/JPEG/YCG 路径不变(仅候选链末尾追加一项)。
