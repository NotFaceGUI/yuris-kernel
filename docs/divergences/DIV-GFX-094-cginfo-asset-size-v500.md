# DIV-GFX-094 · `CGINFO[SX/SY]` 资产尺寸解析与 0.500 `.ydg`(按钮点击失效)

- 状态: **fixed(no-manual,2026-11-24;`<v500-relirium>` 取证)**
- 优先级: 高(按钮可见但点击无反应、按钮组合缺块、动效几何异常)
- 发现日期 / 方式: 2026-11-24 / 桌面实跑 + 代码链路复盘
- 关联: DIV-GFX-024(尺寸回填)、DIV-GFX-093(YDG/QOI)、DIV-RT-079

## 1. 现象

标题/主界面能画出背景和部分按钮,但:

- 鼠标点击按钮无任何反应;
- 按钮的状态合成(COPY/SIZE/TRANSITION)缺块;
- 依赖 `CGINFO[SX/SY]` 的布局/动画几何异常。

## 2. 根因

按钮/精灵的命中矩形来自 `CGINFO[SX/SY]`;该查询在 gfx 层没有尺寸时,
走 VM 的 `resolve_asset_size(MFILE)` 回填。两个环节只认 0.495:

1. `resolve_asset_size` 内联了 0.495 码表
   `1=.png 2=.bmp 3=.jpg 4=.gif 5=.psb`,不认 0.500 的
   `1=.bmp 2=.png 5=.ydg 6=.webp`;
2. `yu_rt::image::pixel_size` 不解析 `YDG\0` 头。

于是所有 `.ydg` 资产的尺寸回填失败 → 图层矩形退化为 0/1×1 → 按钮命中区
只剩一个点,`CGACT` 的组合也按错误尺寸计算。表现就是「有画面、点不动、
按钮缺块」。

## 3. 修复(单一码表 + 头解析)

- `crates/yu-format/src/image_ext.rs`:0.495/0.500 图像扩展码表的**唯一
  真值表**;`yu-gfx`(`ImageFormat::from_code*`)与 `yu-rt`
  (`resolve_asset_size`)共用,并加交叉一致性测试。
- `crates/yu-rt/src/image.rs`:`pixel_size` 增加 YDG 分支
  (`YDG\0YU-RIS\0\0`,`+0x20/+0x22` u16 LE,上限 0x2000;官方
  `FUN_00449a08` 同址)。
- `resolve_asset_size`:按 `yscm.version == 500` 选表,候选与
  `TextureStore::candidates` 同序(含 `.ydg/.webp`),不再内联旧表。
- `ImageFormat` 补齐 0.500 的 `Jxl/Qoi/Avif`;渲染器识别独立 `.qoi`
  并复用 `yu_format::ydg::decode_qoi`,JXL/AVIF 交给 `image` 后端,
  失败显式 `asset.decode`。

## 4. 验证

- `cargo test -p yu-rt --lib image::` → 7 passed(含 YDG 头);
- `cargo test -p yu-gfx --lib image_format_tables` → 1 passed
  (0.495/0.500 两表与共享表逐码一致);
- `cargo test -p yu-render-core --lib` YDG 拼图/真实样本用例通过;
- 桌面/headless 实跑(`<v500-relirium>`,release):
  - `ATTENTION` 警示页 → 标题 build 动画 → 完整标题菜单;
  - 8 个菜单按钮 + 手册/官网按钮全部出现,`はじめから` hover 出高亮、
    点击后进入淡出,证明命中矩形与状态切换都生效。

补充根因(同日第二轮):`FILEINFO[CGFILE]` 的候选扩展名仍是**内联
0.495 表**(`5=psb`),`es_buttonque.yst` 的五次
`FILEINFO[EXIST=1 CGFILE=..._off/_over/_on/_onover/_na]` 对 `.ydg`
全部返回 0,脚本随即 `es._mes(\"ボタン画像指定が正しくありません\")` +
`END`。所以此前只有显式传五个名字的 `LLOAD` 活下来。修复为与
`CG[FILE]`/`CGINFO`/`resolve_asset_size` 共用 `yu_format::image_ext`
(按 `yscm.version == 500` 选 0.500 表),并新增夹具
`fileinfo_cgfile_uses_v500_ydg_table`。

## 5. 残余

- 部分按钮请求 `cgsys/main/button/btn_skip_bt4`(无 `type2/`),包里只有
  `type2/btn_skip_bt4.ydg`;需确认按钮类型/皮肤目录的官方选择规则
  (`es.BT.CG.SET` 的 type 参数)而不是加别名。该按钮属于段落内隐藏的
  快捷操作,不影响标题菜单。
- 音频 `.opus`(0.500 码表含 `.wav/.ogg/.opus`):**已支持**,见
  `DIV-SND-006`。候选序按 0.500/0.555 档切到 `.wav → .ogg → .opus`,
  `opus-pure` 解码 Ogg Opus;官方表位证据在 `DIV-SND-006 §1`。
