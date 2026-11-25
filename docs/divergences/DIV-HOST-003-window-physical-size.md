# DIV-HOST-003 · 桌面窗口按逻辑像素建窗,150% DPI 下比官方大 1.5 倍

- 状态: fixed
- 优先级: 中(所有 v555 工程窗口尺寸对不上)
- 发现日期 / 发现方式: 2026-11-25 / `<game-c>` 桌面日志 + 官方 exe manifest/REA

## 复现

- 游戏位置: `<game-c>`(yscfg 1920×1080);对照 `<v555-2>`(1280×720)
- 锚点: `yu-desktop: window=2880x1620 viewport=1920x1080`

## 官方行为

官方 v555 exe 的资源 manifest:

```xml
<dpiAware>True/PM</dpiAware>
```

`FUN_00457788` 用 `in_ECX[3]/[4]`(yscfg 宽高)填 `SetRect` 后
`AdjustWindowRectEx` → `CreateWindowExA`。DPI-aware 进程里这就是
**物理像素**客户区;150% 缩放下 1920×1080 工程仍是 1920×1080 物理窗口。

## 内核行为

`host-desktop` 用 `LogicalSize::new(viewport)`;winit 按窗口 scale factor
(1.5)换算,1920×1080 变成 2880×1620 物理像素。

## 根因

逻辑尺寸/物理尺寸口径与官方相反。`--width/--height` 也随之被乘。

## 修法

`resumed()` 改用 `PhysicalSize::new(opts.width.unwrap_or(vw), ...)`。
`with_min_inner_size` 保持 LogicalSize(最小尺寸不是游戏内容契约)。

## 回归

- 桌面实测: `<game-c>` `window=1920x1080 viewport=1920x1080`;
  `<v555-2>` `1280x720`; `<game-b>`(v494) `1100x825` —— 均等于各自 yscfg。
- `cargo build --workspace --all-targets` → rc 0。

## 残余

`ScriptAction::Resize`(`--script` 用例)仍用 LogicalSize;用例脚本的
宽高语义需单独定口径(当前无 v555 用例依赖它)。
