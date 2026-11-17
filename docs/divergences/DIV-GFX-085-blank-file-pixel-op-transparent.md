# DIV-GFX-085 · `CG[FILE=""]` 的 1×1 空白面:像素运算读四零透明,直接绘制呈黑

- 状态:**fixed**(2026-11-15)
- 取证对象:官方 0.495 `yu-ris.exe`(sha256 `4f5eefa3…`)`FUN_0040c5f8`;
  v474 商业样本实机逐帧捕获
- 相关:DIV-GFX-060(空 `FILE` 的原始字节)、DIV-GFX-084(目录形 `FILE` 的透明画布)、
  DIV-GFX-046(`CGACT[SIZE]` 缓存层)

## 现象

转场起点(开幕 logo、读档淡入淡出)出现**恰好一帧**全黑:

```text
捕获 f000793.png  mean=255.0
捕获 f000794.png  mean=0.0      ← 单帧全黑
捕获 f000795.png  mean=255.0
```

同一捕获里另有 211/403/501/727 等周期黑帧,间隔与转场次数一致。

## 证据链

### 1. 黑帧的命令增量(帧报告)

黑帧(gfx 796)比相邻帧多一条纹理 op,且 `gfx_gpu.offscreen` 2→3:

```text
[copy] f=796 id=53 src=Group(:/W0) screen_snapshot=true snap_layers=131
[texop] f=796 seq=805 IMG :/W0/SP/LNO_FB.1 -> :/W0/SP/LNO_FB.1 RectPaint3
[texop] f=796 seq=807 IMG :/W0/SP/BUF.LNO_FB.1 -> :/W0/SP/LNO_FB.1 SizeNearest
[texop] f=796 seq=808 COPY Group(:/W0) -> :/W0/SP/BUF.LNO_FB.1   ← 屏幕→缓冲
```

即脚本**先** `SIZE(BUF→LNO_FB)`、**后** `COPY2(Group→BUF)`。执行到 seq 807 时
BUF 仍是空白 1×1;SIZE 把它放大成 1281×721。

### 2. 画面层

```text
[op] f=796 LY :/W0/SP/LNO_FB.1 z=8900000 a=256 vis=1 tex=solid fill=Some([0,0,0,0])
      content=1281x721@2
[pass] f=796 i=5 tgt=OFF::/W0/SP#118 draws=11
      [... Quad/tex=2(1x1) ...]                     ← 第 11 笔即 LNO_FB.1
```

`content@2` 的纹理是 1×1,与 `TRBUF.*` 的 `fill=None` 空白基面是**同一张**
`SOLID_BLACK_PX` 纹理。LNO_FB.1 在 z=8,900,000、alpha=256 全屏绘制 ⇒ 整屏黑。

### 3. 官方空串分支写入的是四个零字节

REA 0.495 `FUN_0040c5f8`(Ghidra 12.1.4,逐指令):

```text
0x40c60a  MOV [ESI+0x78], 1        ; 表面宽 = 1
0x40c60d  MOV [ESI+0x7c], 1        ; 表面高 = 1
0x40c638  XOR EDX, EDX
0x40c63a  MOV byte [EAX], DL
0x40c642  MOV byte [ECX+1], DL
0x40c64b  MOV byte [ECX+2], DL
0x40c654  MOV byte [ECX+3], DL     ; 四字节全 0
```

⇒ `CGACT[SIZE]/COPY/MERGE` 读这块像素时看到的是 **ARGB 0x00000000 = 透明**。
官方同脚本同样在 seq 807 放大空白 BUF,结果透明,所以不闪。

### 4. 直接绘制/过渡取样的口径仍是黑(手册)

手册 `cg/cg.html`:`FILE="" で1x1サイズの黒画像生成`。黑幕/遮挡板依赖这条:
把 1×1 像素本身改成透明会让 `A` 淡出的黑幕提前消失、转场不可见
(用户实机 A/B,2026-11-14)。

结论:**两个消费口径不是同一个东西**——

| 消费点 | 读到的 1×1 空白 |
|---|---|
| `CGACT[SIZE/SIZE2/COPY/MERGE]` 的源、文本基面 | 四字节全 0 → 透明 |
| 直接绘制、双纹理过渡取样 | 手册口径 → 黑 |

## 根因

wgpu `resolve_ref` / soft `solid_fill_texture` 只实现了一个默认像素
(`SOLID_BLACK_PX`),被像素运算与直接绘制共用。`SIZE` 从空白 BUF 取源时
拿到不透明黑,放大成 1281×721 黑板。

## 修法

- `crates/yu-render/wgpu/src/plan.rs`:`resolve_ref` 增加 `blank: [u8;4]` 参数,
  并把它并入基面缓存键。像素 op 源/文本基面传 `SOLID_EMPTY`;过渡取样
  (`passes.rs::layer_image`)传 `SOLID_BLACK_PX`。
- `crates/yu-render/soft/src/render.rs`:`solid_fill_texture`(仅像素 op 源
  使用)默认改 `SOLID_EMPTY`;`draw_layer` 的直接绘制保持 `SOLID_BLACK_PX`。
- `crates/yu-render/soft/src/text.rs`:文本基面默认改 `SOLID_EMPTY`。
- `Layer.fill`(整层 `RECTPAINT*` 结果)在两个口径里都优先,行为不变。

## 验证

```text
YU_OP_TRACE=1 YU_PASS_TRACE=1 yu-cli run <project> --mode desktop \
  --frames 800 --paused --step-to 798 --capture-dir target/tmp/fix798
```

逐帧平均亮度(32×18 采样):

```text
近黑帧 = [1..20]                  ← 启动黑屏,正常
790..798 = 255.0 255.0 255.0 255.0 255.0 255.0 252.36 249.70 247.03
```

修复前同一命令的 794 = 0.0;211/403/501/727 也在近黑列表里,修复后全部消失。

回归:

```text
cargo test --release -p yu-backend-soft --test render   → 26 passed
cargo test --release -p yu-gfx -p yu-render-core         → 全绿
cargo test --release -p yu-backend-wgpu                  → 38 passed,1 并行偶发
  (diffusion/blur 两条软硬对拍在并行 GPU 下随机一条失败;单跑必过,与本次改动无关)
```

新增用例:`cgact_size_from_fileless_blank_layer_is_transparent`
(`crates/yu-render/soft/tests/render.rs`)。

## 残留(未在本轮改)

- `CG[FILE=""]` 在**已存在层**上的矩形重置已在 2026-11-17 裁定并修复:
  官方 `FUN_0042b144` 空串分支 `FUN_0040c5f4` 无条件写
  `+0x78/+0x7c = 1`;非空走 `FUN_0040c7c5` 按新图重建。内核
  `apply_cg_inner` 换图分支已同步,回归
  `empty_file_on_existing_layer_resets_rect_to_1x1`。像素 alpha 的四零
  语义仍按本档结论。
- `screen_snapshot` 合成与官方后缓冲读取的差异(DIV-GFX-051)仍是独立议题;
  本轮黑帧已由空白像素语义解释,不需要动快照路径。
