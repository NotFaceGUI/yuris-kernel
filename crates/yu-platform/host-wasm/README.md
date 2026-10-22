# yu-host-wasm

> **当前定位(只是调试期胶水,不是最终 wasm 包)**:现在这个 wasm 只服务于
> dev 热重启调试——页面刷新即重新加载,它只把 native 引擎(`yu-cli serve`)
> 推来的 RGBA 帧画上 canvas,全部读取与渲染语义都在 native。**后期的 wasm
> 包才是真正的编译产物**:把 `yu-rt` + `yu-gfx` + `yu-backend-soft` 整个编进
> wasm32,浏览器脱离本地服务自跑(PROGRESS 路线表"wasm 宿主");届时本
> crate 的 `present()` 就是帧生产端从 WS 换成页面内的接入缝。

浏览器**呈现端**(架构 B):读取与执行都在 native 本地服务(`yu-cli serve`),
本 crate 只接收 RGBA 帧并呈现到 canvas。**不依赖** `yu-rt`/`yu-gfx`/
`yu-backend-*`/`yu-vfs`:不读游戏数据,不解释任何渲染语义——合成语义的
真相留在 native 端做逐像素对拍,浏览器永远只是第二屏幕。

```text
WebSocket 二进制帧(YUF1)
  → www/app.js 解析头
  → Presenter::present(rgba, w, h)
       ├─ WebGPU:页面安装的 window.__YU_PRESENT_GPU__(ptr,len,w,h)
       │           (wasm 线性内存 → GPU 纹理 → 全屏 blit,零拷贝)
       └─ Canvas2D:ImageData + putImageData
```

为什么 WebGPU 的 JS 调用在页面里:`web-sys` 的 WebGPU 绑定需要
`--cfg web_sys_unstable_apis`(整棵依赖树都要带),把它变成构建命令的
隐藏前提会让别处的 `cargo check` 语义漂移。因此 GPU 对象创建/绘制由页面
里一个受控 shim 完成;wasm 决定优先级、提供帧缓冲,并在 shim 失败时回退 2D。

## 构建

`www/pkg/` 是构建产物(已 gitignore),提交前先重建:

```bash
rustup target add wasm32-unknown-unknown        # 工具链(task K2 已装 wasm-bindgen CLI)

cargo build -p yu-host-wasm --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir www/pkg --out-name yu_host_wasm \
    target/wasm32-unknown-unknown/release/yu_host_wasm.wasm
```

(debug 就用 `target/wasm32-unknown-unknown/debug/` 路径;`yu-cli bench`
的第 6 步跑的就是这两条。)

## 最小示例

不依赖任何服务:一页 HTML + 20 行 JS,把一帧合成渐变呈现到 canvas。
存成 `www/mini.html`,经任意静态伺服(ES module 不能走 `file://`),
浏览器开 `http://127.0.0.1:<port>/mini.html` 即可:

```html
<!doctype html>
<meta charset="utf-8">
<title>yu-host-wasm 最小示例</title>
<canvas id="c" width="256" height="128" style="width:512px;image-rendering:pixelated"></canvas>
<script type="module">
  // ① WebGPU shim(可选):页面装 window.__YU_PRESENT_GPU__,wasm 传
  //    (ptr,len,w,h),返回 true 表示已呈现;不装/失败则自动走 Canvas2D。
  //    完整 GPU 初始化见 www/app.js 的 initWebGpu()。
  // window.__YU_PRESENT_GPU__ = (ptr, len, w, h) => { …; return true; };

  // ② 载入 wasm 呈现器(--target web 的胶水:default() 是 init)
  const mod = await import('/pkg/yu_host_wasm.js');
  await mod.default();
  const presenter = new mod.Presenter(document.getElementById('c'));

  // ③ 呈现一帧 RGBA8:同步返回实际使用的后端("webgpu" / "2d")
  const w = 256, h = 128;
  const rgba = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++)
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4;
      rgba[i] = x; rgba[i + 1] = y * 2; rgba[i + 2] = 128; rgba[i + 3] = 255;
    }
  console.log('backend =', presenter.present(rgba, w, h));
</script>
```

### 契约

| 项 | 行为 |
|---|---|
| `new Presenter(canvas)` | 此时页面若已装好 GPU shim 则选 WebGPU,否则 Canvas2D(canvas 上下文类型一旦确定不可更改,所以选择发生在构造时) |
| `present(rgba, w, h)` | `rgba.len == w*h*4` 否则显式报错;`w/h` 为 0 显式报错;返回 `"webgpu"` 或 `"2d"`;GPU shim 返回 false 视为失败上抛 |
| `window.__YU_PRESENT_GPU__(ptr,len,w,h)` | 从 `wasmExports.memory.buffer` 的 `[ptr, ptr+len)` 读像素;整帧上传 GPU 并全屏 blit;失败返回 false |
| 帧尺寸变化 | Canvas2D 路径自动改写 canvas 宽高;WebGPU 路径由 shim 负责重建纹理 |

### 这个 .wasm 是什么

Rust 源码经 `wasm32-unknown-unknown` 编出的字节码模块(文件头
`00 61 73 6d 01 00 00 00` 即 `\0asm` v1,与 ELF/PE 同类的"机器码"格式)。
它只有算力——数值计算 + 线性内存,没有文件/网络/DOM;一切 IO 都要经
页面胶水(`www/pkg/yu_host_wasm.js`)传话。路线图上的 **wasm 宿主**
(把 `yu-rt`+`yu-gfx`+`yu-backend-soft` 整个编进来、浏览器脱离本地服务
自跑)是同一技术换更大代码量;本 crate 的 `present()` 接口就是那时帧
生产端替换的接入缝。

## 验证

```bash
cargo check -p yu-host-wasm --target wasm32-unknown-unknown   # wasm 目标编译
cargo clippy -p yu-host-wasm --all-targets                    # 原生目标 0 新告警
```
