# DIV-GFX-005 · `MID` 外部掩码 = `target.a = mask.R`

- 状态:**fixed(no-manual;图像层)**;组图层 `open(evidence)`
- 关联:DIV-GFX-013(图像合成)、DIV-GFX-041(组 RT)
- 取证:官方 0.495 `yu-ris.exe` CG 处理体 `0x42a514`,
  sha256 `4f5eefa3544c7244a907477d89ccee97286f559df5e38b1b1fa674206981138f`
- 日期:2026-11-05

## 1. 官方行为(REA `0x42a801..0x42a8a4`)

`CG` 在 `FILE` 处理之后检查 `MID`(slot 35,存在标志 `0x684183`):

```text
0x42a801  movzx edx, byte [0x684183]   ; MID 标志
0x42a808  test / je 0x42a8cd           ; 未给 MID ⇒ 下一参数
0x42a810  mov edx, [edx*4+0x683fc0]    ; MID 字符串
0x42a817  movsx ecx, byte [edx] / test / je 0x42a8cd  ; 空串跳过
0x42a822  ... path 规范化(FUN_004141e4)+ 查层表(FUN_00414559)
0x42a850  mov edi, [eax+0x34]          ; mask 层对象
0x42a857  mov esi, [ebp+0x78]          ; target 宽
0x42a85a  cmp esi, [edi+0x78] / jne 0x42a8ad   ; 宽不等 ⇒ 错误
0x42a85f  mov edx, [ebp+0x7c]          ; target 高
0x42a862  cmp edx, [edi+0x7c] / jne 0x42a8ad   ; 高不等 ⇒ 错误
0x42a86b  .. 逐像素:
0x42a888    movzx edx, byte [mask+0x90 + i*4]      ; mask **R** 通道
0x42a88c    mov byte [target+0x90 + i*4 + 3], dl    ; 写入 target alpha
0x42a8a4  mov byte [ebp+0x86], 1                    ; alpha 有效标志
```

⇒ `MID` 是**命令时刻**的逐像素 alpha 替换:
`target.alpha = mask.R`,RGB 不动;两层宽高必须相等。

## 2. 内核实现

- `CG[MID]` 由 `record_mask_op` 记录为 `ImageAction::Other("mask_alpha")`
  合成 op(`source = mask 层`,`target = 被掩码层`),在命令序位置执行;
- 重复 MID 命令按 `(action,target)` 折叠为最后一条(纯写,不读目标旧 alpha);
- `Layer.mask` 保留在帧 JSON 供审计;目标层 `alpha_aware=true`(对应 `+0x86=1`);
- soft:`mask_alpha` 分支检查宽高相等后 `target.a = mask.r`;
- wgpu:`composite.wgsl` mode 3 = `vec4(lower.rgb, upper.r)`;执行前同样按
  实际纹理宽高拒绝不等尺寸(层声明尺寸未知时不能由 gfx 提前判定),
  与 soft 的跳过口径一致(`gfx.mask.size.mismatch`)。
- 组图层:组合成表面的 alpha 写入路径未实现,保留基图绘制 + recoverable
  诊断(`open(evidence)`,与 041 同族)。

## 3. 回归

```text
cargo test -p yu-backend-soft --test render mid_uses_mask
  mid_uses_mask_red_channel_as_layer_alpha ... ok
cargo test -p yu-backend-wgpu --test wgpu mid_mask_red
  mid_mask_red_channel_matches_soft ... ok   (GPU 与 soft RGBA 逐字节一致)
cargo test -p yu-backend-wgpu --test wgpu mid_mask_size_mismatch
  mid_mask_size_mismatch_is_skipped_by_both_backends ... ok
  (2×2 目标 / 3×3 mask:两后端都 skipped=1,基图仍绘制)
cargo test -p yu-gfx --test parity_fixes mid_keeps
  mid_keeps_layer_drawable_and_retains_mask ... ok
```

缺层/尺寸不等时保留基图并出 `gfx.mask.unsupported` recoverable 诊断,
不再整层跳过。
