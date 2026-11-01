package org.yuris.kernel;

import android.app.Activity;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.util.Log;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

import java.io.File;

/**
 * 轻量验证壳：把内核 so 跑起来，帧画到 SurfaceView，触摸转成引擎输入。
 *
 * <p>刻意不引入 AndroidX / 第三方库：目标是把「同一份 yu-engine 在 Android
 * 上跑同一份游戏数据」这条链路验证通，UI 只需要能看见帧、能点。
 *
 * <p><b>线程</b>：内核是单线程的，所有 {@link NativeHost} 调用都在
 * {@link #renderLoop} 这一个线程里，主线程只发输入标志。
 *
 * <p><b>数据布局</b>：游戏数据放在 app 外置私有目录
 * {@code Android/data/org.yuris.kernel/files/game/}（adb push 即可，无需权限）：
 * <pre>
 * files/game/data.ypf      ← 数据入口（或散盘 data/ 目录）
 * files/game/yscfg.dat     ← 视口/配置
 * files/game/yu-ris.exe    ← 可选：脚本密钥扫描来源
 * files/game/fonts/*.ttf   ← 可选：游戏点名的日文字体（宿主注入）
 * </pre>
 * 存档写 {@code files/save/}。
 */
public final class MainActivity extends Activity implements SurfaceHolder.Callback {
    static final String TAG = "yu-android";
    static final String ENTRY = "SYSTEM_START";
    /** 配置界面传入的目录/入口/字体(缺省则回落到 app 私有目录)。 */
    static final String EXTRA_GAME_DIR = "game_dir";
    static final String EXTRA_SAVE_DIR = "save_dir";
    static final String EXTRA_ENTRY = "entry";
    static final String EXTRA_FONT_PATH = "font_path";
    static final String EXTRA_FONT_FAMILY = "font_family";
    static final String EXTRA_BACKEND = "backend";
    static final String EXTRA_GPU_BACKEND = "gpu_backend";
    static final String EXTRA_DEBUG_LOG = "debug_log";
    /** 启动失败原因回传给配置界面。 */
    static final String EXTRA_START_ERROR = "start_error";

    private SurfaceView surfaceView;
    private volatile boolean running;
    private Thread renderThread;

    /**
     * 输入事件队列(主线程写、渲染线程取)。
     *
     * <p>不能只采样"当前是否按下":快速点击可能整个发生在两帧之间,采样会
     * 直接丢掉这次点击(表现为"点了没反应")。这里按顺序排队,渲染线程每帧
     * 全部取出并逐个投给内核。
     */
    private final java.util.concurrent.ConcurrentLinkedQueue<int[]> pendingInputs =
            new java.util.concurrent.ConcurrentLinkedQueue<>();
    /** 被丢弃的事件数(上限保护;正常不会触发)。 */
    private volatile int inputDropped;

    private static final int EV_DOWN = 1;
    private static final int EV_UP = 0;
    private static final int EV_MOVE = 2;

    private volatile int pointerX;
    private volatile int pointerY;
    private volatile boolean pointerDirty;
    private volatile int tapCount;

    /**
     * 画面在 Surface 上的实际绘制矩形(居中、等比缩放后的位置与尺寸)。
     *
     * <p>触摸坐标必须按**这个**矩形映射回游戏视口。之前直接用 SurfaceView
     * 全尺寸做比例换算,黑边区域也被算进去 ⇒ 越靠画面边缘偏移越大(实测
     * 点菜单项落到别的项上)。
     */
    private volatile int presentLeft;
    private volatile int presentTop;
    private volatile int presentWidth;
    private volatile int presentHeight;

    private volatile String statusLine = "";
    private final Paint statusPaint = new Paint();
    /**
     * 上屏缩放画笔。
     *
     * <p>必须带 {@code FILTER_BITMAP_FLAG}:`drawBitmap` 传 null paint 时是**最近邻**
     * 采样,800x600 非整数倍放大到屏幕会产生明显锯齿(实测读档界面文字与立绘边缘)。
     * 开双线性后同样分辨率下边缘平滑。
     */
    private final Paint presentPaint = new Paint(Paint.FILTER_BITMAP_FLAG);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        surfaceView = new SurfaceView(this);
        SurfaceHolder surfaceHolder = surfaceView.getHolder();
        // 显式 RGBA_8888:不指定时部分设备用 RGB_565,渐变/半透明会出现色带。
        surfaceHolder.setFormat(android.graphics.PixelFormat.RGBA_8888);
        surfaceHolder.addCallback(this);
        surfaceView.setOnTouchListener(new View.OnTouchListener() {
            @Override
            public boolean onTouch(View view, MotionEvent event) {
                pointerX = (int) event.getX();
                pointerY = (int) event.getY();
                pointerDirty = true;
                switch (event.getActionMasked()) {
                    case MotionEvent.ACTION_DOWN:
                        enqueueInput(EV_DOWN);
                        return true;
                    case MotionEvent.ACTION_MOVE:
                        enqueueInput(EV_MOVE);
                        return true;
                    case MotionEvent.ACTION_UP:
                        enqueueInput(EV_UP);
                        tapCount++;
                        return true;
                    case MotionEvent.ACTION_CANCEL:
                        enqueueInput(EV_UP);
                        return true;
                    default:
                        return true;
                }
            }
        });
        setContentView(surfaceView);
        // 沉浸式要在 setContentView 之后:onCreate 早期 decorView 还没挂上,
        // `getInsetsController()` 会返回 null 并抛 NPE(实机崩溃已复现)。
        surfaceView.post(this::applyImmersiveFullscreen);

        statusPaint.setColor(Color.WHITE);
        statusPaint.setTextSize(24f);
        statusPaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        startRenderLoop();
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        // 视口由游戏 yscfg 决定；surface 尺寸变化只影响上屏缩放。
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        stopRenderLoop();
    }

    @Override
    protected void onPause() {
        super.onPause();
        // 挂起 = 停时钟停泵（架构 §2.5.2 ④）；恢复时重建渲染循环。
        // 存档已在 SAVE 刷盘的命令时刻落盘，因此这里无需额外 flush。
        stopRenderLoop();
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (surfaceView != null && surfaceView.getHolder().getSurface() != null
                && surfaceView.getHolder().getSurface().isValid()) {
            startRenderLoop();
        }
    }

    @Override
    protected void onDestroy() {
        stopRenderLoop();
        NativeHost.nativeShutdown();
        super.onDestroy();
    }

    private void startRenderLoop() {
        if (running) {
            return;
        }
        running = true;
        renderThread = new Thread(this::renderLoop, "yu-render");
        renderThread.start();
    }

    private void stopRenderLoop() {
        running = false;
        Thread thread = renderThread;
        renderThread = null;
        if (thread != null) {
            try {
                thread.join(2000);
            } catch (InterruptedException ignored) {
                Thread.currentThread().interrupt();
            }
        }
    }

    private void renderLoop() {
        String version = NativeHost.nativeVersion();
        Log.i(TAG, "so: " + version);

        // 数据目录:配置界面选的优先;否则 app 私有 files/game(adb 旁载路径)。
        String configured = getIntent().getStringExtra(EXTRA_GAME_DIR);
        File gameDir = configured != null ? new File(configured) : new File(getFilesDir(), "game");
        String configuredSave = getIntent().getStringExtra(EXTRA_SAVE_DIR);
        File saveDir = configuredSave != null && !configuredSave.trim().isEmpty()
                ? new File(configuredSave)
                : new File(gameDir, "save");
        String entry = getIntent().getStringExtra(EXTRA_ENTRY);
        if (entry == null || entry.trim().isEmpty()) {
            entry = ENTRY;
        }
        Log.i(TAG, "dirs: game=" + gameDir.getAbsolutePath() + " save=" + saveDir.getAbsolutePath());

        String fontPath = getIntent().getStringExtra(EXTRA_FONT_PATH);
        if (fontPath == null) {
            fontPath = "";
        }
        String fontFamily = getIntent().getStringExtra(EXTRA_FONT_FAMILY);
        if (fontFamily == null) {
            fontFamily = "";
        }
        // 走 JSON 入口:与 yu-cli 的旗标一一对应,壳只负责拼键。
        // 新增能力(禁音频/存档编码/调试模式…)只要加键,不必改 JNI 签名。
        org.json.JSONObject config = new org.json.JSONObject();
        String backend = getIntent().getStringExtra(EXTRA_BACKEND);
        String backendHint = backend;
        boolean debugLog = getIntent().getBooleanExtra(EXTRA_DEBUG_LOG, false);
        try {
            config.put("data_root", gameDir.getAbsolutePath());
            config.put("save_root", saveDir.getAbsolutePath());
            config.put("entry", entry);
            if (backend != null && !backend.isEmpty()) {
                config.put("backend", backend);
            }
            String gpuBackend = getIntent().getStringExtra(EXTRA_GPU_BACKEND);
            if (gpuBackend != null && !gpuBackend.isEmpty()) {
                config.put("gpu_backend", gpuBackend);
            }
            if (debugLog) {
                // 日志落游戏目录,adb pull 即可取回(与 CLI --log-file 同格式)。
                config.put("log_file", new File(gameDir, "debug/yu-kernel.jsonl").getAbsolutePath());
                config.put("log_level", "verbose");
            }
            if (!fontFamily.isEmpty()) {
                config.put("font", fontFamily);
            }
            // 用户选的字体文件由内核按 data_root/fonts/ 之外的显式路径加载,
            // 这里用 font_bytes 的等价入口:先把路径告诉内核。
            if (!fontPath.isEmpty()) {
                config.put("font_file", fontPath);
            }
        } catch (org.json.JSONException error) {
            Log.e(TAG, "config build failed", error);
        }
        // GPU 模式在部分环境(尤其 Android 模拟器的 gfxstream Vulkan)会在
        // 首次提交时于驱动内部 SIGSEGV —— 那是进程级崩溃,Java 侧 try/catch
        // 接不住。所以启动前先探测:已知不支持的环境直接提示并留在配置界面。
        // 只有当"模拟器 + 明确要求 Vulkan"时才拦(驱动实测 SIGSEGV)。
        // GLES 与 auto(先 GLES)在模拟器上可用,不拦。
        String gpuBackendHint = getIntent().getStringExtra(EXTRA_GPU_BACKEND);
        if ("wgpu".equals(backendHint) && "vulkan".equals(gpuBackendHint)
                && isEmulator()) {
            Log.w(TAG, "模拟器 Vulkan 驱动会在 vkQueueSubmit 崩溃,已阻止");
            statusLine = "模拟器的 Vulkan 驱动有缺陷(会崩溃),请选 GLES 或自动";
            setResultAndReturn();
            return;
        }
        String init = NativeHost.nativeInitJson(config.toString());
        Log.i(TAG, "init: " + init);
        if (init.startsWith("ERR")) {
            statusLine = init;
            drawError();
            return;
        }

        int[] pixels = new int[0];
        int frameW = 0;
        int frameH = 0;
        Bitmap bitmap = null;
        // 帧节拍:按引擎 fps 换算成纳秒,截止时间从第一帧起累加(不漂移)。
        final int targetFps = 60;
        long frameNanos = 1_000_000_000L / Math.max(1, targetFps);
        long nextFrameNanos = System.nanoTime() + frameNanos;
        long fpsWindowStart = SystemClock.elapsedRealtime();
        int fpsFrames = 0;
        double fps = 0.0;

        while (running) {
            long started = SystemClock.elapsedRealtime();
            int tickResult = NativeHost.nativeTick();
            if (tickResult != 0) {
                statusLine = NativeHost.nativeStatus();
                Log.e(TAG, "tick failed: " + statusLine);
                drawError();
                break;
            }

            int[] size = NativeHost.nativeFrameSize();
            if (size.length >= 2 && (size[0] != frameW || size[1] != frameH)) {
                frameW = size[0];
                frameH = size[1];
                pixels = new int[frameW * frameH];
                bitmap = Bitmap.createBitmap(frameW, frameH, Bitmap.Config.ARGB_8888);
            }
            if (bitmap != null && pixels.length > 0) {
                NativeHost.nativeCopyFrame(pixels);
                bitmap.setPixels(pixels, 0, frameW, 0, 0, frameW, frameH);

                // 输入:把触摸点从 Surface 坐标映射回游戏视口坐标(经过缩放与
                // 黑边偏移),再按队列顺序投递按下/抬起。
                //
                // 顺序很关键:光标位置先更新,再投按钮事件 —— 否则内核记录的是
                // 上一个位置的按下。
                int pLeft = presentLeft;
                int pTop = presentTop;
                int pWidth = Math.max(1, presentWidth);
                int pHeight = Math.max(1, presentHeight);
                boolean moved = pointerDirty;
                if (moved) {
                    pointerDirty = false;
                }
                boolean anyInput = moved || !pendingInputs.isEmpty();
                if (anyInput) {
                    int relX = pointerX - pLeft;
                    int relY = pointerY - pTop;
                    int vx = clamp(relX * frameW / pWidth, 0, frameW - 1);
                    int vy = clamp(relY * frameH / pHeight, 0, frameH - 1);
                    NativeHost.nativeSetCursor(vx, vy);
                }
                int[] input;
                while ((input = pendingInputs.poll()) != null) {
                    switch (input[0]) {
                        case EV_DOWN:
                            NativeHost.nativeSetInput("MOUSE_L", true);
                            break;
                        case EV_UP:
                            NativeHost.nativeSetInput("MOUSE_L", false);
                            break;
                        default:
                            // MOVE 只更新光标(上面已做),不产生按钮事件。
                            break;
                    }
                }

                present(bitmap);
            }

            fpsFrames++;
            long now = SystemClock.elapsedRealtime();
            if (now - fpsWindowStart >= 1000) {
                fps = fpsFrames * 1000.0 / (now - fpsWindowStart);
                fpsFrames = 0;
                fpsWindowStart = now;
                statusLine = String.format("%s fps=%.1f taps=%d dropped=%d",
                        NativeHost.nativeStatus(), fps, tapCount, inputDropped);
                Log.i(TAG, statusLine);
            }

            // 帧调度:固定截止时间(deadline),不是"干完活再睡满一帧"。
            //
            // 旧写法 `sleep(16 - elapsed)` 有两个毛病:①`SystemClock.sleep` 只有
            // 毫秒粒度且实际会晚 ~1ms,16ms 目标跑成 17.3ms ⇒ 只有 57fps;
            // ②16ms 本身是 62.5Hz,不是 60Hz。这里改用纳秒截止时间 + 亚毫秒
            // 睡眠,与桌面宿主 `schedule_next_frame` 同口径:错过截止就对齐到
            // 下一个未来槽位(保留相位),不补跑。
            nextFrameNanos += frameNanos;
            long nowNanos = System.nanoTime();
            if (nextFrameNanos <= nowNanos) {
                long behind = nowNanos - nextFrameNanos;
                long missed = behind / frameNanos + 1;
                nextFrameNanos += missed * frameNanos;
            }
            sleepUntil(nextFrameNanos);
        }

        NativeHost.nativeShutdown();
        Log.i(TAG, "render loop stopped");
    }

    /**
     * 全屏沉浸:隐藏状态栏与导航栏,并允许内容延伸到系统栏区域。
     *
     * <p>不做这一步时,导航栏会占掉底部一条,画面按可用区域居中后底部留黑边
     * (实机截图确认)。游戏是 4:3,宽屏上左右仍会有黑边,这是正确行为;
     * 但底部那一条属于系统栏,必须让出去。
     */
    private void applyImmersiveFullscreen() {
        Window window = getWindow();
        window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
            WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                controller.hide(WindowInsets.Type.systemBars());
                controller.setSystemBarsBehavior(
                        WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            } else if (window.getDecorView().isAttachedToWindow()) {
                // 已挂载但拿不到控制器的设备:退回旧 API。
                applyLegacyFullscreen(window);
            }
        } else {
            applyLegacyFullscreen(window);
        }
    }

    /** 旧 API(< R)的全屏标志;也作为新 API 取不到控制器时的退路。 */
    private void applyLegacyFullscreen(Window window) {
        window.getDecorView().setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // 用户滑出系统栏后需要重新隐藏(immersive 的常规做法)。
        if (hasFocus && getWindow().getDecorView().isAttachedToWindow()) {
            applyImmersiveFullscreen();
        }
    }

    private void present(Bitmap bitmap) {
        SurfaceHolder holder = surfaceView.getHolder();
        Canvas canvas;
        try {
            canvas = holder.lockCanvas();
        } catch (Exception error) {
            Log.w(TAG, "lockCanvas failed: " + error);
            return;
        }
        if (canvas == null) {
            return;
        }
        try {
            // 整数倍缩放 + 居中：保持像素锐利（软后端输出已是最终画面）。
            int sw = canvas.getWidth();
            int sh = canvas.getHeight();
            float scale = Math.min((float) sw / bitmap.getWidth(), (float) sh / bitmap.getHeight());
            int dw = Math.round(bitmap.getWidth() * scale);
            int dh = Math.round(bitmap.getHeight() * scale);
            int left = (sw - dw) / 2;
            int top = (sh - dh) / 2;
            // 触摸映射与绘制用同一个矩形,二者不能各算一套。
            presentLeft = left;
            presentTop = top;
            presentWidth = dw;
            presentHeight = dh;
            canvas.drawColor(Color.BLACK);
            canvas.drawBitmap(bitmap, null,
                    new android.graphics.Rect(left, top, left + dw, top + dh), presentPaint);
            if (!statusLine.isEmpty()) {
                canvas.drawText(statusLine, 16f, 32f, statusPaint);
            }
        } finally {
            holder.unlockCanvasAndPost(canvas);
        }
    }

    private void drawError() {
        SurfaceHolder holder = surfaceView.getHolder();
        Canvas canvas;
        try {
            canvas = holder.lockCanvas();
        } catch (Exception error) {
            return;
        }
        if (canvas == null) {
            return;
        }
        try {
            canvas.drawColor(Color.BLACK);
            statusPaint.setTextSize(20f);
            float y = 60f;
            for (String line : wrap(statusLine, 46)) {
                canvas.drawText(line, 24f, y, statusPaint);
                y += 28f;
            }
        } finally {
            holder.unlockCanvasAndPost(canvas);
        }
    }

    /** 入队一次输入;队列积压超过上限时丢最旧(防止异常情况下无限增长)。 */
    private void enqueueInput(int kind) {
        if (pendingInputs.size() > 512) {
            pendingInputs.poll();
            inputDropped++;
        }
        pendingInputs.add(new int[]{kind});
    }

    /**
     * GPU 可用性的保守探测。
     *
     * <p>Android 模拟器的 gfxstream Vulkan 实现在 `vkQueueSubmit` 里对空指针
     * 解引用(实测 `vulkan.ranchu.so` SIGSEGV),而 wgpu 的首帧提交必然触发。
     * 这是**进程级崩溃**,Java 无法捕获,所以只能在启动前判定:模拟器环境
     * 一律建议 CPU,真机才放行 GPU。
     */
    private boolean isEmulator() {
        String fingerprint = Build.FINGERPRINT == null ? "" : Build.FINGERPRINT;
        String hardware = Build.HARDWARE == null ? "" : Build.HARDWARE;
        String product = Build.PRODUCT == null ? "" : Build.PRODUCT;
        return fingerprint.contains("generic")
                || fingerprint.contains("emulator")
                || hardware.contains("ranchu")
                || hardware.contains("goldfish")
                || product.contains("sdk")
                || product.contains("emulator");
    }

    /** 返回配置界面并带上原因(壳据此提示)。 */
    private void setResultAndReturn() {
        android.content.Intent result = new android.content.Intent();
        result.putExtra(EXTRA_START_ERROR, statusLine);
        setResult(RESULT_CANCELED, result);
        runOnUiThread(this::finish);
    }

    /**
     * 睡到 `deadlineNanos`。用 `Thread.sleep(ms, ns)` 而不是
     * `SystemClock.sleep(ms)`:后者只有毫秒粒度,60Hz(16.67ms)会被抬到
     * 17~18ms,直接丢掉 3~5 帧/秒。
     */
    private static void sleepUntil(long deadlineNanos) {
        while (true) {
            long remaining = deadlineNanos - System.nanoTime();
            if (remaining <= 0) {
                return;
            }
            long millis = remaining / 1_000_000L;
            int nanos = (int) (remaining % 1_000_000L);
            try {
                if (millis > 0 || nanos > 0) {
                    Thread.sleep(millis, nanos);
                }
            } catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                return;
            }
        }
    }

    private static int clamp(int value, int low, int high) {
        return value < low ? low : Math.min(value, high);
    }

    private static java.util.List<String> wrap(String text, int width) {
        java.util.List<String> lines = new java.util.ArrayList<>();
        String rest = text;
        while (rest.length() > width) {
            lines.add(rest.substring(0, width));
            rest = rest.substring(width);
        }
        if (!rest.isEmpty()) {
            lines.add(rest);
        }
        return lines;
    }
}
