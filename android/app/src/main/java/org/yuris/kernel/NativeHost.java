package org.yuris.kernel;

/**
 * 内核 so 的 JNI 绑定。
 *
 * <p>方法名与 {@code crates/yu-platform/host-android/src/lib.rs} 的导出符号
 * 一一对应（Java 方法名 = 符号名后缀）。所有方法必须在<b>同一线程</b>调用：
 * 内核侧会话状态放在 thread-local，`Engine` 内部是单线程的
 * {@code Rc/RefCell}。
 */
public final class NativeHost {
    static {
        System.loadLibrary("yu_host_android");
    }

    private NativeHost() {}

    /**
     * <b>推荐入口</b>:用一个 JSON 配置启动,与 {@code yu-cli} 的旗标一一对应。
     *
     * <pre>
     * {
     *   "data_root": "/sdcard/Download/yugame",   // 必填
     *   "save_root": "/sdcard/Download/yugame/save", // 省略 = data_root/save
     *   "entry": "SYSTEM_START",
     *   "debug_mode": false,
     *   "script_key": "0x11223344",
     *   "font": "Noto Sans CJK JP",
     *   "os_sound": true,      // @_OS_SOUND 注入值
     *   "audio": true,          // 是否真的出声
     *   "fps": 60,
     *   "screen_color": 32,
     *   "dx_mode": 0,
     *   "save_codec": "zlib|snappy",
     *   "media_readonly": false,
     *   "record_events": false
     * }
     * </pre>
     *
     * 新增能力只加 JSON 键,签名不变 —— 各模拟器壳按需拼键即可。
     * 返回 {@code OK ...} 或 {@code ERR: ...}。
     */
    public static native String nativeInitJson(String json);

    /**
     * 旧入口(等价于只设 data/save/entry/font 的 JSON)。
     *
     * @param fontPath 用户选的字体文件路径;空串 = 只用系统字体与目录内置字体。
     */
    public static native String nativeInit(
            String dataRoot, String saveRoot, String entry, String fontPath, String fontFamily);

    /**
     * 初始化 NDK 上下文(JavaVM + Context)。
     *
     * <p><b>必须在任何音频调用之前调用一次</b>。oboe(cpal 的 Android 音频
     * 后端)通过它取 JavaVM/Context 来建 AudioTrack;不初始化会在打开音频
     * 设备时 panic(`android context was not initialized`)。
     *
     * @param activity 传 Activity 实例(其 Context 可拿到 AudioManager)。
     */
    public static native int nativeInitAndroidContext(android.app.Activity activity);

    /** 本机可用字体族(换行分隔;空串 = 没有可枚举字体)。 */
    public static native String nativeListFonts();

    /**
     * 绑定 {@link android.view.Surface} 做<b>零拷贝上屏</b>:内核把渲染结果
     * 直接送到这个窗口,不再每帧把整帧读回 CPU。
     *
     * <p>必须在 {@code nativeInitJson} 之后、且 GPU 后端下调用。返回 0 成功;
     * 非 0 时壳继续用 {@link #nativeCopyFrame} 的 Bitmap 路径(不致命)。
     */
    public static native int nativeAttachSurface(android.view.Surface surface);

    /** 窗口尺寸变化时调用(重建交换链)。 */
    public static native int nativeResizeSurface(int width, int height);

    /**
     * 推进一帧。
     *
     * @return {@code 0} 成功继续；{@code 1} 游戏已结束（脚本 END / 主脚本走完
     *     —— 壳应 {@code finish()} 回到自己的界面）；{@code -1} 未初始化；
     *     {@code -2} tick 失败。
     */
    public static native int nativeTick();

    /** 当前帧尺寸 {@code [width, height]}；未出帧时为 {@code [0, 0]}。 */
    public static native int[] nativeFrameSize();

    /**
     * 把当前帧写进 {@code out}（长度必须 ≥ w*h），写入 ARGB_8888 打包像素。
     * 返回写入的像素数，负值 = 失败。
     */
    public static native int nativeCopyFrame(int[] out);

    /** 注入输入按钮（{@code MOUSE_L} / {@code KEY_ENTER} …）。 */
    public static native int nativeSetInput(String button, boolean pressed);

    /** 注入光标位置（引擎视口像素坐标）。 */
    public static native int nativeSetCursor(int x, int y);

    /** 诊断摘要（浮层 / logcat 用）。 */
    public static native String nativeStatus();

    /**
     * 挂起会话：停时钟（恢复后不快进）、静音音频输出。
     *
     * <p>在 {@code onPause} 调用。**不做存档 flush** —— 存档在脚本 {@code SAVE}
     * 命令时刻就已落盘。返回 0 成功 / -1 未初始化；重复调用幂等。
     */
    public static native int nativeSuspend();

    /** 恢复会话（{@code onResume}）。与 {@link #nativeSuspend} 配对。 */
    public static native int nativeResume();

    /**
     * 会话生命周期代号。
     *
     * <ul>
     *   <li>{@code 0} running（可继续 tick）</li>
     *   <li>{@code 1} finished（主脚本走完）</li>
     *   <li>{@code 2} exited（脚本 END）</li>
     *   <li>{@code 3} exited 且带 RESTART 标志</li>
     *   <li>{@code -1} 未初始化</li>
     * </ul>
     *
     * <p>与 {@link #nativeTick()} 的返回值同一判据：tick 返回 1 ⇔ 这里非 0/1。
     */
    public static native int nativeLifecycle();

    /** 构建目标标记，用于核对 so 与壳是否匹配。 */
    public static native String nativeVersion();

    /** 释放会话。返回 0 表示确实释放了一个会话。 */
    public static native int nativeShutdown();

    /**
     * 消费一次"跨线程销毁请求",在调用线程上真正释放会话。
     *
     * <p>挂起/无 surface 时空转路径不调 {@link #nativeTick()},而销毁请求
     * 正常只在那里被取走 —— 那个状态下退出应用会话就永远不释放。壳在
     * 空转循环里轮询本方法:返回 {@code 4} 表示已释放(退出渲染循环),
     * {@code 0} 表示没有请求。
     *
     * <p>必须在**持有会话的线程**(渲染线程)上调用。
     */
    public static native int nativeTakeShutdownRequest();
}
