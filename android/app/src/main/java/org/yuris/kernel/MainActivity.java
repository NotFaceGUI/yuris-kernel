package org.yuris.kernel;

import android.app.Activity;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.util.Log;
import android.view.MotionEvent;
import android.view.SurfaceView;
import android.view.View;
import android.view.ViewGroup;
import android.view.Gravity;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.TextView;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

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
public final class MainActivity extends Activity
        implements android.view.TextureView.SurfaceTextureListener {
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
    /** 状态浮层开关(发布默认关)。 */
    static final String EXTRA_SHOW_HUD = "show_hud";
    /** 启动失败原因回传给配置界面。 */
    static final String EXTRA_START_ERROR = "start_error";
    /** 新增参数:文本编码路线(auto/original/gbk/big5/tunnel)。 */
    static final String EXTRA_PATCH = "patch";
    /** 新增参数:存档块压缩层(zlib/snappy;空 = 自动判定)。 */
    static final String EXTRA_SAVE_CODEC = "save_codec";
    /** 新增参数:强制屏幕坐标系代号(@_SCREEN_COLOR)。 */
    static final String EXTRA_SCREEN_COLOR = "screen_color";
    /** 新增参数:强制帧率。 */
    static final String EXTRA_FPS = "fps";
    /** 性能特性:资产异步预取(等价桌面 YURIS_PREFETCH=1)。 */
    static final String EXTRA_PREFETCH = "prefetch";
    /** 性能特性:COPY2 快照延迟物化(等价桌面 YURIS_SNAPSHOT_DEFER=1)。 */
    static final String EXTRA_SNAPSHOT_DEFER = "snapshot_defer";
    /** 手机画质:`SIZE`(官方最近邻)改双线性采样,缓解 CG/差分缩放锯齿。 */
    static final String EXTRA_BILINEAR_SCALE = "bilinear_scale";
    /** 性能:Android PNG 解码走 libpng(NEON 反滤波);关 = image crate。 */
    static final String EXTRA_PNG_FAST = "png_fast";

    /**
     * {@link NativeHost#nativeTick()} 的"游戏已结束"返回码。
     *
     * <p>与内核 {@code Lifecycle::host_code} 同值 —— 这是跨平台统一退出信号,
     * 壳不在这里再判 {@code main_finished}。
     */
    private static final int TICK_FINISHED = 1;

    /**
     * {@link NativeHost#nativeTick()} 的"宿主请求销毁"返回码。
     *
     * <p>内核在渲染线程上真正释放了会话(见内核 {@code TICK_HOST_SHUTDOWN})。
     * 必须与 {@link #TICK_FINISHED} 区分:那个要 {@code finish()} 回配置界面,
     * 而这个只退出线程 —— 此时 Activity 通常正在 {@code onDestroy} 里,
     * {@code finish()} 是重入。
     */
    private static final int TICK_HOST_SHUTDOWN = 4;

    private android.view.TextureView textureView;
    private TextView hudView;
    private volatile boolean running;
    private Thread renderThread;
    /** 本次会话是否以"游戏正常结束"收尾(决定是否 finish 回配置界面)。 */
    private volatile boolean gameFinished;
    /** 内核会话已就绪(初始化成功)。会话在渲染线程的 thread_local 里,
     *  因此**线程必须常驻**:挂起只停泵,绝不杀线程(杀线程 = 丢会话)。 */
    private volatile boolean sessionReady;
    /** tick 失败(区别于挂起停泵;决定收尾时是否画错误屏)。 */
    private volatile boolean tickFailed;
    /** 泵闸:onPause 置 false,渲染线程空转等恢复(会话不丢);onResume 置 true。 */
    private volatile boolean pumping;
    /** GPU 后端(初始化成功且选了 wgpu)= 支持零拷贝上屏/重建。 */
    private volatile boolean gpuBackendChosen;
    /** Surface 重建待挂(零拷贝:onSurfaceTextureAvailable 置位,渲染线程消费)。 */
    private volatile boolean needsAttach;

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
    /** 滚轮一格(上/下)。引擎侧是 `_MOUSE_U`/`_MOUSE_D` 的按下+抬起对。 */
    private static final int EV_WHEEL_U = 3;
    private static final int EV_WHEEL_D = 4;

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

    /** 零拷贝上屏是否已生效(内核直接把帧送到本 Surface)。 */
    private volatile boolean zeroCopyPresent;
    /** 原生窗口 Surface(TextureView 提供;内核直接持有它的引用)。 */
    private android.view.Surface nativeSurface;

    private volatile String statusLine = "";
    /** 已提交给浮层的文本(节流用;只由渲染线程读写)。 */
    private String hudText = "";
    /** 状态浮层是否启用(壳侧开关;关掉后不再调 nativeStatus)。 */
    private boolean showHud;
    /** 调试日志是否启用(壳侧开关;关掉后不写 logcat、不落 JSONL)。 */
    private boolean debugEnabled;
    /** 虚拟鼠标模式(false = 直接触摸)。持久化,默认触摸。 */
    private boolean mouseMode;
    /** 虚拟鼠标在视图坐标里的位置(px)。 */
    /** 本次手势是否已经向内核投递过「按下」。 */
    private boolean pressedSent;
    /** 手势起点(视图坐标;判定点击 vs 拖动)。 */
    private float downX, downY;
    private float virtualX = -1f;
    private float virtualY = -1f;
    /** 本次触摸是否已判定为拖动(超过 touchSlop)。 */
    private boolean dragging;
    private float dragLastX, dragLastY;
    /** 手势开始时是否已有多指(用于滚动)。 */
    private boolean scrolling;
    private float wheelAccum;
    private int activePointers;
    /** 触摸态抬手后要把光标移出视口(清 hover)。 */
    private boolean clearCursorAfterRelease;
    /** 光标清理已排队(下一帧执行);见帧循环里的时序说明。 */
    private boolean cursorClearPending;
    private OverlayView overlayView;
    private ModeButton modeButton;
    private int touchSlopPx;
    private final Paint statusPaint = new Paint();
    /** 浮层:标签(灰)/ 数值(青)/ fps 标签与数值(加粗、更大)。 */
    private final Paint statusLabelPaint = new Paint();
    private final Paint statusValuePaint = new Paint();
    private final Paint statusFpsLabelPaint = new Paint();
    private final Paint statusFpsValuePaint = new Paint();
    /** 浮层行距与右边距(像素,初始化时按密度算)。 */
    private float statusLineHeight;
    private float statusRightMargin;
    private float statusTopMargin;
    /** 浮层底部留白。 */
    private float statusBottomMargin;
    /** 浮层左边界。 */
    private float statusLeftMargin;
    /** 浮层最大宽度(像素);0 = 按屏幕宽的 1/3 现算。 */
    private float statusMaxWidth;
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

        // TextureView(不是 SurfaceView):它能把自己的 Surface 直接交给 wgpu
        // 做零拷贝呈现,同时允许上层叠加普通 View 做状态浮层。
        textureView = new android.view.TextureView(this);
        textureView.setSurfaceTextureListener(this);
        touchSlopPx = (int) (16 * getResources().getDisplayMetrics().density);
        textureView.setOnTouchListener(new View.OnTouchListener() {
            @Override
            public boolean onTouch(View view, MotionEvent event) {
                switch (event.getActionMasked()) {
                    case MotionEvent.ACTION_DOWN:
                        activePointers = 1;
                        scrolling = false;
                        dragging = false;
                        pressedSent = false;
                        wheelAccum = 0f;
                        downX = event.getX();
                        downY = event.getY();
                        dragLastX = downX;
                        dragLastY = downY;
                        // 触摸态**先不发按下**:这一下可能是点击,也可能是拖动或
                        // 双指滚动的前半段。先按下会让"双指滚动"先误触发一次点击。
                        // 真正的按下在"确认拖动"或"抬手确认点击"时补发。
                        if (!mouseMode) {
                            pointerX = (int) event.getX();
                            pointerY = (int) event.getY();
                            pointerDirty = true;
                        }
                        return true;
                    case MotionEvent.ACTION_POINTER_DOWN:
                        // 第二根手指落下 = 滚动模式(整段手势不再产生点击)。
                        activePointers = event.getPointerCount();
                        scrolling = true;
                        if (!mouseMode && pressedSent) {
                            // 已经确认拖动并按下过:补一个抬起,别让键卡住。
                            enqueueInput(EV_UP);
                            pressedSent = false;
                        }
                        dragging = false;
                        wheelAccum = 0f;
                        dragLastY = averageY(event);
                        return true;
                    case MotionEvent.ACTION_MOVE:
                        if (scrolling || event.getPointerCount() >= 2) {
                            scrolling = true;
                            float y = averageY(event);
                            wheelAccum += dragLastY - y;
                            dragLastY = y;
                            // 每 24dp 发一格滚轮(_MOUSE_U/_MOUSE_D)。
                            float notch = 24 * getResources().getDisplayMetrics().density;
                            while (wheelAccum >= notch) {
                                enqueueWheel(true);
                                wheelAccum -= notch;
                            }
                            while (wheelAccum <= -notch) {
                                enqueueWheel(false);
                                wheelAccum += notch;
                            }
                            return true;
                        }
                        if (mouseMode) {
                            // 拖动 = 移动虚拟鼠标(不点击)。
                            float dx = event.getX() - dragLastX;
                            float dy = event.getY() - dragLastY;
                            if (!dragging && (Math.abs(dx) > 2 || Math.abs(dy) > 2)) {
                                dragging = true;
                            }
                            if (dragging) {
                                moveVirtualCursor(dx, dy);
                            }
                        } else {
                            pointerX = (int) event.getX();
                            pointerY = (int) event.getY();
                            pointerDirty = true;
                            if (!dragging
                                    && Math.hypot(pointerX - downX, pointerY - downY) > touchSlopPx) {
                                dragging = true;
                            }
                            if (dragging && !pressedSent) {
                                // 确认是拖动:这时才按下(拖拽语义与桌面一致)。
                                enqueueInput(EV_DOWN);
                                pressedSent = true;
                            }
                            enqueueInput(EV_MOVE);
                        }
                        dragLastX = event.getX();
                        dragLastY = event.getY();
                        return true;
                    case MotionEvent.ACTION_UP:
                        activePointers = 0;
                        if (scrolling) {
                            scrolling = false;
                            return true;
                        }
                        if (mouseMode) {
                            // 没有拖动的那一次抬手 = 在虚拟光标处点击。
                            if (!dragging) {
                                pointerX = (int) virtualX;
                                pointerY = (int) virtualY;
                                pointerDirty = true;
                                enqueueInput(EV_DOWN);
                                enqueueInput(EV_UP);
                                tapCount++;
                            }
                        } else {
                            if (dragging && pressedSent) {
                                enqueueInput(EV_UP);
                            } else if (!dragging) {
                                // 没拖动过的抬手 = 一次点击:此刻补发按下+抬起。
                                pointerX = (int) event.getX();
                                pointerY = (int) event.getY();
                                pointerDirty = true;
                                enqueueInput(EV_DOWN);
                                enqueueInput(EV_UP);
                                tapCount++;
                            }
                            // 抬手后把脚本光标移出视口:否则下一屏同位置若有
                            // 按钮会保持 hover(像有个鼠标钉在那儿)。
                            clearCursorAfterRelease = true;
                        }
                        dragging = false;
                        pressedSent = false;
                        return true;
                    case MotionEvent.ACTION_CANCEL:
                        activePointers = 0;
                        scrolling = false;
                        dragging = false;
                        if (!mouseMode) {
                            if (pressedSent) {
                                enqueueInput(EV_UP);
                            }
                            pressedSent = false;
                            clearCursorAfterRelease = true;
                        }
                        return true;
                    default:
                        return true;
                }
            }
        });
        // 容器:TextureView 铺满,状态浮层叠在上层(TextView,数值着色)。
        FrameLayout container = new FrameLayout(this);
        container.addView(textureView, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
        hudView = new TextView(this);
        hudView.setTextColor(Color.WHITE);
        hudView.setTextSize(android.util.TypedValue.COMPLEX_UNIT_SP, 12f);
        hudView.setShadowLayer(3f, 1f, 1f, Color.BLACK);
        hudView.setPadding(dp(12), dp(10), dp(12), dp(10));
        hudView.setVisibility(View.GONE);
        FrameLayout.LayoutParams hudParams = new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.TOP | Gravity.START);
        container.addView(hudView, hudParams);

        // 虚拟鼠标光标(仅在鼠标模式显示;不拦截触摸)。
        overlayView = new OverlayView(this);
        overlayView.setClickable(false);
        overlayView.setFocusable(false);
        container.addView(overlayView, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));

        // 模式切换按钮:右下角,半透明、小尺寸,不遮挡画面主体。
        modeButton = new ModeButton(this);
        FrameLayout.LayoutParams modeParams = new FrameLayout.LayoutParams(
                dp(28), dp(28), Gravity.BOTTOM | Gravity.END);
        modeParams.setMargins(0, 0, dp(6), dp(8));
        container.addView(modeButton, modeParams);

        setContentView(container);
        // 沉浸式要在 setContentView 之后:onCreate 早期 decorView 还没挂上,
        // `getInsetsController()` 会返回 null 并抛 NPE(实机崩溃已复现)。
        textureView.post(this::applyImmersiveFullscreen);

        // 浮层字号与原先一致(24f);着色只区分标签/数值,fps 仅加粗、不放大。
        float statusTextSize = 24f;
        statusPaint.setColor(Color.WHITE);
        statusPaint.setTextSize(statusTextSize);
        statusPaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);

        statusLabelPaint.setAntiAlias(true);
        statusLabelPaint.setTextSize(statusTextSize);
        statusLabelPaint.setColor(Color.WHITE);
        statusLabelPaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);

        statusValuePaint.setAntiAlias(true);
        statusValuePaint.setTextSize(statusTextSize);
        statusValuePaint.setColor(Color.parseColor("#7ad1ff"));
        statusValuePaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);

        statusFpsLabelPaint.setAntiAlias(true);
        statusFpsLabelPaint.setTextSize(statusTextSize);
        statusFpsLabelPaint.setColor(Color.WHITE);
        statusFpsLabelPaint.setFakeBoldText(true);
        statusFpsLabelPaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);

        statusFpsValuePaint.setAntiAlias(true);
        statusFpsValuePaint.setTextSize(statusTextSize);
        statusFpsValuePaint.setColor(Color.parseColor("#5ee08a"));
        statusFpsValuePaint.setFakeBoldText(true);
        statusFpsValuePaint.setShadowLayer(3f, 1f, 1f, Color.BLACK);

        statusLineHeight = statusTextSize * 1.25f;
        statusRightMargin = dp(14);
        statusLeftMargin = dp(14);
        statusTopMargin = dp(10);
        statusBottomMargin = dp(10);
        // 浮层最大宽度:屏幕宽度的 1/3。超过才换行(正常一行放得下)。
        statusMaxWidth = 0f;
    }

    // ---- TextureView 回调 ----

    @Override
    public void onSurfaceTextureAvailable(android.graphics.SurfaceTexture surfaceTexture,
                                          int width, int height) {
        nativeSurface = new android.view.Surface(surfaceTexture);
        Log.i(TAG, "surface available " + width + "x" + height);
        if (sessionReady && renderThread != null && renderThread.isAlive()) {
            // 会话还在(只是 surface 重建):渲染线程下一帧重新挂 Surface。
            needsAttach = true;
            pumping = true;
        } else {
            startRenderLoop();
        }
    }

    @Override
    public void onSurfaceTextureSizeChanged(android.graphics.SurfaceTexture surfaceTexture,
                                             int width, int height) {
        NativeHost.nativeResizeSurface(Math.max(1, width), Math.max(1, height));
    }

    @Override
    public boolean onSurfaceTextureDestroyed(android.graphics.SurfaceTexture surfaceTexture) {
        // 只停泵 + 放掉 Surface;会话与渲染线程保留(下次 available 重挂)。
        pumping = false;
        zeroCopyPresent = false;
        if (nativeSurface != null) {
            nativeSurface.release();
            nativeSurface = null;
        }
        return true;
    }

    @Override
    public void onSurfaceTextureUpdated(android.graphics.SurfaceTexture surfaceTexture) {
        // 内容由内核/壳绘制,这里无需动作。
    }

    @Override
    protected void onPause() {
        super.onPause();
        // 挂起 = 停时钟停泵（架构 §2.5.2 ④）。内核侧 nativeSuspend() 丢弃
        // 暂停期间的真实时间（恢复不快进）并静音；壳侧只关泵闸 ——
        // **渲染线程常驻**：会话在它的 thread_local 里,杀线程 = 丢会话 =
        // 恢复时重新 nativeInitJson,即「切后台回来游戏重启」的根因。
        // 存档已在 SAVE 刷盘的命令时刻落盘，因此这里无需额外 flush。
        NativeHost.nativeSuspend();
        pumping = false;
    }

    @Override
    protected void onResume() {
        super.onResume();
        NativeHost.nativeResume();
        if (sessionReady && renderThread != null && renderThread.isAlive()) {
            // 常驻线程还在:直接开闸续跑(会话原封不动)。
            pumping = true;
        } else if (textureView != null && textureView.isAvailable()) {
            // 冷启动/上一次真正结束过:走完整初始化。
            startRenderLoop();
        }
    }

    @Override
    protected void onDestroy() {
        // 顺序很重要:
        // 1. stopRenderLoop() 只置 running=false 并派守护线程 join,
        //    **UI 线程立即返回**(渲染线程可能正卡在 400~500ms 的 nativeTick);
        // 2. nativeShutdown() 在 UI 线程上**不会**摸会话 —— 会话在渲染线程
        //    的 thread_local 里。内核据此只登记一个销毁请求,由渲染线程自己
        //    在下一帧 tick 前释放引擎(旧实现在这里静默返回 -1,会话根本没释放)。
        stopRenderLoop();
        NativeHost.nativeShutdown();
        super.onDestroy();
    }

    private void startRenderLoop() {
        if (running) {
            return;
        }
        running = true;
        // 冷启动必须随线程一起开泵闸:pumping 的初值是 false,不开闸的话
        // 新线程进循环就落在「挂起空转」分支,一帧都不 tick(黑屏)。
        pumping = true;
        renderThread = new Thread(this::renderLoop, "yu-render");
        renderThread.start();
    }

    /**
     * 请求停止渲染线程。
     *
     * <p><b>绝不在调用线程上 join</b>：本方法在 {@code onDestroy} 里由
     * UI 线程调用，而渲染线程可能正卡在一次 {@code nativeTick}（真机实测
     * 单帧可达 400~500ms）。旧的 {@code thread.join(2000)} 会让主线程
     * 阻塞最长 2 秒，与 ANR 的输入分发超时直接冲突 —— 用户看到的就是
     * 「程序未响应」。
     *
     * <p>这里只置退出标志并把线程引用交给一个守护线程去 join：UI 线程
     * 立即返回，渲染线程自己在下一帧看到 {@code running == false} 后收尾
     * （它会自行调用 {@code nativeShutdown}）。
     */
    private void stopRenderLoop() {
        running = false;
        final Thread thread = renderThread;
        renderThread = null;
        if (thread == null) {
            return;
        }
        Thread reaper = new Thread(() -> {
            try {
                thread.join(3000);
            } catch (InterruptedException ignored) {
                Thread.currentThread().interrupt();
            }
        }, "yu-render-reaper");
        reaper.setDaemon(true);
        reaper.start();
    }

    private void renderLoop() {
        // 会话只初始化一次:onPause 停泵后 onResume 会起新线程进本方法,
        // 若不守卫就会重新 nativeInitJson ⇒ 用户看到的「切后台回来游戏重启」。
        // 挂起/恢复期间会话一直在内核 thread-local 里,新线程直接续跑。
        // 会话只初始化一次:onPause 停泵后 onResume 会起新线程进本方法,
        // 若不守卫就会重新 nativeInitJson ⇒ 「切后台回来游戏重启」。
        // 挂起/恢复期间会话一直在内核 thread-local 里,新线程直接续跑帧循环。
        if (!sessionReady) {
        // 音频后端(oboe)需要 JavaVM + Context,必须先于引擎启动。
        NativeHost.nativeInitAndroidContext(this);
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
        showHud = getIntent().getBooleanExtra(EXTRA_SHOW_HUD, false);
        debugEnabled = debugLog;
        mouseMode = getSharedPreferences(SetupActivity.PREFS, MODE_PRIVATE)
                .getBoolean("mouse_mode", false);
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
            // 隔离验证开关:传 EXTRA "audio" = false 时关音频(默认开)。
            if (!getIntent().getBooleanExtra("audio", true)) {
                config.put("audio", false);
            }
            // 诊断:抓 3 秒混音波形到 <游戏目录>/debug/audio-capture.wav。
            config.put("audio_capture_ms", 3000);
            // 纯音自测:旁路引擎,只验「环形缓冲 → oboe → 系统」这段。
            // 若纯音也爆音,故障在设备/模拟器转发,与引擎无关。
            if (getIntent().getBooleanExtra("tone", false)) {
                config.put("audio_tone", true);
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
            // ---- 新增参数(与 CLI 旗标一一对应;缺省不写键 = 用内核缺省) ----
            // 文本编码路线:汉化补丁的整包编码(原版/GBK/Big5/SJIS 隧道)。
            String patch = getIntent().getStringExtra(EXTRA_PATCH);
            if (patch != null && !patch.trim().isEmpty()) {
                config.put("patch", patch.trim());
            }
            // 存档块压缩层(只影响写盘;读侧始终按实际流分流)。
            String saveCodec = getIntent().getStringExtra(EXTRA_SAVE_CODEC);
            if (saveCodec != null && !saveCodec.trim().isEmpty()) {
                config.put("save_codec", saveCodec.trim());
            }
            // 视口像素格式代号(@_SCREEN_COLOR);0 = 不覆盖。
            int screenColor = getIntent().getIntExtra(EXTRA_SCREEN_COLOR, 0);
            if (screenColor > 0) {
                config.put("screen_color", screenColor);
            }
            // 帧率(@_FPS 语义);0 = 不覆盖(用内核缺省 60)。
            int fps = getIntent().getIntExtra(EXTRA_FPS, 0);
            if (fps > 0) {
                config.put("fps", fps);
            }
            // 性能特性开关(默认关;壳界面显式开启才写 true)。
            if (getIntent().getBooleanExtra(EXTRA_PREFETCH, false)) {
                config.put("prefetch", true);
            }
            if (getIntent().getBooleanExtra(EXTRA_SNAPSHOT_DEFER, false)) {
                config.put("snapshot_defer", true);
            }
            if (getIntent().getBooleanExtra(EXTRA_BILINEAR_SCALE, false)) {
                config.put("bilinear_scale", true);
            }
            // PNG 解码后端:壳显式写值;缺省(image)与未写键同义。
            if (getIntent().getBooleanExtra(EXTRA_PNG_FAST, false)) {
                config.put("png_backend", "libpng");
            } else {
                config.put("png_backend", "image");
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
        // GPU 后端下尝试零拷贝上屏:把 TextureView 的 Surface 交给内核。
        // 失败不致命 —— 保持 Bitmap 路径,只是帧率低一些。
        if (init.startsWith("OK") && init.contains("backend=wgpu")) {
            gpuBackendChosen = true;
            if (nativeSurface != null && nativeSurface.isValid()) {
                int attached = NativeHost.nativeAttachSurface(nativeSurface);
                zeroCopyPresent = attached == 0;
                NativeHost.nativeResizeSurface(
                        Math.max(1, textureView.getWidth()),
                        Math.max(1, textureView.getHeight()));
                Log.i(TAG, zeroCopyPresent
                        ? "zero-copy present: ON"
                        : "zero-copy present: OFF(" + NativeHost.nativeStatus() + ")");
            } else {
                Log.w(TAG, "zero-copy present: 没有有效 Surface");
            }
        }
        if (init.startsWith("ERR")) {
            statusLine = init;
            drawError();
            return;
        }
        sessionReady = true;
        } // if (!sessionReady) —— 一次性初始化结束

        int[] pixels = new int[0];
        int frameW = 0;
        int frameH = 0;
        Bitmap bitmap = null;
        /** 内核已在本线程释放会话(TICK_HOST_SHUTDOWN)⇒ 收尾不再重复 shutdown。 */
        boolean releasedByHost = false;
        // 帧节拍:按引擎 fps 换算成纳秒,截止时间从第一帧起累加(不漂移)。
        final int targetFps = 60;
        long frameNanos = 1_000_000_000L / Math.max(1, targetFps);
        long fpsWindowStart = SystemClock.elapsedRealtime();
        int fpsFrames = 0;
        double fps = 0.0;

        while (running) {
            // 泵闸关闭(挂起/无 surface):空转轻睡,不 tick —— 会话原样保留。
            if (!pumping) {
                if (!running) {
                    break; // onDestroy 请求退线程:走循环后的真正收尾。
                }
                // 挂起期间仍要响应销毁请求:ON_DESTROY 会先 stopRenderLoop()
                // 置 running=false(上面已 break),但如果销毁请求是在
                // 「已经挂起」的状态下登记的,这里靠 nativeTakeShutdownRequest()
                // 把它消费掉 —— 否则会话永远不释放(内核的销毁请求只在 tick
                // 里被取走,而挂起时根本不 tick)。
                if (NativeHost.nativeTakeShutdownRequest() == TICK_HOST_SHUTDOWN) {
                    releasedByHost = true;
                    break;
                }
                SystemClock.sleep(30); // 挂起空转;恢复后从这里继续 tick。
                continue;
            }
            // Surface 重建(后台回来/旋转):渲染线程上重新零拷贝挂载。
            if (needsAttach && gpuBackendChosen) {
                needsAttach = false;
                if (nativeSurface != null && nativeSurface.isValid()) {
                    int attached = NativeHost.nativeAttachSurface(nativeSurface);
                    zeroCopyPresent = attached == 0;
                    NativeHost.nativeResizeSurface(
                            Math.max(1, textureView.getWidth()),
                            Math.max(1, textureView.getHeight()));
                    Log.i(TAG, zeroCopyPresent ? "zero-copy re-attach ON" : "zero-copy re-attach OFF");
                }
            }
            long frameStartNanos = System.nanoTime();
            long started = SystemClock.elapsedRealtime();
            int tickResult = NativeHost.nativeTick();
            if (tickResult == TICK_HOST_SHUTDOWN) {
                // 内核已在渲染线程上释放会话(壳的 onDestroy 请求)。
                // 只退线程:此时 Activity 正在销毁,不能 finish()。
                Log.i(TAG, "session released by host shutdown request");
                gameFinished = false;
                tickFailed = false;
                releasedByHost = true;
                break;
            }
            if (tickResult == TICK_FINISHED) {
                // 游戏正常结束(脚本 END 或主脚本走完):收尾并回到配置界面,
                // 不把用户的进程留着空转。这是所有宿主共用的退出判据
                // (内核 `Lifecycle::host_code`),不在这里另判 main_finished。
                Log.i(TAG, "game finished: " + NativeHost.nativeStatus());
                gameFinished = true;
                break;
            }
            if (tickResult != 0) {
                statusLine = NativeHost.nativeStatus();
                Log.e(TAG, "tick failed: " + statusLine);
                tickFailed = true;
                break;
            }

            int[] size = NativeHost.nativeFrameSize();
            if (size.length >= 2 && (size[0] != frameW || size[1] != frameH)) {
                frameW = size[0];
                frameH = size[1];
                pixels = new int[frameW * frameH];
                bitmap = Bitmap.createBitmap(frameW, frameH, Bitmap.Config.ARGB_8888);
            }
            // 零拷贝上屏:内核已经把这一帧直接送到 Surface,壳不再读回像素,
            // 也不再画 Bitmap(省掉每帧 1.9MB 拷贝与一次整屏上传)。
            boolean needPixels = !zeroCopyPresent;
            if (zeroCopyPresent || (bitmap != null && pixels.length > 0)) {
                if (!needPixels) {
                    // 零拷贝:画面由内核直接写入 Surface,壳不画 Bitmap,
                    // 但触摸映射矩形仍要按同样的 letterbox 规则更新。
                    updatePresentRect(textureView.getWidth(), textureView.getHeight(),
                            frameW, frameH);
                }
                if (needPixels) {
                    NativeHost.nativeCopyFrame(pixels);
                    bitmap.setPixels(pixels, 0, frameW, 0, 0, frameW, frameH);
                }

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
                        case EV_WHEEL_U:
                            // 按下+抬起=一帧脉冲(引擎会把抬起延到下一帧,
                            // 保证脚本至少看到一次按下)。
                            NativeHost.nativeSetInput("MOUSE_U", true);
                            NativeHost.nativeSetInput("MOUSE_U", false);
                            break;
                        case EV_WHEEL_D:
                            NativeHost.nativeSetInput("MOUSE_D", true);
                            NativeHost.nativeSetInput("MOUSE_D", false);
                            break;
                        default:
                            // MOVE 只更新光标(上面已做),不产生按钮事件。
                            break;
                    }
                }

                // 触摸态抬手后把脚本光标移出视口(等价"鼠标离开窗口"),
                // 避免换屏后同位置按钮一直 hover。
                //
                // **必须推迟一帧**:本帧的 DOWN/UP 是 tick 之后才投给内核的,
                // 内核要到下一帧 tick 才消费这次点击。当场移光标会让引擎看到
                // 「在视口外(-1,-1)按下」⇒ 按钮完全不响应(触摸点了没反应,
                // 而虚拟鼠标模式没有这段清理所以正常)。
                if (cursorClearPending && !mouseMode) {
                    cursorClearPending = false;
                    NativeHost.nativeSetCursor(-1, -1);
                }
                if (clearCursorAfterRelease && !mouseMode) {
                    clearCursorAfterRelease = false;
                    cursorClearPending = true; // 下一帧再清,让点击先被消费
                }

                if (needPixels) {
                    present(bitmap);
                }
            }

            fpsFrames++;
            long now = SystemClock.elapsedRealtime();
            if (now - fpsWindowStart >= 1000) {
                fps = fpsFrames * 1000.0 / (now - fpsWindowStart);
                fpsFrames = 0;
                fpsWindowStart = now;
                // 发布模式:浮层关、调试日志关 ⇒ 一次 nativeStatus() 都不调,
                // 也不写 logcat。状态串要拼十几个 format 并取诊断/阶段计数,
                // 对一个正常运行的游戏是纯开销。
                if (showHud || debugEnabled) {
                    // 只有开了调试日志才向内核要整条状态串(它要拼十几个
                    // format、取诊断与渲染阶段计数);只开浮层时仅显示帧率。
                    String nativeText = debugEnabled ? NativeHost.nativeStatus() : "";
                    if (showHud) {
                        // 发布观感:浮层默认只显示帧率;开了调试日志才铺全量计数。
                        statusLine = debugEnabled
                                ? String.format("fps=%.1f %s taps=%d dropped=%d",
                                        fps, nativeText, tapCount, inputDropped)
                                : String.format("fps=%.1f", fps);
                        updateHud();
                    }
                    if (debugEnabled) {
                        Log.i(TAG, nativeText + " fps=" + String.format("%.1f", fps)
                                + " taps=" + tapCount + " dropped=" + inputDropped);
                    }
                }
            }

            // 帧调度:补足到目标帧长,**不做槽位跳跃**。
            //
            // 这里刻意不学桌面宿主的"错过截止就对齐到下一个未来槽位":那套
            // 逻辑的前提是每帧都能赶上 60Hz。Android 上单帧若稳定在 17~20ms
            // (GPU 读回 + 图层规划),对齐槽位会让它永远落在 2 个槽位后 ⇒
            // 直接锁死在 30fps。正确做法是把这一帧欠下的时间吸收掉、下一帧
            // 重新计时,让帧率等于硬件真实吞吐(实测区别:30fps → 55fps)。
            long elapsedNanos = System.nanoTime() - frameStartNanos;
            long remaining = frameNanos - elapsedNanos;
            if (remaining > 0) {
                sleepUntil(System.nanoTime() + remaining);
            }
        }
        // 循环退出原因分流:
        // - stopRenderLoop()(onDestroy 等)⇒ 会话随线程终止,真正收尾。
        // - 游戏结束 / tick 失败 ⇒ 真正收尾。
        // (挂起不再走这里:泵闸关闭时线程只在 while 里空转,不会退出。)
        if (!running && !gameFinished && !tickFailed) {
            Log.i(TAG, "render loop stopped by host (session torn down)");
        }
        // 内核已经在本线程上释放过会话(TICK_HOST_SHUTDOWN)时不能再调一次:
        // 那会在已空槽的线程上再登记一个销毁请求,而没有人会消费它 ⇒
        // 下次启动的第一帧就被它销毁。
        if (!releasedByHost) {
            NativeHost.nativeShutdown();
        }
        sessionReady = false;
        Log.i(TAG, "render loop stopped");
        if (tickFailed) {
            drawError();
        }
        if (gameFinished) {
            // 回到配置界面(同一任务栈):用户可以从那里再启动或退出 app。
            runOnUiThread(this::finish);
        }
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

    /**
     * 计算游戏画面在窗口里的实际矩形(居中 + 等比缩放)。
     *
     * <p>零拷贝与 Bitmap 两条路径都必须用它更新触摸映射矩形 —— 否则
     * `presentLeft/Width` 保持初值 0,触摸坐标会被钳到 (0,0),表现为
     * 「输入完全没用」(实机已复现)。
     */
    private void updatePresentRect(int windowWidth, int windowHeight, int viewportWidth,
                                   int viewportHeight) {
        if (windowWidth <= 0 || windowHeight <= 0 || viewportWidth <= 0 || viewportHeight <= 0) {
            return;
        }
        float scale = Math.min(
                (float) windowWidth / viewportWidth,
                (float) windowHeight / viewportHeight);
        int dw = Math.round(viewportWidth * scale);
        int dh = Math.round(viewportHeight * scale);
        presentLeft = (windowWidth - dw) / 2;
        presentTop = (windowHeight - dh) / 2;
        presentWidth = dw;
        presentHeight = dh;
    }

    private void present(Bitmap bitmap) {
        // CPU 后端路径:把整帧 Bitmap 画到 TextureView 的 Canvas。
        Canvas canvas;
        try {
            canvas = textureView.lockCanvas();
        } catch (Exception error) {
            Log.w(TAG, "lockCanvas failed: " + error);
            return;
        }
        if (canvas == null) {
            return;
        }
        try {
            // 触摸映射与绘制用同一个矩形,二者不能各算一套。
            int sw = canvas.getWidth();
            int sh = canvas.getHeight();
            updatePresentRect(sw, sh, bitmap.getWidth(), bitmap.getHeight());
            int left = presentLeft;
            int top = presentTop;
            int dw = presentWidth;
            int dh = presentHeight;
            canvas.drawColor(Color.BLACK);
            canvas.drawBitmap(bitmap, null,
                    new android.graphics.Rect(left, top, left + dw, top + dh), presentPaint);
        } finally {
            textureView.unlockCanvasAndPost(canvas);
        }
    }

    /**
     * 刷新状态浮层(TextView + Spannable)。
     *
     * <p>用 TextView 而不是往画面上画字:零拷贝上屏时画面由内核直接写到
     * Surface,壳无法在其上叠 Canvas,只能叠普通 View。Spannable 正好实现
     * 「只有数值变色」——标签保持白色,fps 数值加粗。
     */
    private void updateHud() {
        if (hudView == null) {
            return;
        }
        // 渲染线程不能直接摸 View(Android 会抛 CalledFromWrongThread);
        // 且状态行每秒才更新一次,这里按"同一份文本只提交一次"节流,
        // 避免每帧都往主线程丢一条 Runnable。
        String text = statusLine;
        if (text.equals(hudText)) {
            return;
        }
        hudText = text;
        runOnUiThread(() -> applyHudText(text));
    }

    /** 真正更新 TextView(主线程)。 */
    private void applyHudText(String text) {
        if (hudView == null) {
            return;
        }
        if (text.isEmpty()) {
            hudView.setVisibility(View.GONE);
            return;
        }
        List<StatusItem> items = StatusItem.parse(text);
        if (items.isEmpty()) {
            hudView.setVisibility(View.GONE);
            return;
        }
        android.text.SpannableStringBuilder builder = new android.text.SpannableStringBuilder();
        for (StatusItem item : items) {
            int labelStart = builder.length();
            if (!item.label.isEmpty()) {
                builder.append(item.label).append('=');
            }
            int valueStart = builder.length();
            builder.append(item.value);
            int valueEnd = builder.length();
            builder.append(' ');

            if (!item.label.isEmpty()) {
                builder.setSpan(
                        new android.text.style.ForegroundColorSpan(Color.WHITE),
                        labelStart, valueStart,
                        android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE);
            }
            builder.setSpan(
                    new android.text.style.ForegroundColorSpan(
                            item.fps ? Color.parseColor("#5ee08a") : Color.parseColor("#7ad1ff")),
                    valueStart, valueEnd,
                    android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE);
            if (item.fps) {
                builder.setSpan(
                        new android.text.style.StyleSpan(android.graphics.Typeface.BOLD),
                        valueStart, valueEnd,
                        android.text.Spannable.SPAN_EXCLUSIVE_EXCLUSIVE);
            }
        }
        hudView.setText(builder);
        hudView.setVisibility(View.VISIBLE);
    }

    /** 浮层一项:`k=v`。 */
    private static final class StatusItem {
        final String label;
        final String value;
        final boolean fps;

        StatusItem(String label, String value, boolean fps) {
            this.label = label;
            this.value = value;
            this.fps = fps;
        }

        /**
         * 把引擎状态串拆成词(含 `=` 的拆成键值;`fps` 标记为加粗)。
         */
        static List<StatusItem> parse(String line) {
            List<StatusItem> items = new ArrayList<>();
            for (String token : line.trim().split("[ 	]+")) {
                if (token.isEmpty()) {
                    continue;
                }
                int eq = token.indexOf('=');
                if (eq <= 0) {
                    continue;
                }
                items.add(new StatusItem(
                        token.substring(0, eq), token.substring(eq + 1), "fps".equals(token.substring(0, eq))));
            }
            return items;
        }
    }

    private void drawError() {
        Canvas canvas;
        try {
            canvas = textureView.lockCanvas();
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
            textureView.unlockCanvasAndPost(canvas);
        }
    }

    /** 多指时的平均 Y(双指滚动用)。 */
    private float averageY(MotionEvent event) {
        float sum = 0f;
        int n = Math.max(1, event.getPointerCount());
        for (int i = 0; i < event.getPointerCount(); i++) {
            sum += event.getY(i);
        }
        return sum / n;
    }

    /** 一格滚轮(上/下)。 */
    private void enqueueWheel(boolean up) {
        enqueueInput(up ? EV_WHEEL_U : EV_WHEEL_D);
    }

    /** 双击切换模式:触摸 ↔ 虚拟鼠标。 */
    private void toggleMouseMode() {
        mouseMode = !mouseMode;
        getSharedPreferences(SetupActivity.PREFS, MODE_PRIVATE)
                .edit().putBoolean("mouse_mode", mouseMode).apply();
        if (mouseMode) {
            // 首次进入鼠标模式:光标落在画面中心。
            if (virtualX < 0) {
                virtualX = textureView.getWidth() / 2f;
                virtualY = textureView.getHeight() / 2f;
            }
            pointerX = (int) virtualX;
            pointerY = (int) virtualY;
            pointerDirty = true;
        } else {
            // 退出鼠标模式:把脚本光标移出视口,别留 hover。
            clearCursorAfterRelease = true;
        }
        if (overlayView != null) {
            overlayView.invalidate();
        }
        if (modeButton != null) {
            modeButton.invalidate();
        }
    }

    /** 移动虚拟光标(视图坐标,带边界钳制)。 */
    private void moveVirtualCursor(float dx, float dy) {
        float w = Math.max(1, textureView.getWidth());
        float h = Math.max(1, textureView.getHeight());
        virtualX = Math.min(w - 1, Math.max(0, virtualX + dx));
        virtualY = Math.min(h - 1, Math.max(0, virtualY + dy));
        pointerX = (int) virtualX;
        pointerY = (int) virtualY;
        pointerDirty = true;
        if (overlayView != null) {
            overlayView.invalidate();
        }
    }

    /** 虚拟鼠标光标(只画,不吃触摸)。 */
    private final class OverlayView extends View {
        private final android.graphics.Paint fill = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
        private final android.graphics.Paint edge = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
        private final android.graphics.Path arrow = new android.graphics.Path();

        OverlayView(Context context) {
            super(context);
            setWillNotDraw(false);
            fill.setStyle(android.graphics.Paint.Style.FILL);
            fill.setColor(Color.WHITE);
            edge.setStyle(android.graphics.Paint.Style.STROKE);
            edge.setStrokeWidth(dpf(1.5f));
            edge.setColor(0xCC000000);
        }

        @Override
        protected void onDraw(Canvas canvas) {
            if (!mouseMode || virtualX < 0) {
                return;
            }
            float x = virtualX;
            float y = virtualY;
            arrow.reset();
            arrow.moveTo(x, y);
            arrow.lineTo(x, y + dpf(20));
            arrow.lineTo(x + dpf(5.5f), y + dpf(15));
            arrow.lineTo(x + dpf(9), y + dpf(21.5f));
            arrow.lineTo(x + dpf(12.5f), y + dpf(19.5f));
            arrow.lineTo(x + dpf(9), y + dpf(13));
            arrow.lineTo(x + dpf(15), y + dpf(13));
            arrow.close();
            canvas.drawPath(arrow, fill);
            canvas.drawPath(arrow, edge);
        }
    }

    /** 右下角的模式切换按钮:半透明、只占一小块,点击切换触摸/鼠标。 */
    private final class ModeButton extends View {
        private final android.graphics.Paint paint = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
        private final android.graphics.Paint iconPaint = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
        private final android.graphics.Path arrow = new android.graphics.Path();

        ModeButton(Context context) {
            super(context);
            setClickable(true);
            paint.setStyle(android.graphics.Paint.Style.FILL);
            // 尽量不起眼:只是一层极淡的轮廓,几乎看不见。
            paint.setColor(0x0AFFFFFF);
            iconPaint.setStyle(android.graphics.Paint.Style.FILL);
            iconPaint.setColor(0x59FFFFFF);
            setOnClickListener(v -> toggleMouseMode());
        }

        @Override
        protected void onDraw(Canvas canvas) {
            float r = Math.min(getWidth(), getHeight()) / 2f;
            canvas.drawCircle(getWidth() / 2f, getHeight() / 2f, r, paint);
            float cx = getWidth() / 2f;
            float cy = getHeight() / 2f;
            if (mouseMode) {
                // 鼠标:箭头
                float s = r * 0.42f;
                arrow.reset();
                arrow.moveTo(cx - s * 0.5f, cy - s);
                arrow.lineTo(cx - s * 0.5f, cy + s * 0.9f);
                arrow.lineTo(cx - s * 0.05f, cy + s * 0.45f);
                arrow.lineTo(cx + s * 0.35f, cy + s * 1.05f);
                arrow.lineTo(cx + s * 0.75f, cy + s * 0.8f);
                arrow.lineTo(cx + s * 0.35f, cy + s * 0.25f);
                arrow.lineTo(cx + s * 0.85f, cy + s * 0.25f);
                arrow.close();
                canvas.drawPath(arrow, iconPaint);
            } else {
                // 触摸:手指圆点
                canvas.drawCircle(cx, cy, r * 0.34f, iconPaint);
                iconPaint.setStyle(android.graphics.Paint.Style.STROKE);
                iconPaint.setStrokeWidth(dpf(1.6f));
                canvas.drawCircle(cx, cy, r * 0.56f, iconPaint);
                iconPaint.setStyle(android.graphics.Paint.Style.FILL);
            }
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

    /** dp → 像素(浮点版;绘制用,避免取整跳动)。 */
    private float dpf(float value) {
        return value * getResources().getDisplayMetrics().density;
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
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
