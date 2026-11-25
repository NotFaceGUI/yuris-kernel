package org.yuris.kernel;

import android.Manifest;
import android.app.Activity;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.util.Log;
import android.widget.Toast;

import org.json.JSONObject;

import java.io.File;

/**
 * Tyranor-Next 第三方 APK 运行时模块的**唯一入口**。
 *
 * <p>只做三件事:校验 v1 协议参数、确保真实路径存储权限、把游戏文件夹直接
 * 交给 {@link MainActivity}。没有内容视图、没有配置页/游戏库,也不依赖
 * Compose/Miuix/AppCompat —— 这是模块体积能压到接近纯 so 的原因。
 *
 * <p>可选能力全部走 1.73 的静态声明(`res/raw/tyranor_runtime.json`):
 * <ul>
 *   <li>{@code settings}:主 App 在游戏抽屉里渲染,启动时以
 *       {@code com.tyranor.runtime.extra.SETTINGS} 下发完整生效值;</li>
 *   <li>{@code saves}:声明游戏内 {@code save/} 子目录,主 App 负责导入/导出/
 *       版本与 WebDAV 同步;</li>
 *   <li>{@code fileImports}:字体导入 {@code <游戏目录>/fonts/}(内核启动时自动
 *       装载),翻译文件导入 {@code <游戏目录>/translate.json}({@code translate}
 *       开关打开时自动加载)。</li>
 * </ul>
 *
 * <p>正常退出游戏后任务栈里没有本 Activity(启动后立即 {@code finish()}),
 * 因此直接回到 Tyranor,而不是回到一个空壳页面。
 */
public final class TyranorRuntimeActivity extends Activity {
    private static final String TAG = "yu-tyranor";

    private static final String ACTION_LAUNCH = "com.tyranor.runtime.action.LAUNCH";
    private static final String EXTRA_FOLDER_PATH = "com.tyranor.runtime.extra.FOLDER_PATH";
    private static final String EXTRA_GAME_TITLE = "com.tyranor.runtime.extra.GAME_TITLE";
    private static final String EXTRA_PROTOCOL_VERSION =
            "com.tyranor.runtime.extra.PROTOCOL_VERSION";
    private static final String EXTRA_SETTINGS = "com.tyranor.runtime.extra.SETTINGS";
    private static final int PROTOCOL_VERSION = 1;
    private static final int REQUEST_LEGACY_STORAGE = 1005;

    /** Tyranor 下发的游戏根(绝对路径)。 */
    private String gameDir;
    /** SETTINGS 的已校验值;缺失/损坏时用声明默认值。 */
    private RuntimeSettings settings = new RuntimeSettings();
    /** 已经把 Intent 交给游戏页;之后不再响应入口重复调用。 */
    private boolean dispatched;
    /** 已经发起过一次存储权限请求(避免 onCreate/onResume 循环弹)。 */
    private boolean storageAsked;
    /** 本次权限请求是跳系统「所有文件访问」设置页(API 30+)。 */
    private boolean awaitingSettingsPage;
    /** 确实离开过本页去设置页;没有这个标记时 onResume 不能判失败。 */
    private boolean leftForSettingsPage;
    /** 已经报错并收尾。 */
    private boolean finished;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (acceptIntent(getIntent())) {
            maybeLaunch();
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        // 正常流程下入口交棒后立即 finish(),不会再收到 onNewIntent;真收到
        // 说明本次已交棒,忽略即可(新启动会由系统创建新的入口实例)。
        if (dispatched) {
            return;
        }
        if (acceptIntent(intent)) {
            maybeLaunch();
        }
    }

    @Override
    protected void onPause() {
        super.onPause();
        if (awaitingSettingsPage && !dispatched) {
            leftForSettingsPage = true;
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (!leftForSettingsPage || dispatched || finished) {
            return;
        }
        leftForSettingsPage = false;
        if (hasStorageAccess()) {
            if (gameDir == null || !new File(gameDir).isDirectory()) {
                fail("Tyranor 指定的游戏目录不存在");
                return;
            }
            launchGame();
        } else {
            fail("需要「所有文件访问」权限才能读取游戏目录");
        }
    }

    @Override
    public void onRequestPermissionsResult(
            int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode != REQUEST_LEGACY_STORAGE || dispatched || finished) {
            return;
        }
        if (grantResults.length > 0
                && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
            launchGame();
        } else {
            fail("没有存储权限:内核无法按真实路径读写游戏目录");
        }
    }

    // ------------------------------------------------------------ 协议

    private boolean acceptIntent(Intent intent) {
        if (intent == null) {
            fail("没有收到 Tyranor 启动参数");
            return false;
        }
        String action = intent.getAction();
        if (action != null && !ACTION_LAUNCH.equals(action)) {
            fail("不支持的入口 action: " + action);
            return false;
        }
        int protocol = intent.getIntExtra(EXTRA_PROTOCOL_VERSION, PROTOCOL_VERSION);
        if (protocol != PROTOCOL_VERSION) {
            fail("不支持的 Tyranor 模块协议版本: " + protocol);
            return false;
        }
        String folder = intent.getStringExtra(EXTRA_FOLDER_PATH);
        if (folder == null || folder.trim().isEmpty()) {
            fail("Tyranor 未提供游戏目录");
            return false;
        }
        File dir = new File(folder);
        if (!dir.isAbsolute()) {
            fail("Tyranor 指定的游戏目录不是绝对路径: " + folder);
            return false;
        }
        // 先落目录/参数,再看存储权限:scoped storage 下未授权时
        // `isDirectory()` 可能为 false,旧顺序会误报“目录不存在”且永远
        // 不弹“所有文件访问”设置页。
        gameDir = dir.getAbsolutePath();
        settings = RuntimeSettings.parse(intent.getStringExtra(EXTRA_SETTINGS));
        String title = intent.getStringExtra(EXTRA_GAME_TITLE);
        if (!hasStorageAccess()) {
            requestStorage();
            return false;
        }
        if (!dir.isDirectory()) {
            fail("Tyranor 指定的游戏目录不存在: " + folder);
            return false;
        }
        Log.i(TAG, "launch"
                + (title == null || title.isEmpty() ? "" : " title=" + title)
                + " protocol=" + protocol);
        return true;
    }

    private void maybeLaunch() {
        if (dispatched || finished || gameDir == null) {
            return;
        }
        if (!hasStorageAccess()) {
            requestStorage();
            return;
        }
        launchGame();
    }

    // ------------------------------------------------------------ 权限

    /** 与 :app 的 StoragePaths.hasStorageAccess 同一判据。 */
    private boolean hasStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        return checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED;
    }

    private void requestStorage() {
        if (storageAsked || finished) {
            return;
        }
        storageAsked = true;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            awaitingSettingsPage = true;
            Intent request = new Intent(
                    Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                    Uri.parse("package:" + getPackageName()));
            try {
                startActivity(request);
            } catch (Exception first) {
                try {
                    startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
                } catch (Exception second) {
                    fail("无法打开存储权限设置页: " + second.getMessage());
                }
            }
        } else {
            requestPermissions(new String[] {
                    Manifest.permission.READ_EXTERNAL_STORAGE,
                    Manifest.permission.WRITE_EXTERNAL_STORAGE,
            }, REQUEST_LEGACY_STORAGE);
        }
    }

    // ------------------------------------------------------------ 交棒

    private void launchGame() {
        if (dispatched || finished) {
            return;
        }
        dispatched = true;

        Intent intent = new Intent(this, MainActivity.class);
        intent.putExtra(MainActivity.EXTRA_GAME_DIR, gameDir);
        // 与声明里的 saves.directory 一致;主 App 的存档管理只认这个子目录。
        intent.putExtra(MainActivity.EXTRA_SAVE_DIR, new File(gameDir, "save").getAbsolutePath());
        intent.putExtra(MainActivity.EXTRA_BACKEND, "wgpu");
        intent.putExtra(MainActivity.EXTRA_GPU_BACKEND, settings.gpuBackend);
        intent.putExtra(MainActivity.EXTRA_MOVIE_BACKEND, settings.movieBackend);
        intent.putExtra(MainActivity.EXTRA_MOUSE_MODE, settings.mouseMode);
        intent.putExtra(MainActivity.EXTRA_FPS, settings.fps);
        intent.putExtra(MainActivity.EXTRA_PATCH, settings.patch);
        intent.putExtra(MainActivity.EXTRA_BILINEAR_SCALE, settings.bilinearScale);
        intent.putExtra(MainActivity.EXTRA_FONT_FALLBACK, settings.fontFallback);
        intent.putExtra(MainActivity.EXTRA_DEBUG_LOG, settings.debugLog);
        intent.putExtra(MainActivity.EXTRA_SHOW_HUD, settings.showHud);
        intent.putExtra(MainActivity.EXTRA_VERSION_BADGE, settings.versionBadge);
        intent.putExtra(MainActivity.EXTRA_PREFETCH, settings.prefetch);
        intent.putExtra(MainActivity.EXTRA_SNAPSHOT_DEFER, settings.snapshotDefer);
        intent.putExtra(MainActivity.EXTRA_VM_PROFILE, settings.vmProfile);
        intent.putExtra(MainActivity.EXTRA_ASSET_TIMING, settings.assetTiming);
        intent.putExtra(MainActivity.EXTRA_TICK_SPLIT, settings.tickSplit);
        intent.putExtra(MainActivity.EXTRA_CG_TIMING, settings.cgTiming);
        // 存档压缩:"auto" = 内核按实际流自动判定(extra 留空)。
        if (!settings.saveCodec.isEmpty() && !"auto".equals(settings.saveCodec)) {
            intent.putExtra(MainActivity.EXTRA_SAVE_CODEC, settings.saveCodec);
        }
        // 运行时翻译:推荐由「导入翻译文件」把 translate.json 放进游戏根,
        // 这里只在开关打开且文件存在时加载;外部路径不进模块配置。
        if (settings.translate) {
            File translate = new File(gameDir, "translate.json");
            if (translate.isFile()) {
                intent.putExtra(MainActivity.EXTRA_TRANSLATE, translate.getAbsolutePath());
            }
        }
        if (!settings.font.isEmpty()) {
            if (looksLikeFontFile(settings.font)) {
                File file = new File(settings.font);
                if (file.isFile()) {
                    intent.putExtra(MainActivity.EXTRA_FONT_PATH, file.getAbsolutePath());
                    intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, file.getName());
                } else {
                    Log.w(TAG, "字体文件不存在,忽略: " + settings.font);
                }
            } else {
                intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, settings.font);
            }
        }
        // 新启动清掉旧任务:换游戏时不把上一个游戏留在返回栈里。
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TASK);
        startActivity(intent);
        finish();
    }

    private void fail(String message) {
        if (finished) {
            return;
        }
        finished = true;
        Log.w(TAG, message);
        Toast.makeText(this, message, Toast.LENGTH_LONG).show();
        finish();
    }

    private static boolean looksLikeFontFile(String value) {
        String lower = value.toLowerCase();
        return lower.endsWith(".ttf") || lower.endsWith(".otf")
                || lower.endsWith(".ttc") || lower.endsWith(".otc")
                || value.indexOf('/') >= 0 || value.indexOf('\\') >= 0;
    }

    // ------------------------------------------------------------ SETTINGS

    /**
     * 1.73 下发的完整生效配置(`{"version":1,"values":{…}}`)。
     *
     * <p>只按白名单/类型读取;缺项或非法值回落到声明默认值,与主 App 的
     * settings 解析约定一致。绝不把外部 JSON 当命令/路径/反射输入。
     */
    private static final class RuntimeSettings {
        int fps = 60;
        boolean mouseMode;
        boolean bilinearScale = true;
        boolean fontFallback;
        boolean debugLog;
        boolean showHud;
        boolean versionBadge;
        boolean translate = true;
        boolean prefetch;
        boolean snapshotDefer;
        boolean vmProfile;
        boolean assetTiming;
        boolean tickSplit;
        boolean cgTiming;
        String patch = "auto";
        String gpuBackend = "auto";
        String movieBackend = "auto";
        String saveCodec = "auto";
        String font = "";

        static RuntimeSettings parse(String raw) {
            RuntimeSettings value = new RuntimeSettings();
            if (raw == null || raw.isEmpty()) {
                return value;
            }
            try {
                JSONObject root = new JSONObject(raw);
                if (root.optInt("version", -1) != 1) {
                    Log.w(TAG, "忽略不支持的 SETTINGS 版本");
                    return value;
                }
                JSONObject values = root.optJSONObject("values");
                if (values == null) {
                    return value;
                }
                String fps = stringValue(values, "fps");
                if ("30".equals(fps)) {
                    value.fps = 30;
                } else if ("120".equals(fps)) {
                    value.fps = 120;
                }
                value.mouseMode = booleanValue(values, "mouse_mode", value.mouseMode);
                value.bilinearScale =
                        booleanValue(values, "bilinear_scale", value.bilinearScale);
                value.fontFallback =
                        booleanValue(values, "font_fallback", value.fontFallback);
                value.debugLog = booleanValue(values, "debug_log", value.debugLog);
                value.showHud = booleanValue(values, "show_hud", value.showHud);
                value.versionBadge =
                        booleanValue(values, "version_badge", value.versionBadge);
                value.translate = booleanValue(values, "translate", value.translate);
                value.prefetch = booleanValue(values, "prefetch", value.prefetch);
                value.snapshotDefer =
                        booleanValue(values, "snapshot_defer", value.snapshotDefer);
                value.vmProfile = booleanValue(values, "vm_profile", value.vmProfile);
                value.assetTiming =
                        booleanValue(values, "asset_timing", value.assetTiming);
                value.tickSplit = booleanValue(values, "tick_split", value.tickSplit);
                value.cgTiming = booleanValue(values, "cg_timing", value.cgTiming);
                value.patch = oneOf(
                        stringValue(values, "patch"), value.patch,
                        "auto", "original", "gbk", "big5", "tunnel");
                value.gpuBackend = oneOf(
                        stringValue(values, "gpu_backend"), value.gpuBackend,
                        "auto", "gles", "vulkan");
                value.movieBackend = oneOf(
                        stringValue(values, "movie_backend"), value.movieBackend,
                        "auto", "builtin", "external");
                value.saveCodec = oneOf(
                        stringValue(values, "save_codec"), value.saveCodec,
                        "auto", "zlib", "snappy");
                String font = stringValue(values, "font");
                if (font != null) {
                    value.font = font.trim();
                }
            } catch (Exception error) {
                Log.w(TAG, "SETTINGS 无法解析,按声明默认值启动", error);
            }
            return value;
        }

        private static String stringValue(JSONObject values, String key) {
            Object raw = values.opt(key);
            return raw instanceof String ? (String) raw : null;
        }

        private static boolean booleanValue(JSONObject values, String key, boolean fallback) {
            Object raw = values.opt(key);
            return raw instanceof Boolean ? (Boolean) raw : fallback;
        }

        private static String oneOf(String value, String fallback, String... allowed) {
            if (value != null) {
                for (String candidate : allowed) {
                    if (candidate.equals(value)) {
                        return value;
                    }
                }
            }
            return fallback;
        }
    }
}
