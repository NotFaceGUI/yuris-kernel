# yu-platform-input · 宿主侧平台输入翻译

> 内核输入语义在 `yu-rt::input`(带符号帧计数器、`INPUT`/`MOUSE` 门控、
> `_INP`);本 crate **不**碰脚本语义,只做平台边界。

## 分层

```text
winit / DOM / Android 事件
        │ 物理键码、指针按钮、滚轮增量
        ▼
yu-platform-input        ← 唯一键位表/光标策略
        │ 引擎按钮名(KEY_* / MOUSE_*) + CursorShape
        ▼
yu-cli::Engine           ← 原始事件防抖(极短点击至少一帧)、时间线桥
        ▼
yu-rt::input             ← 计数器/源开关/脚本写回钳制
```

- **键位表唯一来源**:`yu_rt::input::button_code()` 的 ABI TSV。表里没有
  的物理键显式返回 `None`,不猜。
- **桌面**:`host-desktop` 把 winit `KeyCode` 的 debug 名传给
  `key_button`;鼠标按钮/滚轮走 `PointerButton`/`wheel_button`。
- **浏览器**:`www/app.js` 只上送 `{t:'key',code}` / `{t:'pointer',button}` /
  `{t:'wheel',dy}`;`yu-cli serve` 在本 crate 里映射,页面不再复制键表。
- **光标**:`cursor_shape` + `cursor_visible` 是平台无关策略;桌面映射到
  winit `CursorIcon`,后续 wasm/Android 映射到 CSS/系统光标。

## 为什么极短点击要延迟抬起

`Engine::set_input(false)` 若发现按下还没被任何逻辑帧消费,会延到该帧之后
再抬(`deferred_release`)。否则“按下+抬起都发生在两次 tick 之间”的快速点击
会被时间线折叠掉——自动化点击和低帧率下的真实点击都会丢。

## 验证

```text
cargo test -p yu-platform-input
cargo test -p yu-cli --test input_settings -- --ignored
```
