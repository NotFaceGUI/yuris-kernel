# DIV-GFX-063 · 无手册页滤镜/合成族(`partial(no-manual)`)

> 状态:`partial(no-manual)`。按 AGENTS §0 的 2026-10-24 政策,手册缺页
> 不等于不实现:凡 `yscm` 参数表存在、且语料有实际调用的动作,一律给出
> 执行体并标注证据级别;语义未取证的部分在此登记,不伪装成已对齐。

## 1. 已落地(证据 = yscm 参数表 + 语料调用)

| 动作 | 参数 | 实现位置 | 证据级别 |
|---|---|---|---|
| `ROTATION` | `SET`(角度) | `imagefilters::apply_rotation` | 手册有页 |
| `ROTATION2` | `SET` | 同上 | `partial(no-manual)` |
| `SAIDO` | `SET`(0..256,128=原样) | `apply_saido` | 手册有页 |
| `AKARUSA` | `SET`(0..256,128=原样) | `apply_akarusa` | 手册有页 |
| `MONO` | `SET`(阈值) | `apply_mono` | 手册有页 |
| `MOSAIC` | `SET`(块边长) | `apply_mosaic` | 手册有页 |
| `BLUR` | `SET`(半径) | `apply_blur` | 手册有页 |
| `BLUR2..BLUR6` | `SET` | 复用 `apply_blur` | `partial(no-manual)`:强度映射未取证 |
| `DIFFUSION` | `SET`(强度) | `apply_diffusion` | 手册有页 |
| `WHIRLPOOL` | `SET`(进度) | `apply_whirlpool` | 手册有页 |
| `RASTER` | `SET`(宽)+`SET2`(进度) | `apply_raster` | 手册有页 |
| `RASTER2..3` | 同上 | 同上 | `partial(no-manual)` |
| `SEPIA` | 无 | `apply_sepia` | `partial(no-manual)`:系数按 BT.601 取,未对引擎取证 |
| `XOR` / `AND` | `ID`(上层) | `apply_bitwise` | `partial(no-manual)`:RGB 逐通道,alpha 规则未取证 |
| `MERGE` | `ID`(上层) | `apply_merge(upper,false)` | `partial`:alpha 公式仅手册文字依据 |
| `MERGE2` | `ID`(上层) | `apply_merge(upper,true)` | `partial`:同上 |
| `RESIZE` | `X/Y/X2/Y2` | `imageop::apply_resize` | 几何已对 REA,滤波未取证 |
| `TRIM` | `SET` | `imageop::apply_trim` | 手册有页 |
| `MEIDO` | `SET` | `imageop::apply_meido` | `partial(no-manual)` |
| `VERTICAL` | 无 | `imageop::apply_vertical` | 手册例 10/11 + 语料 `:8233/8240` |
| `TRANSITION` | `SET`+`ID2`(第二输入)+`ID3`(输出) | `imageop::blend_transition` | 见 DIV-GFX-061 |

## 2. 明确未取证的点

1. **`BLUR2..BLUR6` 各自的强度/半径映射**:当前一律按 `BLUR` 的半径处理。
   语料里这几个键的出现次数极少,没有可对拍的像素基准。
2. **`SEPIA` 系数**:采用整数版 BT.601 近似。引擎是否用同一组系数、
   是否先做 gamma 处理,均无证据。
3. **`XOR`/`AND` 的 alpha 通道**:当前只作用于 RGB,alpha 保留下层。
   引擎侧的混合公式未反出。
4. **`MERGE`/`MERGE2` 的 alpha 公式**:目前依据手册文字(`MERGE` 按
   上层 alpha 插值,`MERGE2` 直接覆盖)。缺 REA 级验证。
   - **2026-10-28 新增待取证点(表面已被同帧清空时的 alpha)**:ERIS 立绘换
     差分的命令序是 `COPY2 → RECTPAINT3 SET=0 → MERGE`(`es_SPExec.yst`)。
     官方在同一表面**就地**执行(`FUN_0042f809`:`*(iVar4+0x90) = local_68`),
     清空后再 MERGE 的结果在实机上必然可见 ⇒ 官方这次绘制**必然写了 alpha**,
     与「`MERGE` 保留目标 alpha」矛盾。缺直接 REA 证据,故不猜。
     - 现状:GPU 快照合成路径在该形态下回退 CPU(基面 = `snapshot_pixels()`
       的 COPY 时刻冻结像素,与回归前一致);未清空/未改写的快照仍走 GPU。
       见 `PROGRESS.md` 成果 `SPRITE-FLASH-GPU-MERGE` 与门禁
       `merge_after_rectpaint_clear_on_snapshot_keeps_pixels`。
     - 待办:补 REA(`FUN_004405a4` 调用点的 `SetRenderState` 混合状态 +
       目标表面 alpha 变化),再定 `MERGE` 是否应写 alpha。
5. **`RESIZE` 的缩放滤波**:`SIZE` 族用最近邻/双线性区分,`RESIZE` 走
   双线性;官方是否另有滤波选择未取证。

## 3. 不做的部分(显式排除)

- `CGINFO[TEX]`(E-mote 判定)仍属 `deferred(master)`。
- `TEXCOPY`/`TEXSSCOPY` 属纹理拷贝族,不在此档案内。

## 4. 验证

```text
cargo test -p yu-gfx --test parity_fixes     # 23 passed
cargo test -p yu-backend-soft --lib          # 43 passed(含 11 条滤镜像素单测)
```

单测锚点(`crates/yu-render/soft/src/imagefilters.rs`):
`saido_64_is_half_saturation`、`akarusa_192_is_1_5x`、
`mono_threshold_splits_black_white`、`mosaic_fills_blocks_uniformly`、
`blur_preserves_uniform_color`、`zero_params_are_identity`、
`merge_transparent_upper_keeps_lower`、`merge2_opaque_upper_overwrites`、
`bitwise_applies_to_rgb_only`。
