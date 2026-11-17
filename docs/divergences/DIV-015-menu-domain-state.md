# DIV-015 · `MENU` 域状态在 gfx 落地(MENU/MENUEND),MENUACT/MENUINFO 仍显式挂号

- 状态: **fixed(MENU/MENUEND)/ open(MENUACT/MENUINFO)**
- 优先级: 中(菜单栏定义;2xx 启动链必经)
- 发现日期 / 发现方式: 2026-11-17,0.292/0.255 首启诊断;手册
  `menu/menu.html`、`menu/menuact.html`、`menu/menuinfo.html`、
  `menu/menuend.html`。

## 事实

- 手册: `MENU` 建立/修改菜单层(`ID` 图层 ID;`NAME` 项名,空串 =
  分隔线;`E` 存在/无效;`A` 0..256 灰色;`Z` 优先级;`CHECK` 复选;
  `CODE` 被选中时写入 `@_MENUCODE`)。`MENUEND` 删除。菜单本身不显示,
  要 `WINDOW[MENUID=:/M0]` 才绑定为窗口菜单栏。
- 0.292/0.255 的 `userdefine/メニューバー定義.txt` 在启动链执行
  `MENU[ID=END/... NAME="終了" CODE=1001]` 等定义;修复前该域未接线,
  每次启动报 `rt.cmd.unimplemented`。
- 内核没有原生窗口菜单栏(桌面窗口/无头都不建 OS 菜单),但**菜单层状态**
  是命令语义的一部分,可保留给宿主消费。

## 实现

- `yu-gfx::state`:新增 `menu: BTreeMap<Vec<u8>, MenuEntry>`;
  `dispatch_menu` 处理 `MENU`(增量覆盖 NAME/E/A/Z/CHECK/CODE)与
  `MENUEND`(按 ID 删除);不渲染、不绑定窗口。
- `yu-rt` 域分派:仅当 `graphics_host` 安装且命令名为 `MENU`/`MENUEND`
  时视为已消费;`MENUACT`/`MENUINFO` **不**被吞掉(仍走原 Unimplemented),
  避免按域整体放过真未实现。
- 手册有页但 MENUACT/MENUINFO 暂无宿主状态机;保留显式诊断,不假装实现。

## 验证

- 0.292/0.255 headless 60 帧:`UNIMP=0`(修复前各 1 条 MENU);
- 菜单定义进入 `GfxState` 状态表,标题/按钮流程不受影响;
- `MENUACT`/`MENUINFO` 未在样本启动路径出现;若脚本调用仍显式挂号。
