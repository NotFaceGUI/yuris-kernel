# DIV-GFX-093 · 0.500 资产容器 `.ydg`(YDG + QOI 瓦片)与扩展码表换代

- 状态: **fixed(no-manual,2026-11-24;`<v500-relirium>` 取证)**
- 优先级: 高(0.500 的 UI/CG 主力格式;缺它标题/按钮整屏缺失)
- 发现日期 / 方式: 2026-11-24 / 标题层 `asset.missing` + REA 反编译
- 关联: DIV-GFX-091(YCG)、DIV-GFX-092(WebP)、
  `docs/formats/v500-abi-census-2026-11-22.md`

## 1. 现象

`<v500-relirium>` 启动后标题/按钮层全部 `asset.missing`(改前 190 条),
`textures: decoded=0`。资产在 `pac/cgsys.ypf` 里实际存在,扩展名是 `.ydg`。

## 2. 容器布局(0.500 `Relirium.exe` 实测)

```text
+0x00 char[4] "YDG\0"
+0x04 char[8] "YU-RIS\0\0"
+0x0C u32     版本/调度值(样本恒 100)
+0x10 u32     头长(样本恒 0x30)
+0x14 u32     文件字节数
+0x18 u32     保留
+0x1C u32     保留
+0x20 u16     画布宽(上限 0x2000)
+0x22 u16     画布高(= 各瓦片高之和,上限 0x2000)
+0x24 u32     保留
+0x28 u32     保留
+0x2C u32     保留
+0x30 u32     瓦片数(官方上限 16)
+0x34 瓦片表[N],每项 16 字节:
+  +0x00 u32 数据偏移(相对文件头)
+  +0x04 u32 数据字节数
+  +0x08 u8  编码码:0 = QOI;2 = WebP;其它未取证
+  +0x09 u8  保留
+  +0x0A u16 瓦片高
```

瓦片是**按行切分的同一张图**(不是动画帧):每片同宽,高度累加等于画布高。
官方把每片解码进同一个 `宽 × 总高` 缓冲的 y 偏移处,拼成整图。

样本(全部实测):

| 资产 | 画布 | 瓦片 |
|---|---|---|
| `cgsys/title/normal/back.ydg` | 1920×1080 | 4 × 1920×270 |
| `cgsys/title/normal/btn_start_off.ydg` | 253×60 | 1 |
| `cgsys/main/sysmessage/tip_nextsel.ydg` | 581×86 | 2 × 581×43 |
| `cgsys/title/black.ydg` | 1920×1080 | 4 × 1920×270 |

## 3. QOI 解码(编码码 0)

官方 `FUN_0044c670`(`0x44c670`,1206B)是标准 QOI 解码器:

- 校验 `qoif` 魔数、宽高、channels(3/4)、colorspace(0/1);
- 防溢出判据 `height < 400000000 / width`(宽高分开比);
- 输出缓冲字节序 **B,G,R,A**(Render 侧转 RGBA);
- `FUN_00449594`(122B)只读 QOI 头尺寸。

本仓库 `crates/yu-format/src/ydg.rs` 实现 QOI 解码,并用 PIL 的 QOI 解码器
对拍 `btn_start_off.ydg` 的 253×60 像素(字节和 11920800,逐像素一致)。
**坑**:早期把官方防溢出判据误写成 `w*h > 400000000 / w`,会拒绝
1920×270 宽瓦片(整屏标题背景),单测 `qoi_accepts_wide_frame_beyond_naive_pixel_cap`
锁住。

## 4. 扩展码表换代(0.500)

0.495 `FUN_0040c9b4`: `1=png, 2=bmp, 3=jpg, 4=gif, 5=psb`。

0.500 `FUN_0046fb18`(`0x46fb18`,3581B)按 `yscfg+0x1c` 的分支是:

| 码 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---|---|---|---|---|---|---|---|---|
| 扩展名 | `.bmp` | `.png` | `.jpg` | `.gif` | `.ydg` | `.webp` | `.jxl` | `.qoi` | `.avif` |

`_RDATA` 的扩展名字符串池(RVA `0x499640`)与分支逐项对应。
`<v500-relirium>` 的 `yscfg` 顺序是 `05 01 02 06 07 08 09 03` →
ydg→bmp→png→webp→jxl→qoi→avif→jpg。

实现:`ImageFormat::from_code_v500` + `probe_order_from_codes_v500`;
`GfxState::from_tables` 与引擎 `TextureStore` 构造都按 `yscm.version == 500`
选表(0.555 仍走旧表,直到单独取证)。

## 5. 实现落点

- `crates/yu-format/src/ydg.rs`:容器解析 + QOI 解码(带单测)。
- `crates/yu-render/core/src/texture.rs`:`decode_asset` 识别 YDG,按瓦片
  拼回 `width × total_height`;QOI 直解,WebP 瓦片走宿主 `image` 解码。
- `crates/yu-gfx/src/layer.rs` / `frame.rs`:新增 `ImageFormat::Ydg`、
  `from_code_v500`、`probe_order_from_codes_v500`(单测锁映射)。
- `crates/yu-engine/src/engine.rs`:纹理探测序按 0.500 码表;
  `crates/yu-gfx/src/state.rs` 同步。
- `texture.rs` 的显式扩展名识别与 `strip_image_ext` 补 `.ydg`。

## 6. 验证

- `cargo test -p yu-format --lib ydg::` → 4 passed;
- `cargo test -p yu-gfx --lib v500_probe_order` → 1 passed;
- `cargo run -p yu-format --example ydg_decode -- <back.ydg>`:
  4 瓦片 1920×270 → 1080 行,像素字节和 1555677937(与 PIL 拼图一致);
- 游戏侧:`textures: decoded=204`(改前 0),标题层从 `asset.missing`
  变为可解码;第 900 帧截图 `title900b.png` 为正确的 1920×1080 标题背景
  (4 瓦片拼图)并显示「前回のつづきから」;剩余真实缺失只有
  `btn_skip_bt4`/`tip_voicedel`/`map_back` 与按钮**基名**探测
  (`btn_start` 等,官方本身只有 `_off/_on/_over` 变体)。

## 7. 残余

- 编码码 2(WebP 瓦片)无真实样本;官方缓冲字节序按 `image` 输出处理。
- 编码码 1/3 未取证。
- 码 7/8/9(`.jxl/.qoi/.avif`)目前保留 `Other`(不参与探测);独立
  `.qoi` 资产可复用 QOI 解码器,待有样本再接入。
- 多瓦片是否只按行切分,当前由 4 个样本支持;若出现按列切分需另取证。
