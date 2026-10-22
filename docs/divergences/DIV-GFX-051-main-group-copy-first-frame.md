# DIV-GFX-051 · 主组直绘后 `COPY ID=:/W0/` 的首帧源缺失

- 状态: open
- 优先级: 高(首帧 golden 与全部 `cases/` 依赖)
- 发现日期 / 方式: 2026-10-22 / 代码审查 + `first_frame` 实测

## 现象

`first_frame`(tick 1)原始输出:

```text
frame: 3   layers: 272 (visible 8)   passes: 14   offscreen passes: 1
ops: 17 total, 12 drawn, 5 skipped
  skip copy  x1  gfx.copy.source.offscreen_missing      :/W0/CGS100
  skip image x1  gfx.imageop.copy_source.unsupported    :/W0/CGS100
```

`eris_first_screen_display.json` 中同一条 `Op::Copy` 仍标
`"status":{"executable":true}`。

## 根因

两条已关闭的线之间缺少衔接:

1. J3(EVID-J3-07 / DIV-GFX-008)让 `build_group_passes` 跳过主组
   `:/W0` —— 主组不再产出 `PassTarget::Offscreen`;
2. J2(EVID-T-11/12 / DIV-GFX-007)的 COPY 源解析仍要求组有离屏
   pass:
   - soft:`compose_group` → `gfx.copy.source.offscreen_missing`;
   - wgpu:`plan_group_compose` 找不到 pass 时返回 `Ok(())`,随后
     `group_image` → `gfx.copy.source.offscreen_missing`。

主组直绘是正确的(有 EVID-J3-07 逐帧对拍),COPY 也是正确的(有
EVID-T-11/12/15 微场景);错的是两者的组合:**主组现在是“没有离屏纹理
的组”,而 COPY 的 group 源假设它一定有一块。**

## 官方行为

- `FUN_0042f809` 的 COPY 分支直接改纹理,不读“上一帧”缓冲;
- 受控微场景 `fidelity-micro6h` / `j2-micro-copy2` 里 `COPY2 ID=:/W0/`
  读的是**当时的后缓冲**(EVID-J3-06);
- **首帧**(后缓冲尚未建立)的官方行为没有取证:现有微场景都至少跑过
  一帧。

## 内核行为

- 首帧:`last_screen`/`backbuffer` 为 `None` ⇒ soft 回退 `compose_group`、
  wgpu 回退 `group_image` ⇒ 两者都找不到主组 Offscreen ⇒ 跳过;
- 第二帧起:`last_screen` 有值 ⇒ 走“后缓冲”分支,正常。

## 二分记录

- [x] L1 译码一致(同一份 ybn,命令顺序一致)
- [x] L2 执行/事件流一致(COPY 事件照发)
- [x] L3 帧层状态一致(层树/可见数一致,仅这条 op 被跳过)
- [ ] L4 帧间编排:首帧与后续帧的源不同,未取证官方首帧语义

## 修法(二选一,需裁决)

1. **回退本帧子层自合成**:`frame()` 为主组 COPY 额外产出一个
   `PassTarget::Texture(main)` 的自合成项(或让后端对“主组 + 无后缓冲”
   按 `emit_children_ops` 临摹一块表面)。语义与 J2 的 `compose_group` 等价。
2. **显式挂账**:把原因码改成 `gfx.copy.source.no_backbuffer`,状态保持
   `Unimplemented`,并在报告里计数。这样“未取证”不会被伪装成实现缺陷。

倾向 2 先落地(不改像素、不猜),再用 regtest 微场景补证据后转 1。

## 回归

- 对拍用例:`cases/first-screen.json`(当前期望 `ops_total=18`,实测 17);
- 验证命令:
  `cargo run -p yu-backend-soft --example first_frame`;
- 修好后需同时重生成 `eris_first_screen_display.json` 与三条 case。
