# DIV-GFX-100 · `CGACT[RESIZE]` 的 `LEFT/RIGHT/TOP/BOTTOM` 边距别名

- 状态: **fixed(no-manual)**(2026-11-28)
- 影响面: `yu-gfx::state` 的 CGACT 参数读取
- 关联: DIV-GFX-063(图像滤镜取证)、DIV-099

## 事实

0.292 的 `CGACT` 参数表只有 `X/Y/X2/Y2`;0.290 商业构建的表在
`SIZE` 之前多出 `LEFT/RIGHT/TOP/BOTTOM` 四个整数槽:

```text
[2] CGACT … 15:LEFT 16:RIGHT 17:TOP 18:BOTTOM 19:SIZE 20:SIZE2 …
```

真实脚本只用这四个键发 `RESIZE`:

```text
CGACT[ID=… RESIZE=1 BOTTOM=n LEFT=n]     (0.290 s93 pc1619..)
```

0.292 手册 `cg/cgact.html` 没有这四个键的页(手册缺页);RESIZE 的
`X/Y/X2/Y2` 公式已由官方 `FUN_0043c971` 取证:
`newW = srcW + X + X2`、`newH = srcH + Y + Y2`。

## 决策

把 `LEFT/RIGHT/TOP/BOTTOM` 作为 `X/X2/Y/Y2` 的**通用别名**接入参数解析
(`X/Y/X2/Y2` 优先);不按版本号分支。没有这四个键的工程表不会产生任何
参数,行为不变。

## 验证

- `<0.290 商业>` 静态普查:`CGACT[LEFT]=8`、`[RIGHT]=3`、`[BOTTOM]=23`、
  `[RESIZE]=23`,首次位置 `s93 pc1619`;首帧启动无 `UNIMP`;
- 0.495/0.466 语料回归:参数表无这些键,`RESIZE` 仍走原 `X/Y/X2/Y2`
  (`resize_updates_layer_rect_for_cginfo` 等测试全绿)。
