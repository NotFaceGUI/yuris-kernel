# DIV-GFX-096 · 车窗外光照层整块白矩形(screen 归约漏乘 stage1 当前 alpha)

- 状态: fixed
- 优先级: 高
- 发现日期 / 发现方式: 2026-11-24 目视 + 2026-11-25 帧抓包/资产逐像素核对

## 复现

- 游戏位置 / 操作序列 / 所需数据(只写结构,不写本机路径):
  `<v500-relirium>` 车窗外场景,脚本进入 `es.SP` 光照段后画面出现约
  704..1761 × 0..608 的纯白矩形。
- 锚点(脚本/标签/帧号):
  - 帧抓包 `white5/f002520`(soft 后端)与 `debugui/f003260`(wgpu);
  - 脚本 `data\script\userdefine\…` s304 pc286:
    `es.SP.CG.SET "ITEM/フレア_アマツチ1"` + `es.SP.BL.SET 5`
    ⇒ 显示层 `SPL2101` MODE 5(スクリーン)。

## 官方行为

D3D9 固定管线,MODE 5 分支(0.495 SDK `yu-ris.exe` sha256 `4f5eefa3…`,
`FUN_00414ba4` @ `0x414ba4`;0.500 商业样本 `FUN_0043d828` @ `0x43d828`,
sha256 `36121d4f…`,与 0.495 仅地址不同;0.479 SDK `yu-ris.exe`
sha256 `479f6bf7…`,`FUN_0040d8f8` @ `0x40d8f8`,状态序列同构):

```text
SetTexture(1, BLACKTEXBUF)                  ; DAT_00801d9c / DAT_007de448
stage0 COLOROP  = MODULATE, ARG1=TEXTURE, ARG2=DIFFUSE
stage0 ALPHAOP  = MODULATE, ARG1=TEXTURE, ARG2=DIFFUSE
stage1 COLOROP  = BLENDCURRENTALPHA(0x10), ARG1=CURRENT, ARG2=TEXTURE
stage1 ALPHAOP  = SELECTARG1, ARG1=CURRENT
SRCBLEND=INVDESTCOLOR(10), DESTBLEND=ONE(2), BLENDOP=ADD(1)
```

顶点 DIFFUSE 四通道都填层 alpha `A`(0.479/0.495 反编译同构,
0.500 同一函数仅地址不同:
`local_58/local_4c = A<<24|A<<16|A<<8|A`)。因此:

```text
stage0.rgb = T·A
stage0.a   = T.a·A                     (C.a)
stage1.rgb = stage0.rgb·C.a + 黑·(1−C.a) = T·A·(T.a·A)
最终 RGB   = T · T.a · A²
```

`FUN_00462e40` 里两张 1×1 纹理的错误串分别确认:
`DAT_00801d98` = “WHITETEXBUF”、`DAT_00801d9c` = “BLACKTEXBUF”;
MODE 3 用白纹理、MODE 5 用黑纹理。0.479 同一分支用
`DAT_007de444`(MODE 3)/`DAT_007de448`(MODE 5),按 blend 语义
(MODE 3 `ZERO/SRCCOLOR` 乘算在 α=0 时必须恒等 ⇒ 白;
MODE 5 `INVDESTCOLOR/ONE` screen 在 α=0 时必须恒等 ⇒ 黑)同样闭合。

同族 MODE 3/7(乘算/暗算)的 stage1 也是 `BLENDCURRENTALPHA`,
arg2 为白纹理/白 TFACTOR(0.479/0.495 分支都显式 `SetRenderState(0x3c,
0xFFFFFFFF)`),源 RGB 应为:

```text
stage1.rgb = T·A·C.a + 1·(1−C.a)
```

## 内核行为

白块帧的 `SPL2101` 表面与原始资产 `cg/item/フレア_アマツチ2.png`
**逐像素相同**(39389 字节,1920×1080;alpha=0 共 1,921,500 px,其中
RGB(255,255,255) 492,222 px;alpha>0 152,100 px 全为白色)。资产本身
就是「RGB=白、alpha=形状」的光照面,加工链没有写坏表面。

旧实现:

- `screen`:`stage_reduce = T·a`(= T·T.a·A),漏掉 stage0 的层 A;
  白块区域内 alpha=0 但 RGB=255,于是整块 screen 成纯白;
- `multiply/darken`:`T·A + (1−a)`,漏掉 stage1 的 `C.a`。

## 根因

MODE 3/5/7 的 stage1 `BLENDCURRENTALPHA` 没有进入归约:screen 少乘
当前 alpha,乘算/暗算少乘当前 alpha。白块是 MODE 5 在纹理透明区读到
白色 RGB 后被整块 screen 上屏的直接结果。

## 修法

- `crates/yu-render/soft/src/render.rs::stage_reduce`
  - `Screen => T·a·layer`;
  - `Multiply | Darken => T·layer·a + (1−a)`。
- `crates/yu-render/wgpu/src/shaders/quad.wgsl`
  - `SRC_MODE=3`:`texel.rgb·src8·layer8/255`;
  - `SRC_MODE=2`:`texel.rgb·layer8·src8/255 + (1−frag)·255`。
- 注释同步为官方 stage0/stage1 链(0.495 + 0.479 证据锚点)。

## 回归

- 单测:`blend_screen_uses_stage1_current_alpha_for_source_rgb`
  (层 A=128、纹理 α=255 ⇒ 源 RGB=64,不是 128)、
  `blend_multiply_uses_stage1_current_alpha_for_white_mix`
  (A=128 ⇒ 69,不是 89)。
- 验证命令与原始结果:
  - `cargo test -p yu-backend-soft` → 70 / 32 / 2 / 2 / 1 passed;
  - `cargo test -p yu-backend-wgpu --lib` → 16 passed;
  - `cargo test -p yu-backend-wgpu --test wgsl_check` → 1 passed;
  - `cargo build --workspace --all-targets` → rc 0。
- 目视:主人已在同一场景确认白块消失(<v500-relirium>)。
- 版本结论:0.479 / 0.495 / 0.500 的 MODE 5 状态序列一致,本修正
  **不需要按版本门控**,不引入 <0.495 回归。

## 残余

- MODE 3/7 的 blend 因子与 alpha 通道未在本档案逐项复测,仅按
  D3D9 固定管线同因子推导;若淡出场景仍有亮度差,优先复查
  `D3DRS_TEXTUREFACTOR` 的 RGB 取值与 `BLENDOP`。
- `SPL2101` 的 `SIZE2` 目标矩形动画(缩放/位移)未在本档案内逐帧
  对拍;白块已由 blend 归约解释,与表面加工无关。
