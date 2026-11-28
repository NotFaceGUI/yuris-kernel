# DIV-GFX-099 · 原地滤镜的显式 `ID2` 目标

- 状态: **fixed**(2026-11-28;按动作族通用放行,不按版本分支)
- 影响面: `yu-gfx::state` 的 CGACT 可执行性门禁、soft/wgpu 单层滤镜回放
- 关联: DIV-GFX-061(过渡族)、DIV-014(参数名别名)

## 事实

`CGACT` 的原地滤镜族(`ROTATION`/`RESIZE`/`TRIM`/`MEIDO`/`SAIDO`/…)
官方语义是"读源层像素 → 写目标层";`ID2` 存在时目标层是 `ID2`,缺省
`target == source`。手册示例只列原地写法,但真实商业脚本使用显式目标:

- `<0.263 商业>` `_productlib` s171 pc35:
  `CGACT[ROTATION=1 ID="LOGO.BUF" ID2="LOGO.1" SET=@X]`,随后
  `CG[ID="LOGO.1" Y=…]` 绘制旋转结果 —— 标题 LOGO 旋转动画。
- `<0.290 商业>` s38 pc37 同型。

修复前门禁把"显式目标 + target != source"一律判
`gfx.imageop.id2.unsupported`,两处旋转动画整条被跳过。

## 决策

门禁改为按**动作族**放行:`action.is_in_place_filter()` 为真时允许
`ID2` 目标。非原地动作(合成/缩放等)的 `ID2` 语义仍显式未实现。

**后端补充(同日修复)**:放行门禁只解决"命令被跳过";soft/wgpu 的单层
滤镜路径实际是从**目标层基面**起算的,`ID2 != ID` 时旋转的是空白目标。
两个后端已改为:显式目标且源≠目标时,先取**源层**像素(含 source
override/快照基面),加工后写目标层。像素级回归用例仍缺(挂账:
`docs/tasks/open-findings-2026-11-28.md` E1)。

## 验证

- `crates/yu-gfx/tests/parity_fixes.rs::in_place_filter_with_id2_writes_target`
  (ROTATION + ID2,断言 op 源/目标/可执行);
- `<0.263 商业>` 首帧 `ops: 9/9 drawn, 0 skipped`(修复前 3 条 skipped);
- `<0.290 商业>` 首帧 `ops: 19/19 drawn, 0 skipped`;
- 实机:0.263/0.290 标题 LOGO 旋转动画中间漩涡完整(手工帧);
- 0.495 回归:现有 transition/filter 测试全绿。
