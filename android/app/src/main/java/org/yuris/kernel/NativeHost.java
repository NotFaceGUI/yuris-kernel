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

    /** 本机可用字体族(换行分隔;空串 = 没有可枚举字体)。 */
    public static native String nativeListFonts();

    /** 推进一帧。返回 0 成功 / -1 未初始化 / -2 tick 失败。 */
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

    /** 构建目标标记，用于核对 so 与壳是否匹配。 */
    public static native String nativeVersion();

    /** 释放会话。返回 0 表示确实释放了一个会话。 */
    public static native int nativeShutdown();
}
