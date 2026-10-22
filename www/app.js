// yuris-kernel · 浏览器端(app.js)
//
// 职责:连接本地服务 → 接收 RGBA 帧 → 交给 wasm Presenter 呈现;
// 把鼠标/键盘输入发回服务端。**不解释任何渲染语义**。
//
// WebGPU 呈现由本文件里一个受控 shim 完成(web-sys 的 WebGPU 绑定需要
// 不稳定的 cfg;理由见 crates/yu-host-wasm/src/lib.rs)。wasm 决定优先级
// 并在 shim 不可用时回退 Canvas2D。

const FRAME_MAGIC = 0x59554631; // "YUF1"
const FRAME_HEADER = 56;

const canvas = document.getElementById('screen');
const stageEl = document.getElementById('stage');
const connPill = document.getElementById('conn');
const loadingEl = {
  panel: document.getElementById('loading'),
  text: document.getElementById('loading-text'),
};
const ui = {
  frame: document.getElementById('frame'),
  ops: document.getElementById('ops'),
  skipped: document.getElementById('skipped'),
  executed: document.getElementById('executed'),
  ms: document.getElementById('ms'),
  present: document.getElementById('present'),
  engine: document.getElementById('engine'),
  viewport: document.getElementById('viewport'),
  entry: document.getElementById('entry'),
  marker: document.getElementById('marker'),
  hint: document.getElementById('hint'),
  statusLine: document.getElementById('status-line'),
  error: document.getElementById('error'),
  restart: document.getElementById('restart'),
  pause: document.getElementById('pause'),
  frameBack: document.getElementById('step-back'),
  frameFwd: document.getElementById('step-fwd'),
};

const logEl = {
  panel: document.getElementById('log'),
  lines: document.getElementById('log-lines'),
  count: document.getElementById('log-count'),
  toggle: document.getElementById('log-toggle'),
  clear: document.getElementById('log-clear'),
  report: document.getElementById('log-report'),
};
const LOG_MAX = 300;
let logTotal = 0;
let lastFrameLog = 0;
let lastReportKey = '';

let presenter = null;
let wasmModule = null;
let wasmExports = null;
let js2d = null;
let lastPresent = 'none';
let status = null;
let paused = false;
let latest = { width: 0, height: 0 };
let lastOverlay = 0;
// boot() 在模块求值早期就被调用(?log=1 时同步走 setLogVisible/restoreLogLayout),
// 这两个必须先于 boot() 声明,否则撞 let/const 的 TDZ。
const LOG_LAYOUT_KEY = 'yu-log-layout';
let logPlaced = false;

boot().catch((error) => {
  showError(`启动失败: ${error?.message ?? error}`);
});

// ---------------------------------------------------------------- log module
//
// 页面内日志:连接/hello/帧摘要/输入/错误 + 服务端最近一帧的 skipped 原因。
// 环形缓冲 300 行;Log 按钮显隐,`?log=1` 自动打开。

function log(level, message) {
  if (!logEl.lines) return;
  const lines = logEl.lines;
  const now = new Date();
  const ts =
    String(now.getHours()).padStart(2, '0') + ':' +
    String(now.getMinutes()).padStart(2, '0') + ':' +
    String(now.getSeconds()).padStart(2, '0') + '.' +
    String(now.getMilliseconds()).padStart(3, '0');
  // 只在本来就贴底时跟随滚动;用户上翻阅读时不打扰。
  const stick = lines.scrollHeight - lines.scrollTop - lines.clientHeight < 24;
  const line = document.createElement('div');
  line.className = `log-line ${level}`;
  line.textContent = `${ts} ${message}`;
  lines.appendChild(line);
  while (lines.childElementCount > LOG_MAX) {
    lines.removeChild(lines.firstChild);
  }
  logTotal += 1;
  if (logEl.count) logEl.count.textContent = String(logTotal);
  if (stick) lines.scrollTop = lines.scrollHeight;
}

function setLogVisible(visible) {
  if (!logEl.panel) return;
  logEl.panel.hidden = !visible;
  logEl.toggle?.classList.toggle('active', visible);
  if (visible) {
    if (logPlaced) {
      // 拖动过的面板重新打开时夹回视口内。
      const rect = logEl.panel.getBoundingClientRect();
      positionLogPanel(rect.left, rect.top);
    }
    logEl.lines.scrollTop = logEl.lines.scrollHeight;
  }
}

// ---------------------------------------------------------------- log layout
//
// Log 面板布局:header 拖动移位,右下角原生 resize;位置/尺寸存 localStorage。
// (LOG_LAYOUT_KEY / logPlaced 声明在顶部状态区,原因见彼处注释。)

function positionLogPanel(left, top) {
  const panel = logEl.panel;
  const x = Math.min(Math.max(0, left), window.innerWidth - panel.offsetWidth);
  const y = Math.min(Math.max(0, top), window.innerHeight - panel.offsetHeight);
  panel.style.left = `${x}px`;
  panel.style.top = `${y}px`;
}

function saveLogLayout() {
  if (!logEl.panel) return;
  const rect = logEl.panel.getBoundingClientRect();
  try {
    localStorage.setItem(LOG_LAYOUT_KEY, JSON.stringify({
      left: Math.round(rect.left),
      top: Math.round(rect.top),
      w: Math.round(rect.width),
      h: Math.round(rect.height),
    }));
  } catch { /* 存取失败不致命 */ }
}

function restoreLogLayout() {
  if (!logEl.panel) return;
  try {
    const saved = JSON.parse(localStorage.getItem(LOG_LAYOUT_KEY) || 'null');
    if (!saved) return;
    logEl.panel.style.width = `${saved.w}px`;
    logEl.panel.style.height = `${saved.h}px`;
    logEl.panel.style.right = 'auto';
    logEl.panel.style.bottom = 'auto';
    positionLogPanel(saved.left, saved.top);
    logPlaced = true;
  } catch { /* 忽略坏数据 */ }
}

function initLogDrag() {
  const header = logEl.panel?.querySelector('header');
  if (!header) return;
  let drag = null;
  header.addEventListener('pointerdown', (event) => {
    if (event.target.closest('button')) return;
    const rect = logEl.panel.getBoundingClientRect();
    logEl.panel.style.right = 'auto';
    logEl.panel.style.bottom = 'auto';
    drag = { dx: event.clientX - rect.left, dy: event.clientY - rect.top };
    header.classList.add('dragging');
    header.setPointerCapture(event.pointerId);
    event.preventDefault();
  });
  header.addEventListener('pointermove', (event) => {
    if (!drag) return;
    positionLogPanel(event.clientX - drag.dx, event.clientY - drag.dy);
  });
  const endDrag = () => {
    if (!drag) return;
    drag = null;
    header.classList.remove('dragging');
    saveLogLayout();
  };
  header.addEventListener('pointerup', endDrag);
  header.addEventListener('pointercancel', endDrag);
}

// ---------------------------------------------------------------- loading

// restart/启动期间的遮罩:黑底 + 转圈,盖住 canvas(点 Restart 立即黑屏);
// 首帧到达(handleBinary)后撤掉。

function showLoading(text) {
  if (!loadingEl.panel) return;
  if (loadingEl.text) loadingEl.text.textContent = text;
  placeLoading();
  loadingEl.panel.hidden = false;
}

function placeLoading() {
  if (!loadingEl.panel) return;
  const stageRect = stageEl.getBoundingClientRect();
  const rect = canvas.getBoundingClientRect();
  loadingEl.panel.style.left = `${rect.left - stageRect.left}px`;
  loadingEl.panel.style.top = `${rect.top - stageRect.top}px`;
  loadingEl.panel.style.width = `${rect.width}px`;
  loadingEl.panel.style.height = `${rect.height}px`;
}

function hideLoading() {
  if (loadingEl.panel) loadingEl.panel.hidden = true;
}

function logFrame() {
  if (latest.frame === undefined) return;
  const now = performance.now();
  if (now - lastFrameLog < 500) return;
  lastFrameLog = now;
  log('dim', `frame=${latest.frame} ops=${latest.ops} skipped=${latest.skipped} executed=${latest.executed} ms=${latest.ms} present=${lastPresent}`);
}

async function logServerReport(force = false) {
  try {
    const response = await fetch('/api/report', { cache: 'no-store' });
    const report = await response.json();
    if (!report.ok) return;
    const key = JSON.stringify(report);
    if (!force && key === lastReportKey) return;
    lastReportKey = key;
    const skipped = (report.skipped || [])
      .map((item) => `${item.kind}:${item.reason} x${item.count} (${item.target})`)
      .join(', ') || 'none';
    const missing = (report.missing || [])
      .slice(0, 4)
      .map((item) => `${item.code}:${item.key}`)
      .join(', ');
    const level = report.skipped_total > 0 || (report.missing || []).length > 0 ? 'warn' : 'dim';
    log(level, `report frame=${report.frame} visible=${report.visible}/${report.layers} passes=${report.passes} ops=${report.ops_drawn}/${report.ops_total} skipped=${report.skipped_total} missing=${(report.missing || []).length}${missing ? ` [${missing}]` : ''} text=${report.text_runs} | ${skipped}`);
  } catch (error) {
    log('err', `report: ${error?.message ?? error}`);
  }
}

window.addEventListener('error', (event) => log('err', `window error: ${event.message}`));
window.addEventListener('unhandledrejection', (event) => log('err', `unhandled rejection: ${event.reason}`));

async function boot() {
  const params = new URLSearchParams(location.search);
  // 原始像素模式(调试/像素对拍):?pixel=1 用最近邻放大。
  if (params.get('pixel') === '1') {
    canvas.style.imageRendering = 'pixelated';
  }
  if (params.get('log') === '1') setLogVisible(true);
  restoreLogLayout();
  initLogDrag();
  window.addEventListener('resize', () => {
    if (loadingEl.panel && !loadingEl.panel.hidden) placeLoading();
  });
  showLoading('loading…');
  log('dim', `page ${location.href}`);
  log('dim', 'loading wasm presenter…');
  const wasmReady = await loadWasmPresenter();
  if (wasmReady) {
    // 只有 wasm 可用时才碰 WebGPU:canvas 的上下文类型一旦确定不可更改。
    const gpu = await initWebGpu();
    setHint(gpu ? 'present: WebGPU' : 'present: Canvas2D(WebGPU 不可用)');
    presenter = new wasmModule.Presenter(canvas);
    lastPresent = presenter.backend;
    log('ok', `presenter ready: ${lastPresent}`);
  } else {
    setHint('present: Canvas2D(wasm 未构建,JS 回退)');
    lastPresent = '2d-js';
    log('warn', 'wasm presenter unavailable; JS Canvas2D fallback');
  }
  updateOverlay(true);
  connect();
  bindInput();
  setInterval(() => {
    if (!logEl.panel.hidden) logServerReport(false);
  }, 2000);
}

async function loadWasmPresenter() {
  try {
    wasmModule = await import('/pkg/yu_host_wasm.js');
    wasmExports = await wasmModule.default();
    return true;
  } catch (error) {
    console.warn('wasm presenter unavailable:', error);
    return false;
  }
}

// ---------------------------------------------------------------- WebGPU shim

async function initWebGpu() {
  const forced = new URLSearchParams(location.search).get('present');
  if (forced === '2d') return false;
  if (!('gpu' in navigator) || !navigator.gpu) return false;
  try {
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) return false;
    const device = await adapter.requestDevice();
    const format = navigator.gpu.getPreferredCanvasFormat();
    const module = device.createShaderModule({ code: SHADER });
    const pipeline = device.createRenderPipeline({
      layout: 'auto',
      vertex: { module, entryPoint: 'vs' },
      fragment: { module, entryPoint: 'fs', targets: [{ format }] },
      primitive: { topology: 'triangle-list' },
    });
    const sampler = device.createSampler({ magFilter: 'nearest', minFilter: 'nearest' });
    // 到这里都没有碰 canvas;失败可以直接回退 2D。
    const context = canvas.getContext('webgpu');
    if (!context) return false;
    context.configure({ device, format, alphaMode: 'opaque' });

    let texture = null;
    let textureWidth = 0;
    let textureHeight = 0;
    let lost = false;
    device.lost.then((info) => {
      lost = true;
      showError(`WebGPU device lost: ${info.message || info.reason}`);
    });

    window.__YU_PRESENT_GPU__ = (ptr, len, width, height) => {
      if (lost) return false;
      try {
        if (!wasmExports || !wasmExports.memory) return false;
        if (width !== textureWidth || height !== textureHeight) {
          texture?.destroy();
          texture = device.createTexture({
            size: [width, height, 1],
            format: 'rgba8unorm',
            usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
          });
          textureWidth = width;
          textureHeight = height;
          if (canvas.width !== width) canvas.width = width;
          if (canvas.height !== height) canvas.height = height;
        }
        const pixels = new Uint8Array(wasmExports.memory.buffer, ptr, len);
        device.queue.writeTexture(
          { texture },
          pixels,
          { bytesPerRow: width * 4, rowsPerImage: height },
          { width, height },
        );
        const bindGroup = device.createBindGroup({
          layout: pipeline.getBindGroupLayout(0),
          entries: [
            { binding: 0, resource: texture.createView() },
            { binding: 1, resource: sampler },
          ],
        });
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginRenderPass({
          colorAttachments: [
            {
              view: context.getCurrentTexture().createView(),
              loadOp: 'clear',
              storeOp: 'store',
              clearValue: { r: 0, g: 0, b: 0, a: 1 },
            },
          ],
        });
        pass.setPipeline(pipeline);
        pass.setBindGroup(0, bindGroup);
        pass.draw(3);
        pass.end();
        device.queue.submit([encoder.finish()]);
        return true;
      } catch (error) {
        console.error('webgpu present failed:', error);
        return false;
      }
    };
    return true;
  } catch (error) {
    console.warn('webgpu init failed:', error);
    return false;
  }
}

const SHADER = /* wgsl */ `
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

struct VertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VertexOut {
  var positions = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0),
  );
  let p = positions[index];
  var out: VertexOut;
  out.position = vec4<f32>(p, 0.0, 1.0);
  out.uv = vec2<f32>((p.x + 1.0) * 0.5, (1.0 - p.y) * 0.5);
  return out;
}

@fragment
fn fs(input: VertexOut) -> @location(0) vec4<f32> {
  return textureSample(frame_texture, frame_sampler, input.uv);
}
`;

// ---------------------------------------------------------------- frames

function handleBinary(buffer) {
  if (buffer.byteLength < FRAME_HEADER) return;
  const view = new DataView(buffer);
  if (view.getUint32(0, false) !== FRAME_MAGIC) return;
  const width = view.getUint32(4, true);
  const height = view.getUint32(8, true);
  const frame = Number(view.getBigUint64(12, true));
  const ops = view.getUint32(20, true);
  const skipped = view.getUint32(24, true);
  const executed = view.getUint32(28, true);
  const passes = view.getUint32(32, true);
  const layers = view.getUint32(36, true);
  const visible = view.getUint32(40, true);
  const ms = view.getUint32(44, true);
  const backend = view.getUint8(48);
  const flags = view.getUint8(49);
  const pausedFrame = (flags & 4) !== 0;
  const rgbaLength = view.getUint32(52, true);
  if (FRAME_HEADER + rgbaLength > buffer.byteLength) return;
  const rgba = new Uint8Array(buffer, FRAME_HEADER, rgbaLength);

  latest = { width, height, frame, ops, skipped, executed, passes, layers, visible, ms };
  latest.drawable = (flags & 1) !== 0;
  latest.engineBackend = backend === 1 ? 'wgpu' : 'soft';
  // 服务端步进/重放造成的暂停(帧 flags)要反映到 Pause 按钮。
  if (pausedFrame !== paused) {
    paused = pausedFrame;
    ui.pause.textContent = paused ? 'Resume' : 'Pause';
    ui.pause.classList.toggle('active', paused);
  }

  if (presenter) {
    try {
      lastPresent = presenter.present(rgba, width, height);
    } catch (error) {
      showError(`present 失败: ${error?.message ?? error}`);
    }
  } else {
    presentWithJs2d(rgba, width, height);
  }
  updateOverlay(pausedFrame);
  logFrame();
  hideLoading();
}

function presentWithJs2d(rgba, width, height) {
  if (!js2d) js2d = canvas.getContext('2d');
  if (!js2d) return;
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const image = new ImageData(new Uint8ClampedArray(rgba), width, height);
  js2d.putImageData(image, 0, 0);
  lastPresent = '2d-js';
}

function updateOverlay(force) {
  const now = performance.now();
  if (!force && now - lastOverlay < 200) return;
  lastOverlay = now;
  if (latest.frame !== undefined) {
    ui.frame.textContent = latest.frame;
    ui.ops.textContent = latest.ops;
    ui.skipped.textContent = latest.skipped;
    ui.executed.textContent = latest.executed;
    ui.ms.textContent = latest.ms;
    ui.viewport.textContent = `${latest.width}×${latest.height}`;
  }
  ui.present.textContent = lastPresent;
  if (status) {
    ui.engine.textContent = status.backend ?? '–';
    ui.entry.textContent = status.entry ?? '–';
    ui.marker.textContent = status.marker ?? '–';
    ui.statusLine.textContent = `${status.backend} · ${status.fps} fps · ${status.dataset?.kind ?? '?'} · built ${status.marker ?? '?'}`;
  }
}

function applyStatus(text) {
  try {
    status = JSON.parse(text);
  } catch {
    return;
  }
  log('dim', `status marker=${status.marker} backend=${status.backend} entry=${status.entry} fps=${status.fps} viewport=${(status.viewport || []).join('x')} dataset=${status.dataset?.kind ?? '?'}`);
  if (status.viewport) ui.viewport.textContent = `${status.viewport[0]}×${status.viewport[1]}`;
  updateOverlay(true);
}

// ---------------------------------------------------------------- websocket

let socket = null;
let reconnectDelay = 300;

function connect() {
  setConn('connecting…', 'connecting');
  log('dim', 'ws connecting…');
  socket = new WebSocket(`ws://${location.host}/ws`);
  socket.binaryType = 'arraybuffer';
  socket.onopen = () => {
    reconnectDelay = 300;
    setConn('connected', 'connected');
    log('ok', 'ws connected');
  };
  socket.onclose = () => {
    setConn(`reconnecting in ${Math.round(reconnectDelay)}ms`, 'closed');
    log('warn', `ws closed; retry in ${Math.round(reconnectDelay)}ms`);
    setTimeout(connect, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 2000);
  };
  socket.onerror = () => {
    log('err', 'ws error');
    socket?.close();
  };
  socket.onmessage = (event) => {
    if (typeof event.data === 'string') {
      applyStatus(event.data);
      return;
    }
    handleBinary(event.data);
  };
}

function send(message) {
  if (socket && socket.readyState === WebSocket.OPEN) {
    socket.send(JSON.stringify(message));
  }
}

function setConn(text, kind) {
  connPill.textContent = text;
  connPill.className = `pill ${kind}`;
}

// ---------------------------------------------------------------- input

function bindInput() {
  const press = (event) => {
    canvas.setPointerCapture?.(event.pointerId);
    sendCursor(event);
    send({ t: 'input', button: 'MOUSE_L', pressed: true });
    log('dim', 'input MOUSE_L down');
    event.preventDefault();
  };
  const release = (event) => {
    sendCursor(event);
    send({ t: 'input', button: 'MOUSE_L', pressed: false });
    log('dim', 'input MOUSE_L up');
    event.preventDefault();
  };
  canvas.addEventListener('pointerdown', press);
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener('pointermove', (event) => {
    // 引擎按"当前光标位置"响应悬停(TIP 等),移动即上报,不等按住。
    sendCursor(event);
  });
  canvas.addEventListener('contextmenu', (event) => event.preventDefault());

  const KEYMAP = {
    Enter: 'KEY_ENTER',
    ' ': 'KEY_SPACE',
    Escape: 'KEY_ESC',
    ArrowUp: 'KEY_UP',
    ArrowDown: 'KEY_DOWN',
    ArrowLeft: 'KEY_LEFT',
    ArrowRight: 'KEY_RIGHT',
    Shift: 'KEY_SHIFT',
    Control: 'KEY_CTRL',
    Tab: 'KEY_TAB',
  };
  window.addEventListener('keydown', (event) => {
    const button = KEYMAP[event.key];
    if (!button) return;
    send({ t: 'input', button, pressed: true });
    log('dim', `input ${button} down`);
    if (event.key !== 'Tab') event.preventDefault();
  });
  window.addEventListener('keyup', (event) => {
    const button = KEYMAP[event.key];
    if (!button) return;
    send({ t: 'input', button, pressed: false });
    log('dim', `input ${button} up`);
  });

  logEl.toggle?.addEventListener('click', () => setLogVisible(logEl.panel.hidden));
  logEl.clear?.addEventListener('click', () => {
    logEl.lines.replaceChildren();
    logTotal = 0;
    if (logEl.count) logEl.count.textContent = '0';
  });
  logEl.report?.addEventListener('click', () => logServerReport(true));

  ui.restart.addEventListener('click', () => {
    log('warn', 'control: restart');
    send({ t: 'restart' });
    // 立即黑屏 + 转圈;首帧到达(handleBinary)后撤掉。
    showLoading('restarting…');
    latest = { width: 0, height: 0 };
    ui.frame.textContent = '–';
    ui.ops.textContent = '–';
    ui.skipped.textContent = '–';
    ui.executed.textContent = '–';
    ui.ms.textContent = '–';
  });
  ui.frameFwd.addEventListener('click', () => {
    log('warn', 'control: step +1f');
    send({ t: 'step', n: 1 });
  });
  ui.frameBack.addEventListener('click', () => {
    const f = Number(latest.frame);
    if (!Number.isFinite(f) || f <= 1) {
      log('warn', 'control: back unavailable at frame 1');
      return;
    }
    // 引擎无快照,回退 = 确定性重放:重启后跑到目标帧再暂停。
    log('warn', `control: replay back to frame ${f - 1}`);
    showLoading(`replay → ${f - 1}…`);
    send({ t: 'back', target: f - 1 });
  });
  ui.pause.addEventListener('click', () => {
    paused = !paused;
    log('warn', paused ? 'control: pause' : 'control: resume');
    send({ t: paused ? 'pause' : 'resume' });
    ui.pause.textContent = paused ? 'Resume' : 'Pause';
    ui.pause.classList.toggle('active', paused);
  });
}

function sendCursor(event) {
  const rect = canvas.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return;
  const x = Math.round(((event.clientX - rect.left) / rect.width) * (canvas.width || 800));
  const y = Math.round(((event.clientY - rect.top) / rect.height) * (canvas.height || 600));
  send({ t: 'cursor', x, y });
}

function setHint(text) {
  ui.hint.textContent = text;
}

function showError(message) {
  log('err', message);
  ui.error.hidden = false;
  ui.error.textContent = message;
  console.error(message);
}
