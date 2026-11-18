# DIV-GFX-091 · 4.7x 商业包的 `.png` 实为 YCG 容器，全部 CG/UI 解码失败黑屏

- 状态: fixed(资产解码 + 尺寸头)
- 优先级: 高
- 发现日期 / 发现方式: 2026-11-18 / 主人实机反馈「该社团 47x 游戏全部黑屏」；
  `yu-cli run --backend wgpu` 的 `asset.decode` 报「命中 …png 但解码失败:
  The image format could not be determined」

## 复现

- 游戏位置 / 操作序列 / 所需数据(只写结构,不写本机路径):
  - `<games>/<0.479 样本 A>/pac/cg.ypf` → `cg/tr/bg_black.png`
  - `<games>/<0.479 样本 A>/pac/cgsys.ypf` → `cgsys/main/tip_namewindow_txspace.png`
  - 同族 0.474–0.488 商业包(0.479 样本 A/B、0.474 样本、0.481 样本、
    0.488 样本)的 `cg*/cgsys` 包同型。
- 锚点: 启动首帧即黑屏；`RenderReport.missing_assets` 出现
  `asset.decode`（不是 `asset.missing`，说明路径命中、字节不被识别）。

## 官方行为

官方 SDK 0.479 `yu-ris.exe`(sha256
`479f6bf712edeb34ed3e5d0b1d3e6287309c01d58f30cda700751fe10b6a1cc0`)：

- `FUN_004254e4`(`0x4254e4..0x4257b9`)是 YCG 装载体：
  - `CMP dword [EBX], 0x474359`（magic `"YCG\0"`），不符报 `File Check Error.`；
  - `width=[+0x04]`、`height=[+0x08]`；任一 `> 0x2000` 报错；
  - `method=[+0x10]`；
  - `method==0`：`memcpy(dst, data+0x38, width*height*4)`；
  - `method==1`：两次 `FUN_0048384c`：
    第一段 `src=data+0x38`、压缩长 `[+0x24]`、解压长 `[+0x20]`；
    第二段 `src=data+0x38+[+0x24]`、压缩长 `[+0x34]`、解压长 `[+0x30]`；
    第二段写到 `dst + [+0x20]`（两段解压字节首尾相接）。
- `FUN_0048384c`(`0x48384c..0x4838ff`) 是 zlib `uncompress` 包装
  （内部 `FUN_004839ac` + 版本串 `"1.2.3"`），成功回写实际解压长度。

真实字节(包内条目)：

```text
cg/tr/bg_black.png (4870 B):
59 43 47 00 | 00 05 00 00 (1280) | c0 03 00 00 (960) | 20 00 00 00 (32)
01 00 00 00 (method=1) | ... | 00 80 25 00 (raw1=2,457,600)
67 09 00 00 (packed1=2,407) | ... | 00 80 25 00 (raw2)
67 09 00 00 (packed2) | 78 da ... (zlib)
→ 两段各 2,457,600 B，合计 1280*960*4；首像素 BGRA 00 00 00 FF
```

## 内核行为

`TextureStore::decode_asset` 只走 `image` crate（PNG/BMP/JPEG/GIF），
把 `YCG\0` 字节当未知格式 → `asset.decode`，图层无像素 → 整屏黑。
`yu-format` 也没有 YCG 模块。

## 根因

资产容器层缺失：4.7x/4.8x 商业包沿用 `.png` 文件名，内容是 YCG
（0x38 头 + BGRA + 可选两段 zlib）。与 VM/ABI 版本无关；同族游戏全部
受同一资产层影响。

## 修法

- 新增 `yu-format/src/ycg.rs`：类型化头部解析、method 0/1、官方
  0x2000 尺寸上限、两段 zlib 长度校验、BGRA→RGBA；错误码
  `fmt.ycg.codec` / `fmt.ycg.unsupported`。
- `yu-render-core::decode_asset` 在 `image` 之前嗅探 `YCG\0` 并调用该模块。
- 证据锚点写在模块文档（官方函数地址 + 真实样本字节）。
- 不引入第三方解码代码；格式按 REA 与真实字节自行实现。

## 追加根因：尺寸头缺失让 logo/按钮退化为 1×1（2026-11-18）

上面的解码修复只解决了“像素读不出来”。实机紧接着暴露两个症状：
首幕品牌 logo 不显示、标题/确认框点击无反应。继续取证后确认是
**同一代资产的尺寸查询路径**还没认 YCG：

- `yu-rt/src/image.rs::pixel_size` 只认 PNG/BMP/GIF/JPEG；47x 的
  `cg/bg/logo.png`、`cgsys/confirm/btn_yes_off.png` 字节都是 `YCG\0`，
  于是 `CGINFO[SX/SY]` 回填 0/1×1。
- 后果一：`es_sprite.yst` 的暂存层/裁剪层按 1×1 放样，可见层停在
  屏幕外哨兵 `(999999999, 999999999)`，logo 永远不出现。
- 后果二：0.479 `es_button.yst::es.IDBtnTask` 的命中矩形
  `mouseX >= x && mouseX <= x+w …` 中 `w/h` 来自 1×1 的层尺寸，
  点击坐标再正确也进不了 `CGINFO[ONMOUSE*]` 分支。
- 证据：修复前 `CGINFO SPLTR401 SX/SY -> 1`、`SPL201` 为 1×1；
  ONMOUSE 探针在点击帧 0 次命中。修复后同一帧
  `[onmouse] layer=:/W0/BT.OFFES.CSEL.BTN.SEL=1=1 flag=ONMOUSE2
  pointer=(500,390)`，对话框随即关闭。

### 修法

`pixel_size` 增加 YCG 分支：`magic=YCG\0`、`+0x04=width`、
`+0x08=height`（LE），与官方 0.479 `FUN_004254e4` 读取的字段一致；
尺寸为 0/截断返回 `None`。单测 `image::tests::ycg_header`。

### 回归（追加）

- `cargo test -p yu-rt --lib image::tests` → 5 passed。
- 0.479 样本 A 全新存档软渲染：`(544,384)` 随淡入 144→255，中心
  `(640,480)` 19,18,18→34,31,32；与直接解码 `cg/bg/logo.png` 的
  BGRA 中心 (32,31,34) 一致。
- 0.479 样本 B：同点位 89→158 淡入。
- 点击 `(500,390)`：`ONMOUSE2` 命中 Yes 层，CSEL 对话框关闭
  （ops 18→3）。

## 回归

- 对拍用例 / golden 名称:
  - `cargo test -p yu-format --lib ycg`（5 项：method0/1、空段偏移、
    长度拒绝、0x2000 上限）
  - `YURIS_SAMPLE_YCG_PACK=<…/pac/cg.ypf> cargo test -p yu-render-core --test ycg_corpus`
    → 解码 1280×960(4,915,200 B RGBA)，`cg/tr/bg_black.png`
  - `yu-cli run <games>/<0.479 样本 A> … --backend wgpu --frames 120`
    → `asset.decode` 归零；`textures: decoded=118`（修复前 0）
  - 同族冒烟：0.474 样本首屏 EOCS 图正常；0.481 样本、0.488 样本
    均有实际绘制 op 与不同屏幕哈希。
- 残余（另案）：2026-11-18 实测首幕品牌 logo 淡入与确认框点击均已恢复；
  首幕后场景 CG 的推进时序仍按 `docs/tasks/roadmap-batches.md` 的
  运行时任务线继续跟踪（与资产容器无关）。
- 残余（架构）：尺寸注入仍是查询驱动（`CGINFO[SX/SY]` 失败 → 解析头 →
  回填），不是官方“`CG[FILE]` 装载即写层矩形”。未查询过尺寸的层在
  COPY 快照时仍可能缺矩形；0.479 样本本轮未触发，若要完全同构另案做
  装载时预注入。
