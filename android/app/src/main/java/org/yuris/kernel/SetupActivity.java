package org.yuris.kernel;

import android.Manifest;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.Settings;
import android.util.Log;
import android.view.View;
import android.view.ViewGroup;
import android.view.animation.DecelerateInterpolator;
import android.widget.ArrayAdapter;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Spinner;
import android.widget.TextView;

import androidx.appcompat.app.AppCompatActivity;

import org.json.JSONException;
import org.json.JSONObject;

import java.io.File;
import java.util.ArrayList;
import java.util.List;

/**
 * 启动配置:选游戏目录 → (可选)选字体 → 开始。
 *
 * <p><b>选哪个目录就用哪个目录</b>:不复制、不改路径。内核是原生 Rust,按真实
 * 文件路径读写,所以这里需要「所有文件访问」权限才能直接读
 * {@code /sdcard/Download/yugame} 这类用户目录。存档默认写
 * {@code <游戏目录>/save/},可另指定。
 *
 * <p>界面只显示用户自己选的东西;不展示任何 app 内部路径。
 */
public final class SetupActivity extends AppCompatActivity {
    private static final String TAG = "yu-android";
    /** 首选项文件名(壳内共享:选目录页与游戏页都读写它)。 */
    static final String PREFS = "yu_kernel";
    private static final int REQUEST_GAME_DIR = 1001;
    private static final int REQUEST_FONT = 1002;
    private static final int REQUEST_PLAY = 1003;
    private static final int REQUEST_SAVE_DIR = 1004;
    private static final int REQUEST_STORAGE = 1005;

    /**
     * Tyranor-Next 第三方 APK 运行时模块协议。
     *
     * <p>宿主按 applicationId 前缀 {@code com.tyranor.runtime.*} 发现模块,
     * 再用显式 ComponentName 派发 {@code LAUNCH};只传游戏根目录绝对路径,
     * 不传 SAF URI,也不代授存储权限。协议版本当前为 1。
     */
    private static final String TYRANOR_ACTION_LAUNCH = "com.tyranor.runtime.action.LAUNCH";
    private static final String TYRANOR_EXTRA_FOLDER_PATH =
            "com.tyranor.runtime.extra.FOLDER_PATH";
    private static final String TYRANOR_EXTRA_GAME_TITLE =
            "com.tyranor.runtime.extra.GAME_TITLE";
    private static final String TYRANOR_EXTRA_PROTOCOL_VERSION =
            "com.tyranor.runtime.extra.PROTOCOL_VERSION";
    private static final String TYRANOR_EXTRA_SETTINGS =
            "com.tyranor.runtime.extra.SETTINGS";
    private static final int TYRANOR_PROTOCOL_VERSION = 1;

    /** Android 10 及以下的存储权限回调;授权后接着跑用户刚才点的动作。 */
    private Runnable pendingStorageAction;

    /** Tyranor 派发后待自动启动;权限或字体任一项没就绪都不抢跑。 */
    private boolean pendingExternalLaunch;
    /** 字体枚举是后台线程;外部启动要等它恢复上次选中项后再进游戏。 */
    private Runnable pendingAfterFonts;
    /** 本次启动是否由 Tyranor 派发(正常退出后把用户还给宿主)。 */
    private boolean externalSession;

    private static final int BG = Color.parseColor("#0e1014");
    private static final int CARD = Color.parseColor("#1a1e26");
    private static final int ACCENT = Color.parseColor("#4a90d9");
    private static final int TEXT = Color.parseColor("#e8ecf2");
    private static final int MUTED = Color.parseColor("#8b93a3");

    private TextView gameLabel;
    private TextView saveLabel;
    private TextView status;
    private Spinner fontSpinner;
    private Button startButton;

    /** 游戏根目录(真实路径,内核直接读)。 */
    private String gameDir;
    /** 自定义存档目录;null = 游戏目录/save。 */
    private String saveDir;
    private String fontPath;
    /** 启动时的输入方式:true = 虚拟光标,false = 直接触摸(游戏内可临时切换)。 */
    private boolean mouseMode;
    /** 操作方式按钮组(重置/高亮刷新用)。 */
    private Button[] inputModeButtons;
    private static final String[] INPUT_MODE_VALUES = {"touch", "mouse"};
    /** 帧率上限(壳侧节拍;同时作为内核 fps 参数)。 */
    private int fps = 60;
    private Button[] fpsButtons;
    private static final int[] FPS_VALUES = {30, 60, 120};
    private static final String[] FPS_LABELS = {"30 fps", "60 fps", "120 fps"};
    /** 渲染后端:"soft"(CPU)/ "wgpu"(GPU)。 */
    private String backend = "soft";
    /** GPU 底层接口:"auto" / "gles" / "vulkan"。 */
    private String gpuBackend = "auto";
    /** 文本编码路线:auto / original / gbk / big5 / tunnel(与 CLI `--patch` 同口径)。 */
    private String patch = "auto";
    /** 存档块压缩层:"" = 自动判定 / "zlib" / "snappy"。 */
    private String saveCodec = "";
    /** 全屏影片解码后端:"auto" / "builtin" / "external"(与内核 JSON 同词表)。 */
    private String movieBackend = "auto";
    /** 设备视频解码能力摘要(懒探测,选中外置/自动时显示)。 */
    private TextView movieProbeLabel;
    private String movieProbeText;
    /** 写调试日志到游戏目录(debug/*.jsonl)。 */
    private boolean debugLog;
    /** 状态浮层(HUD)。发布默认关:浮层每秒还要拼状态串。 */
    private boolean showHud;
    /** 左下角版本角标(默认开):显示引擎版本名,支持绿/不支持红。 */
    private boolean versionBadge = true;
    /** 资产异步预取(默认关;等价桌面 --prefetch)。 */
    private boolean prefetch;
    /** COPY2 快照延迟物化(默认关;等价 --snapshot-defer)。 */
    private boolean snapshotDefer;
    /** `SIZE` 双线性采样(手机画质;默认开,官方是最近邻)。 */
    private boolean bilinearScale = true;
    /** 逐字形缺字回退(默认关;等价桌面 --font-fallback)。 */
    private boolean fontFallback;
    /** 高级诊断:VM opcode 逐条耗时归因(等价 --vm-profile)。 */
    private boolean vmProfile;
    /** 高级诊断:资产装载/预取计时(等价 --asset-timing)。 */
    private boolean assetTiming;
    /** 高级诊断:tick 内部分段计时(等价 --tick-split)。 */
    private boolean tickSplit;
    /** 高级诊断:CG 差分计时(等价 --cg-timing)。 */
    private boolean cgTiming;
    private List<String> systemFonts = new ArrayList<>();
    /** 上次选中的字体(持久化;下拉加载完成后恢复选中)。 */
    private String savedFontFamily = "";

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        gameDir = prefs.getString("game_dir", null);
        saveDir = prefs.getString("save_dir", null);
        fontPath = prefs.getString("font_path", null);
        savedFontFamily = prefs.getString("font_family", "");
        // 从已持久化的字体路径重建导入项的名字:下拉最后一项靠它存在,
        // 否则重启后这一项消失、恢复选中必然失败(用户看到的就是"没记住")。
        if (fontPath != null && !fontPath.isEmpty()) {
            importedFontName = new File(fontPath).getName();
        }
        backend = prefs.getString("backend", "soft");
        // CPU 模式已停用:壳只提供 GPU 路径,CPU 按钮仅占位。
        backend = "wgpu";
        mouseMode = prefs.getBoolean("launch_mouse_mode", false);
        fps = prefs.getInt("fps", 60);
        if (fps != 30 && fps != 60 && fps != 120) {
            fps = 60;
        }
        gpuBackend = prefs.getString("gpu_backend", "auto");
        patch = prefs.getString("patch", "auto");
        saveCodec = prefs.getString("save_codec", "");
        movieBackend = prefs.getString("movie_backend", "auto");
        if (!"auto".equals(movieBackend)
                && !"builtin".equals(movieBackend)
                && !"external".equals(movieBackend)) {
            movieBackend = "auto";
        }
        debugLog = prefs.getBoolean("debug_log", false);
        showHud = prefs.getBoolean("show_hud", false);
        versionBadge = prefs.getBoolean("version_badge", true);
        prefetch = prefs.getBoolean("prefetch", false);
        snapshotDefer = prefs.getBoolean("snapshot_defer", false);
        bilinearScale = prefs.getBoolean("bilinear_scale", true);
        fontFallback = prefs.getBoolean("font_fallback", false);
        vmProfile = prefs.getBoolean("vm_profile", false);
        assetTiming = prefs.getBoolean("asset_timing", false);
        tickSplit = prefs.getBoolean("tick_split", false);
        cgTiming = prefs.getBoolean("cg_timing", false);

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(BG);
        setContentView(scroll);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        int pad = dp(20);
        root.setPadding(pad, dp(28), pad, dp(24));
        scroll.addView(root, new ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        // ---- 游戏目录 ----
        LinearLayout gameCard = card();
        gameCard.addView(sectionTitle("游戏目录"));
        gameLabel = bodyText();
        gameCard.addView(gameLabel);
        Button pickGame = primaryButton("选择游戏目录");
        pickGame.setOnClickListener(v -> ensureStorageThen(this::pickDirectory));
        gameCard.addView(pickGame, buttonParams());
        // 兜底:个别文件管理器/受限目录(如 Android 11 的 Download 根)不让选,
        // 或者 provider 解不出真实路径时,直接输入绝对路径。
        Button manualGame = secondaryButton("手动输入路径…");
        manualGame.setTextSize(12f);
        manualGame.setOnClickListener(v -> editGameDir());
        gameCard.addView(manualGame, buttonParams());
        gameCard.addView(hintText("选游戏根目录"));
        root.addView(gameCard, cardParams());

        // ---- 存档目录 ----
        LinearLayout saveCard = card();
        saveCard.addView(sectionTitle("存档目录"));
        saveLabel = bodyText();
        saveCard.addView(saveLabel);

        LinearLayout saveRow = new LinearLayout(this);
        saveRow.setOrientation(LinearLayout.HORIZONTAL);
        Button saveDefault = secondaryButton("默认(游戏目录/save)");
        saveDefault.setOnClickListener(v -> {
            saveDir = null;
            refreshLabels();
        });
        Button saveCustom = secondaryButton("另外指定…");
        saveCustom.setOnClickListener(v -> ensureStorageThen(this::pickSaveDirectory));
        LinearLayout.LayoutParams left = new LinearLayout.LayoutParams(0, dp(48), 1f);
        left.topMargin = dp(8);
        left.rightMargin = dp(5);
        LinearLayout.LayoutParams right = new LinearLayout.LayoutParams(0, dp(48), 1f);
        right.topMargin = dp(8);
        right.leftMargin = dp(5);
        saveRow.addView(saveDefault, left);
        saveRow.addView(saveCustom, right);
        saveCard.addView(saveRow);
        root.addView(saveCard, cardParams());

        // ---- 字体 ----
        LinearLayout fontCard = card();
        fontCard.addView(sectionTitle("字体"));
        fontSpinner = new Spinner(this);
        LinearLayout.LayoutParams spinnerParams =
                new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(46));
        spinnerParams.topMargin = dp(6);
        fontCard.addView(fontSpinner, spinnerParams);

        Button importFont = secondaryButton("导入字体文件(ttf / otf / ttc)");
        // 字体路径同样以真实路径交给内核,先确保存储权限。
        importFont.setOnClickListener(v -> ensureStorageThen(this::pickFont));
        fontCard.addView(importFont, buttonParams());
        // 官方原版排版按等宽字体设计:字宽不一致会让行宽/对齐跑掉。
        fontCard.addView(hintText("推荐使用等宽字体"));
        root.addView(fontCard, cardParams());

        // ---- 操作方式(启动时的输入模式) ----
        LinearLayout inputCard = card();
        inputCard.addView(sectionTitle("操作方式"));
        String[] inputLabels = {"触摸", "虚拟光标"};
        inputModeButtons = new Button[inputLabels.length];
        LinearLayout inputRow = new LinearLayout(this);
        inputRow.setOrientation(LinearLayout.HORIZONTAL);
        for (int i = 0; i < inputLabels.length; i++) {
            final int index = i;
            Button button = secondaryButton(inputLabels[i]);
            button.setOnClickListener(v -> {
                mouseMode = index == 1;
                refreshChoiceButtons(inputModeButtons, INPUT_MODE_VALUES, selectedInputMode());
            });
            inputModeButtons[i] = button;
            LinearLayout.LayoutParams params =
                    new LinearLayout.LayoutParams(0, dp(42), 1f);
            params.topMargin = dp(6);
            params.leftMargin = dp(4);
            params.rightMargin = dp(4);
            inputRow.addView(button, params);
        }
        refreshChoiceButtons(inputModeButtons, INPUT_MODE_VALUES, selectedInputMode());
        inputCard.addView(inputRow);
        inputCard.addView(hintText("游戏内右下角小圆钮可切换"));
        root.addView(inputCard, cardParams());

        // ---- 运行(后端 / 渲染器 / 帧率) ----
        LinearLayout runCard = card();
        runCard.addView(sectionTitle("运行"));

        LinearLayout backendRow = new LinearLayout(this);
        backendRow.setOrientation(LinearLayout.HORIZONTAL);
        Button cpuButton = secondaryButton("CPU(已停用)");
        Button gpuButton = secondaryButton("GPU");
        // CPU 模式已停用:保留按钮占位,不可选;Android 固定走 GPU。
        cpuButton.setEnabled(false);
        cpuButton.setAlpha(0.35f);
        gpuButton.setOnClickListener(v -> {
            backend = "wgpu";
            refreshRunButtons(cpuButton, gpuButton);
        });
        LinearLayout.LayoutParams cpuParams = new LinearLayout.LayoutParams(0, dp(46), 1f);
        cpuParams.topMargin = dp(6);
        cpuParams.rightMargin = dp(5);
        LinearLayout.LayoutParams gpuParams = new LinearLayout.LayoutParams(0, dp(46), 1f);
        gpuParams.topMargin = dp(6);
        gpuParams.leftMargin = dp(5);
        backendRow.addView(cpuButton, cpuParams);
        backendRow.addView(gpuButton, gpuParams);
        runCard.addView(backendRow);
        refreshRunButtons(cpuButton, gpuButton);

        // GPU 底层接口三选一(GPU 模式下有意义)。Android 模拟器的 Vulkan
        // 驱动会在提交流程内崩溃,GLES 更稳;真机两者都可。
        runCard.addView(fieldLabel("GPU 接口"));
        LinearLayout gpuRow = new LinearLayout(this);
        gpuRow.setOrientation(LinearLayout.HORIZONTAL);
        Button autoButton = secondaryButton("自动");
        Button glesButton = secondaryButton("GLES");
        Button vulkanButton = secondaryButton("Vulkan");
        autoButton.setOnClickListener(v -> {
            gpuBackend = "auto";
            refreshGpuButtons(autoButton, glesButton, vulkanButton);
        });
        glesButton.setOnClickListener(v -> {
            gpuBackend = "gles";
            refreshGpuButtons(autoButton, glesButton, vulkanButton);
        });
        vulkanButton.setOnClickListener(v -> {
            gpuBackend = "vulkan";
            refreshGpuButtons(autoButton, glesButton, vulkanButton);
        });
        LinearLayout.LayoutParams third = new LinearLayout.LayoutParams(0, dp(42), 1f);
        third.topMargin = dp(6);
        third.rightMargin = dp(4);
        LinearLayout.LayoutParams third2 = new LinearLayout.LayoutParams(0, dp(42), 1f);
        third2.topMargin = dp(6);
        third2.leftMargin = dp(4);
        third2.rightMargin = dp(4);
        LinearLayout.LayoutParams third3 = new LinearLayout.LayoutParams(0, dp(42), 1f);
        third3.topMargin = dp(6);
        third3.leftMargin = dp(4);
        gpuRow.addView(autoButton, third);
        gpuRow.addView(glesButton, third2);
        gpuRow.addView(vulkanButton, third3);
        runCard.addView(gpuRow);
        refreshGpuButtons(autoButton, glesButton, vulkanButton);

        // 帧率上限:壳侧帧节拍与内核 fps 参数同值。
        runCard.addView(fieldLabel("帧率上限"));
        LinearLayout fpsRow = new LinearLayout(this);
        fpsRow.setOrientation(LinearLayout.HORIZONTAL);
        fpsButtons = new Button[FPS_VALUES.length];
        for (int i = 0; i < FPS_VALUES.length; i++) {
            final int value = FPS_VALUES[i];
            Button button = secondaryButton(FPS_LABELS[i]);
            button.setOnClickListener(v -> {
                fps = value;
                refreshFpsButtons();
            });
            fpsButtons[i] = button;
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(42), 1f);
            params.topMargin = dp(6);
            params.leftMargin = dp(4);
            params.rightMargin = dp(4);
            fpsRow.addView(button, params);
        }
        runCard.addView(fpsRow);
        refreshFpsButtons();

        // ---- 高级设置(运行卡片内,可展开/收起) ----
        runCard.addView(fieldLabel("高级设置"));
        Button advancedHeader = secondaryButton("展开 ▸");
        advancedHeader.setTextSize(14f);
        runCard.addView(advancedHeader, buttonParams());
        LinearLayout advancedBody = new LinearLayout(this);
        advancedBody.setOrientation(LinearLayout.VERTICAL);
        advancedBody.setVisibility(View.GONE);
        runCard.addView(advancedBody);
        advancedHeader.setOnClickListener(v -> {
            boolean open = advancedBody.getVisibility() == View.VISIBLE;
            advancedBody.setVisibility(open ? View.GONE : View.VISIBLE);
            advancedHeader.setText(open ? "展开 ▸" : "收起 ▾");
        });

        advancedBody.addView(toggleButton(
                "状态浮层:开(帧率/计数)", "状态浮层:关", showHud, v -> showHud = v), buttonParams());
        advancedBody.addView(toggleButton(
                "版本名角标:开", "版本名角标:关", versionBadge, v -> versionBadge = v), buttonParams());
        advancedBody.addView(toggleButton(
                "调试日志:开(写游戏目录)", "调试日志:关", debugLog, v -> debugLog = v), buttonParams());
        advancedBody.addView(toggleButton(
                "资产预取:开(后台解码)", "资产预取:关", prefetch, v -> prefetch = v), buttonParams());
        advancedBody.addView(toggleButton(
                "快照延迟:开(菜单少拷贝)", "快照延迟:关", snapshotDefer, v -> snapshotDefer = v), buttonParams());
        advancedBody.addView(toggleButton(
                "SIZE 双线性:开(手机抗锯齿)", "SIZE 双线性:关(官方最近邻)",
                bilinearScale, v -> bilinearScale = v), buttonParams());
        advancedBody.addView(toggleButton(
                "缺字补全:开(其它字体补字形)", "缺字补全:关(官方缺字不画)",
                fontFallback, v -> fontFallback = v), buttonParams());

        // 诊断开关:只影响日志/计时,不改画面;默认全关。
        advancedBody.addView(toggleButton(
                "VM 归因:开", "VM 归因:关", vmProfile, v -> vmProfile = v), buttonParams());
        advancedBody.addView(toggleButton(
                "资产计时:开", "资产计时:关", assetTiming, v -> assetTiming = v), buttonParams());
        advancedBody.addView(toggleButton(
                "tick 分段:开", "tick 分段:关", tickSplit, v -> tickSplit = v), buttonParams());
        advancedBody.addView(toggleButton(
                "CG 计时:开", "CG 计时:关", cgTiming, v -> cgTiming = v), buttonParams());
        advancedBody.addView(hintText("诊断开关只影响日志/计时,不改画面;默认全关。"));
        root.addView(runCard, cardParams());

        // ---- 视频解码 ----
        // 官方 MOVIE 是“系统媒体栈能播的任意格式”;Android 上对应
        // MediaExtractor + MediaCodec。内置 pl_mpeg 只解 MPEG-1,所以给
        // 用户三档:自动(推荐)、只用内置、只用设备解码。
        LinearLayout movieCard = card();
        movieCard.addView(sectionTitle("视频解码"));
        Button[] movieButtons = new Button[3];
        String[] movieValues = {"auto", "builtin", "external"};
        String[] movieLabels = {"自动(推荐)", "内置", "外置"};
        LinearLayout movieRow = new LinearLayout(this);
        movieRow.setOrientation(LinearLayout.HORIZONTAL);
        for (int i = 0; i < movieValues.length; i++) {
            final int index = i;
            Button button = secondaryButton(movieLabels[i]);
            button.setOnClickListener(v -> {
                movieBackend = movieValues[index];
                refreshChoiceButtons(movieButtons, movieValues, movieBackend);
                refreshMovieProbe();
            });
            movieButtons[i] = button;
            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(0, dp(42), 1f);
            params.topMargin = dp(6);
            params.leftMargin = dp(4);
            params.rightMargin = dp(4);
            movieRow.addView(button, params);
        }
        movieCard.addView(movieRow);
        refreshChoiceButtons(movieButtons, movieValues, movieBackend);
        movieCard.addView(hintText(
                "自动:优先设备解码,当前文件不受支持时回退内置;"
                        + "内置只解 MPEG-1;外置只走设备 MediaCodec"
                        + "(MP4/MKV 等,具体取决于设备和文件编码)。"));
        movieProbeLabel = bodyText();
        movieProbeLabel.setTextSize(11f);
        movieProbeLabel.setTextColor(MUTED);
        movieProbeLabel.setPadding(0, dp(8), 0, 0);
        movieCard.addView(movieProbeLabel);
        refreshMovieProbe();
        root.addView(movieCard, cardParams());

        // ---- 文本编码 ----
        LinearLayout textCard = card();
        textCard.addView(sectionTitle("文本编码"));
        // 五种路线 = `yu_engine::patch::PatchKind` 全集(与 CLI `--patch`
        // 同一词表)。「自动」是缺省与推荐路线,整行放大置顶;其余四个
        // 两两一行(显式选项都是"知道自己要什么"的用法,次级视觉)。
        Button[] patchButtons = new Button[5];
        String[] patchValues = {"auto", "original", "gbk", "big5", "tunnel"};
        String[] patchLabels = {"自动(推荐)", "原版(CP932)", "简体(GBK)",
                "繁体(Big5)", "隧道(sjis_ext)"};
        // 「自动」:整行、加高(与后端行同规格),强调它是缺省选择。
        {
            final int index = 0;
            Button button = secondaryButton(patchLabels[index]);
            button.setTextSize(15f);
            button.setOnClickListener(v -> {
                patch = patchValues[index];
                refreshChoiceButtons(patchButtons, patchValues, patch);
            });
            patchButtons[index] = button;
            LinearLayout.LayoutParams autoParams = buttonParams();
            autoParams.height = dp(46);
            textCard.addView(button, autoParams);
        }
        // 其余四个:两两一行。
        for (int i = 1; i < patchValues.length; i += 2) {
            LinearLayout row = new LinearLayout(this);
            row.setOrientation(LinearLayout.HORIZONTAL);
            for (int j = i; j < Math.min(i + 2, patchValues.length); j++) {
                final int index = j;
                Button button = secondaryButton(patchLabels[j]);
                button.setOnClickListener(v -> {
                    patch = patchValues[index];
                    refreshChoiceButtons(patchButtons, patchValues, patch);
                });
                patchButtons[j] = button;
                LinearLayout.LayoutParams params =
                        new LinearLayout.LayoutParams(0, dp(42), 1f);
                params.topMargin = dp(6);
                params.rightMargin = dp(4);
                params.leftMargin = dp(4);
                row.addView(button, params);
            }
            textCard.addView(row);
        }
        refreshChoiceButtons(patchButtons, patchValues, patch);
        textCard.addView(hintText(
                "汉化包整包换过编码时才需要显式选;选错会整屏乱码。"
                        + "「隧道」需要游戏目录里有 sjis_ext.bin,缺失会显式报错。"));
        root.addView(textCard, cardParams());

        // ---- 存档 ----
        LinearLayout saveCodecCard = card();
        saveCodecCard.addView(sectionTitle("存档"));
        Button[] codecButtons = new Button[3];
        String[] codecValues = {"", "zlib", "snappy"};
        String[] codecLabels = {"自动判定", "zlib", "snappy"};
        LinearLayout codecRow = new LinearLayout(this);
        codecRow.setOrientation(LinearLayout.HORIZONTAL);
        for (int i = 0; i < codecValues.length; i++) {
            final int index = i;
            Button button = secondaryButton(codecLabels[i]);
            button.setOnClickListener(v -> {
                saveCodec = codecValues[index];
                refreshChoiceButtons(codecButtons, codecValues, saveCodec);
            });
            codecButtons[i] = button;
            LinearLayout.LayoutParams params =
                    new LinearLayout.LayoutParams(0, dp(42), 1f);
            params.topMargin = dp(6);
            params.leftMargin = dp(4);
            params.rightMargin = dp(4);
            codecRow.addView(button, params);
        }
        refreshChoiceButtons(codecButtons, codecValues, saveCodec);
        saveCodecCard.addView(codecRow);
        saveCodecCard.addView(hintText(
                "只影响写盘;读侧始终按实际流分流。原版带 YSSnp.dll 的作品是 snappy。"));
        root.addView(saveCodecCard, cardParams());

        // ---- 开始 ----
        startButton = primaryButton("开始游戏");
        // 启动同样走权限闸:手动输入路径/恢复旧配置时可能从未授权过。
        startButton.setOnClickListener(v -> ensureStorageThen(this::launchGame));
        LinearLayout.LayoutParams startParams = buttonParams();
        startParams.topMargin = dp(22);
        root.addView(startButton, startParams);

        Button reset = secondaryButton("清除已保存的选择");
        reset.setTextSize(12f);
        reset.setOnClickListener(v -> {
            getSharedPreferences(PREFS, MODE_PRIVATE).edit().clear().apply();
            gameDir = null;
            saveDir = null;
            fontPath = null;
            importedFontName = null;
            savedFontFamily = "";
            mouseMode = false;
            refreshChoiceButtons(inputModeButtons, INPUT_MODE_VALUES, selectedInputMode());
            fps = 60;
            refreshFpsButtons();
            refreshLabels();
            // 下拉去掉「导入字体」项并回到系统默认。
            fontSpinner.setSelection(0);
        });
        root.addView(reset, buttonParams());

        status = bodyText();
        status.setTextSize(12f);
        status.setTextColor(MUTED);
        LinearLayout.LayoutParams statusParams = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        statusParams.topMargin = dp(14);
        root.addView(status, statusParams);

        refreshLabels();
        // 字体枚举要调 native:先把 NDK 上下文备好(与 MainActivity 同序)。
        NativeHost.nativeInitAndroidContext(this);
        loadSystemFonts();
        playEntrance(root);
        // Tyranor-Next 模块入口放在 UI 建好之后:自动启动复用本页已有的
        // 权限闸与启动路径,桌面直接从图标进来时这里是无操作。
        handleExternalLaunch(getIntent());
    }

    // ------------------------------------------------------------ 权限与目录

    /**
     * 访问游戏目录前先确保存储权限,已授权才执行 {@code next}。
     *
     * <p>Android 11+ 用「所有文件访问」;Android 10 及以下没有这个权限,
     * {@link Environment#isExternalStorageManager()} 也是 API 30 才有的方法 ——
     * 无条件调用会在 Android 10 上抛 {@link NoSuchMethodError},按钮直接失效,
     * 表现为「点了没反应/闪退且从不弹权限」。那里改走旧存储模型:
     * 运行时申请 READ/WRITE,配合清单里的 requestLegacyExternalStorage。
     */
    private void ensureStorageThen(Runnable next) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            if (Environment.isExternalStorageManager()) {
                next.run();
            } else {
                status.setText("需要「所有文件访问」权限才能直接读写游戏目录");
                try {
                    Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
                    intent.setData(Uri.parse("package:" + getPackageName()));
                    startActivity(intent);
                } catch (Exception error) {
                    try {
                        startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
                    } catch (Exception fallbackError) {
                        status.setText("请到系统设置里为本应用开启「所有文件访问」");
                    }
                }
            }
            return;
        }
        // Android 10 及以下:旧存储模型,内核按真实路径读写共享存储需要运行时权限。
        if (checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                != PackageManager.PERMISSION_GRANTED) {
            pendingStorageAction = next;
            status.setText("需要存储权限才能直接读写游戏目录");
            requestPermissions(new String[] {
                    Manifest.permission.READ_EXTERNAL_STORAGE,
                    Manifest.permission.WRITE_EXTERNAL_STORAGE,
            }, REQUEST_STORAGE);
            return;
        }
        next.run();
    }

    // ------------------------------------------------------------ Tyranor 模块入口

    /**
     * Tyranor-Next 第三方运行时模块的启动入口。
     *
     * <p>宿主只传游戏根目录绝对路径(不传 SAF URI),与本壳「按真实路径读写」
     * 的模型天然一致;这里把路径落到与手动选目录相同的状态,再走同一套
     * 权限闸和启动路径。协议版本不认识时忽略 extras,停在配置页。
     */
    private void handleExternalLaunch(Intent intent) {
        if (intent == null || !TYRANOR_ACTION_LAUNCH.equals(intent.getAction())) {
            return;
        }
        int protocol = intent.getIntExtra(
                TYRANOR_EXTRA_PROTOCOL_VERSION, TYRANOR_PROTOCOL_VERSION);
        if (protocol != TYRANOR_PROTOCOL_VERSION) {
            status.setText("不支持的 Tyranor 模块协议版本: " + protocol);
            return;
        }
        String folder = intent.getStringExtra(TYRANOR_EXTRA_FOLDER_PATH);
        if (folder == null || folder.trim().isEmpty()) {
            status.setText("Tyranor 未提供游戏目录");
            return;
        }
        File dir = new File(folder);
        if (!dir.isAbsolute() || !dir.isDirectory()) {
            status.setText("Tyranor 指定的游戏目录不存在: " + folder);
            return;
        }
        String nextGameDir = dir.getAbsolutePath();
        // 换游戏时不要把上一个游戏的自定义存档目录带过来。
        if (gameDir != null && !gameDir.equals(nextGameDir)) {
            saveDir = null;
        }
        gameDir = nextGameDir;
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                .putString("game_dir", gameDir)
                .putString("save_dir", saveDir)
                .apply();
        String title = intent.getStringExtra(TYRANOR_EXTRA_GAME_TITLE);
        if (title != null && !title.isEmpty()) {
            status.setText("来自 Tyranor: " + title);
        }
        // 宿主按游戏保存的「引擎设置」以完整生效值下发;未声明的 key 忽略。
        applyTyranorSettings(intent.getStringExtra(TYRANOR_EXTRA_SETTINGS));
        refreshLabels();
        pendingExternalLaunch = true;
        // 首次运行还没授「所有文件访问」:这里拉起系统授权页,回来后 onResume 重试。
        ensureStorageThen(this::requestExternalLaunch);
    }

    /** 权限与字体都就绪后才真正进游戏;字体还在后台枚举时挂起等待。 */
    private void requestExternalLaunch() {
        if (!pendingExternalLaunch) {
            return;
        }
        if (fontSpinner.getAdapter() == null) {
            pendingAfterFonts = this::launchExternalGame;
            return;
        }
        launchExternalGame();
    }

    private void launchExternalGame() {
        if (!pendingExternalLaunch) {
            return;
        }
        pendingExternalLaunch = false;
        pendingAfterFonts = null;
        externalSession = true;
        launchGame();
    }

    @Override
    protected void onResume() {
        super.onResume();
        // 从「所有文件访问」授权页返回:权限到手就继续 Tyranor 派发的启动。
        if (pendingExternalLaunch && hasStorageAccess()) {
            requestExternalLaunch();
        }
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handleExternalLaunch(intent);
    }

    /** 当前是否已拿到内核读写真实路径所需的存储权限。 */
    private boolean hasStorageAccess() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager();
        }
        return checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE)
                == PackageManager.PERMISSION_GRANTED;
    }

    /**
     * 消费 Tyranor 下发的单游戏「引擎设置」。
     *
     * <p>payload 形如 {@code {"version":1,"values":{...}}};只认白名单 key
     * 与合法类型,未知 key 忽略,版本不认识时整份忽略。字符串枚举再按声明
     * 值白名单校验,避免把任意字符串塞进内核参数。
     */
    private void applyTyranorSettings(String raw) {
        if (raw == null || raw.isEmpty()) {
            return;
        }
        JSONObject payload;
        try {
            payload = new JSONObject(raw);
        } catch (JSONException error) {
            return;
        }
        if (payload.optInt("version", -1) != 1) {
            return;
        }
        JSONObject values = payload.optJSONObject("values");
        if (values == null) {
            return;
        }
        String fpsValue = optString(values, "fps");
        if ("30".equals(fpsValue) || "60".equals(fpsValue) || "120".equals(fpsValue)) {
            fps = Integer.parseInt(fpsValue);
            refreshFpsButtons();
        }
        if (values.opt("mouse_mode") instanceof Boolean) {
            mouseMode = values.optBoolean("mouse_mode", mouseMode);
            refreshChoiceButtons(inputModeButtons, INPUT_MODE_VALUES, selectedInputMode());
        }
        if (values.opt("bilinear_scale") instanceof Boolean) {
            bilinearScale = values.optBoolean("bilinear_scale", bilinearScale);
        }
        if (values.opt("font_fallback") instanceof Boolean) {
            fontFallback = values.optBoolean("font_fallback", fontFallback);
        }
        if (values.opt("debug_log") instanceof Boolean) {
            debugLog = values.optBoolean("debug_log", debugLog);
        }
        if (values.opt("show_hud") instanceof Boolean) {
            showHud = values.optBoolean("show_hud", showHud);
        }
        String patchValue = optString(values, "patch");
        if (isOneOf(patchValue, "auto", "original", "gbk", "big5", "tunnel")) {
            patch = patchValue;
        }
        String gpuValue = optString(values, "gpu_backend");
        if (isOneOf(gpuValue, "auto", "gles", "vulkan")) {
            gpuBackend = gpuValue;
        }
        String movieValue = optString(values, "movie_backend");
        if (isOneOf(movieValue, "auto", "builtin", "external")) {
            movieBackend = movieValue;
        }
        String fontValue = optString(values, "font");
        if (fontValue != null) {
            applyFontSetting(fontValue);
        }
    }

    private static String optString(JSONObject values, String key) {
        Object value = values.opt(key);
        return value instanceof String ? (String) value : null;
    }

    private static boolean isOneOf(String value, String... allowed) {
        if (value == null) {
            return false;
        }
        for (String item : allowed) {
            if (item.equals(value)) {
                return true;
            }
        }
        return false;
    }

    /**
     * 字体设置:空 = 系统默认(本游戏 {@code fonts/} 下导入的字体仍由内核
     * 自动加载);含路径分隔符或字体扩展名 = 显式字体文件;其余按字体族名。
     */
    private void applyFontSetting(String value) {
        String trimmed = value.trim();
        if (trimmed.isEmpty()) {
            // 宿主没有 per-game 覆盖(声明默认值 ""):保留模块自己的字体选择。
            // 以前这里清空 fontPath/importedFontName,导致从 Tyranor 启动后
            // 配置页字体下拉的最后一项(已导入字体)消失,也覆盖了用户在模块内
            // 做的全局选择;`fonts/` 的自动默认仍由内核侧处理。
            restoreFontSelection();
            return;
        } else if (trimmed.endsWith(".ttf") || trimmed.endsWith(".otf")
                || trimmed.endsWith(".ttc") || trimmed.contains("/")
                || trimmed.contains("\\")) {
            File file = new File(trimmed);
            if (file.isFile()) {
                fontPath = file.getAbsolutePath();
                importedFontName = file.getName();
                savedFontFamily = importedFontName;
            } else {
                // 坏路径不交给内核:退回系统默认,避免启动即报字体错误。
                savedFontFamily = "";
                fontPath = null;
                importedFontName = null;
            }
        } else {
            savedFontFamily = trimmed;
            fontPath = null;
            importedFontName = null;
        }
        restoreFontSelection();
    }

    /** 字体枚举可能已/尚未完成:能恢复就立即恢复,否则等 loadSystemFonts 回调。 */
    private void restoreFontSelection() {
        if (fontSpinner == null || fontSpinner.getAdapter() == null) {
            return;
        }
        List<String> entries = new ArrayList<>();
        entries.add("系统默认");
        entries.addAll(systemFonts);
        if (importedFontName != null) {
            entries.add(importedFontName);
        }
        fontSpinner.setSelection(indexOfFamily(savedFontFamily, entries));
    }

    @Override
    public void onRequestPermissionsResult(
            int requestCode, String[] permissions, int[] grantResults) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults);
        if (requestCode != REQUEST_STORAGE) {
            return;
        }
        Runnable next = pendingStorageAction;
        pendingStorageAction = null;
        boolean granted = grantResults.length > 0
                && grantResults[0] == PackageManager.PERMISSION_GRANTED;
        if (!granted) {
            status.setText("没有存储权限:内核无法按真实路径读写游戏目录");
            return;
        }
        if (next != null) {
            next.run();
        }
    }

    /**
     * 选游戏目录(系统目录选择器,不是文件选择器)。
     *
     * <p>旧实现用「打开文档」:只能点文件,再取父目录猜游戏根 —— 用户必须
     * 先在游戏目录里随便点一个文件。现在直接用 {@code ACTION_OPEN_DOCUMENT_TREE}
     * 让用户选文件夹,选中项经 {@link #realPathOf} 还原成真实路径交给内核
     * (内核按真实路径读写,content:// URI 用不了)。
     */
    private void pickDirectory() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent, REQUEST_GAME_DIR);
    }

    /**
     * 手动输入游戏根路径(`/sdcard/...` 或 `/storage/...`)。
     *
     * <p>兜底路线:部分文件管理器不实现目录选择,或某些受限目录
     * (Android 11+ 的 Download 根)不允许通过 SAF 选中。
     */
    private void editGameDir() {
        final android.widget.EditText input = new android.widget.EditText(this);
        input.setText(gameDir != null ? gameDir : Environment.getExternalStorageDirectory().getAbsolutePath());
        input.setHint("/sdcard/Download/yugame");
        input.setSingleLine(true);
        new androidx.appcompat.app.AlertDialog.Builder(this)
                .setTitle("游戏目录路径")
                .setView(input)
                .setPositiveButton("确定", (dialog, which) -> {
                    String text = input.getText().toString().trim();
                    if (text.isEmpty()) {
                        gameDir = null;
                        refreshLabels();
                        return;
                    }
                    File picked = new File(text);
                    File dir = picked.isDirectory() ? picked : picked.getParentFile();
                    // 与选目录同一原则:不拒绝。能自动定位到根就用根,否则照收原样,
                    // 内核启动时会给出具体错误。
                    String root = dir != null ? findGameRoot(dir) : null;
                    gameDir = root != null ? root : picked.getAbsolutePath();
                    refreshLabels();
                })
                .setNegativeButton("取消", null)
                .show();
    }

    private void pickFont() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        startActivityForResult(intent, REQUEST_FONT);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_PLAY) {
            boolean failed = false;
            if (data != null) {
                String error = data.getStringExtra(MainActivity.EXTRA_START_ERROR);
                if (error != null && !error.isEmpty()) {
                    failed = true;
                    status.setText(error);
                    // 失败后自动切回 CPU,避免用户反复踩同一个坑。
                    backend = "soft";
                }
            }
            if (externalSession && !failed) {
                // 正常退出游戏:关掉配置页,把用户还给 Tyranor。
                externalSession = false;
                finish();
            }
            return;
        }
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        Uri uri = data.getData();
        if (requestCode == REQUEST_GAME_DIR) {
            String root = resolveGameRoot(uri);
            if (root == null) {
                return;
            }
            gameDir = root;
            refreshLabels();
            return;
        }
        if (requestCode == REQUEST_SAVE_DIR) {
            String realPath = realPathOf(uri);
            if (realPath == null) {
                status.setText("无法解析该目录的路径");
                return;
            }
            File picked = new File(realPath);
            if (!picked.isDirectory()) {
                picked = picked.getParentFile();
            }
            if (picked == null) {
                status.setText("无法解析该目录的路径");
                return;
            }
            saveDir = picked.getAbsolutePath();
            refreshLabels();
            return;
        }
        String realPath = realPathOf(uri);
        if (realPath == null) {
            status.setText("无法解析该位置的路径,请从「内部存储 / 下载」里选");
            return;
        }
        File picked = new File(realPath);
        if (requestCode == REQUEST_FONT) {
            fontPath = picked.getAbsolutePath();
            importedFontName = picked.getName();
            // 导入即选中:重新加载下拉后直接定位到它。
            savedFontFamily = importedFontName;
            loadSystemFonts();
        }
    }

    /**
     * 把目录选择器返回的 tree URI 解析成真实路径,能识别时顺带定位游戏根。
     *
     * <p><b>选目录不是门槛</b>:只要 URI 能还原成真实路径就接受,不做任何
     * 准入判断 —— 能不能跑由内核启动时判定,错误会回显到状态栏。
     *
     * <p>自动定位只是便利:选中目录 → 向上 4 层 → 唯一一个能当数据入口的
     * 直接子目录(用户选了游戏上一级的情况)。一个都没命中就用用户选中的
     * 目录本身。判据交给内核({@link NativeHost#nativeProbeGameRoot},与启动时
     * `Project::discover` 同一份代码),壳不手抄规则。
     */
    private String resolveGameRoot(Uri uri) {
        String realPath = realPathOf(uri);
        if (realPath == null) {
            status.setText("无法解析该目录的路径,请换用「手动输入路径…」");
            return null;
        }
        File picked = new File(realPath);
        if (!picked.isDirectory()) {
            picked = picked.getParentFile();
        }
        if (picked == null) {
            status.setText("无法解析该目录的路径,请换用「手动输入路径…」");
            return null;
        }
        // 便利:选中的是游戏里的子目录或游戏的上一级时自动落到根上。
        String root = findGameRoot(picked);
        if (root != null) {
            return root;
        }
        File only = singleChildGameRoot(picked);
        if (only != null) {
            return only.getAbsolutePath();
        }
        // 认不出来也照收:内核启动时会给出具体错误。
        return picked.getAbsolutePath();
    }

    /** 直接子目录里只有**一个**能当数据入口时返回它;0 个或多个都回 null。 */
    private File singleChildGameRoot(File dir) {
        File[] children = dir.listFiles();
        if (children == null) {
            return null;
        }
        File found = null;
        for (File child : children) {
            if (child.isDirectory() && probeGameRoot(child) != null) {
                if (found != null) {
                    return null; // 多个候选:不替用户猜
                }
                found = child;
            }
        }
        return found;
    }

    /** 从选中位置向上找能当数据入口的目录;找不到回 null(调用方自行兜底)。 */
    private String findGameRoot(File start) {
        File cursor = start;
        for (int depth = 0; cursor != null && depth < 4; depth++) {
            String root = probeGameRoot(cursor);
            if (root != null) {
                return root;
            }
            cursor = cursor.getParentFile();
        }
        return null;
    }

    /**
     * 让内核判断这个目录是不是数据入口(`Project::discover` 同一判据)。
     *
     * <p>只用于**自动定位**,不是选择门槛;JNI 不可用时也返回 null 并记日志,
     * 不崩溃、不拦用户。
     *
     * @return 数据根路径;不是数据入口时 {@code null}
     */
    private static String probeGameRoot(File dir) {
        if (dir == null) {
            return null;
        }
        String reply;
        try {
            reply = NativeHost.nativeProbeGameRoot(dir.getAbsolutePath());
        } catch (Throwable error) {
            Log.w(TAG, "nativeProbeGameRoot failed", error);
            return null;
        }
        try {
            org.json.JSONObject json = new org.json.JSONObject(reply == null ? "" : reply);
            if (!json.optBoolean("ok", false)) {
                return null;
            }
            String root = json.optString("root", "");
            return root.isEmpty() ? dir.getAbsolutePath() : root;
        } catch (Exception error) {
            Log.w(TAG, "nativeProbeGameRoot reply is not JSON: " + reply, error);
        }
        return null;
    }

    /** 把 document / tree URI 还原成真实路径(文件或目录;需要所有文件访问权限)。 */
    private String realPathOf(Uri uri) {
        if ("file".equals(uri.getScheme())) {
            return uri.getPath();
        }
        // 先问 provider 要 _data(部分 provider 直接给真实路径)。
        String dataPath = queryDataPath(uri);
        if (dataPath != null) {
            return dataPath;
        }
        String docId;
        try {
            docId = documentIdOf(uri);
        } catch (Exception ignored) {
            return null;
        }
        if (docId == null) {
            return null;
        }
        // 目录选择器给的是 tree URI:再对它对应的 document URI 试一次 _data
        // (Downloads/MediaStore 一类 provider 只有 document 才认 _data)。
        if (isTreeUri(uri)) {
            try {
                Uri document = android.provider.DocumentsContract
                        .buildDocumentUriUsingTree(uri, docId);
                String documentPath = queryDataPath(document);
                if (documentPath != null) {
                    return documentPath;
                }
            } catch (Exception ignored) {
                // provider 不支持 document 形态,继续按 docId 拼。
            }
        }
        // 旧版 DownloadsProvider 直接给 raw: 绝对路径。
        if (docId.startsWith("raw:")) {
            return docId.substring(4);
        }
        // Download 根在 DownloadsProvider 里的 ID 就是 "downloads"(无冒号)。
        if ("downloads".equals(docId)) {
            return Environment.getExternalStoragePublicDirectory(
                    Environment.DIRECTORY_DOWNLOADS).getAbsolutePath();
        }
        int colon = docId.indexOf(':');
        if (colon < 0) {
            return null;
        }
        String volume = docId.substring(0, colon);
        String relative = docId.substring(colon + 1);
        if (relative.startsWith("raw:")) {
            return relative.substring(4);
        }
        // "home" 等 Documents 专用卷的相对路径基准不是 /storage/<volume>,
        // 不映射 —— 给错路径比明确报错更难排查。
        if ("home".equals(volume)) {
            return null;
        }
        File base = "primary".equals(volume)
                ? Environment.getExternalStorageDirectory()
                : new File("/storage", volume);
        if (relative.isEmpty()) {
            return base.isDirectory() ? base.getAbsolutePath() : null;
        }
        // "primary" = 内部共享存储;其它卷 ID(如 SD 卡的 1A2B-3C4D)在
        // /storage/<volume> 下。
        if (!"primary".equals(volume) && volume.length() < 2) {
            return null;
        }
        File resolved = new File(base, relative);
        // 目录选择器返回目录,字体选择返回文件;两者都必须接受。
        // 旧实现只认 isDirectory(),所以导入字体时即便 URI 能还原出真实
        // 路径也会被当成失败,提示「无法解析该位置的路径」。
        return resolved.exists() ? resolved.getAbsolutePath() : null;
    }

    /**
     * 取 URI 的文档 ID。
     *
     * <p><b>tree URI 必须用 {@code getTreeDocumentId}</b>:{@code getDocumentId}
     * 只接受 document URI,对 {@code .../tree/primary%3ADownload%2Fgame} 会抛
     * {@link IllegalArgumentException} —— 旧实现把它吞掉后返回 null,用户看到
     * 的就是「明明能选却提示无法解析该目录的路径」。
     */
    private static String documentIdOf(Uri uri) {
        List<String> segments = uri.getPathSegments();
        boolean tree = segments.size() >= 2 && "tree".equals(segments.get(0));
        return tree
                ? android.provider.DocumentsContract.getTreeDocumentId(uri)
                : android.provider.DocumentsContract.getDocumentId(uri);
    }

    private static boolean isTreeUri(Uri uri) {
        List<String> segments = uri.getPathSegments();
        return segments.size() >= 2 && "tree".equals(segments.get(0));
    }

    /** 查 `_data` 列;provider 不支持时返回 null(不抛)。 */
    private String queryDataPath(Uri uri) {
        try (android.database.Cursor cursor = getContentResolver().query(uri,
                new String[]{"_data"}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                String path = cursor.getString(0);
                if (path != null && !path.isEmpty()) {
                    return path;
                }
            }
        } catch (Exception ignored) {
            // 部分 provider 不提供 _data。
        }
        return null;
    }

    // ------------------------------------------------------------ 显示与样式

    /** 后端二选一高亮:选中的用主色,未选的用次要色。 */
    private void refreshRunButtons(Button cpu, Button gpu) {
        boolean gpuSelected = "wgpu".equals(backend);
        cpu.setBackground(rounded(gpuSelected ? Color.parseColor("#252b36") : ACCENT, 11));
        cpu.setTextColor(gpuSelected ? TEXT : Color.WHITE);
        gpu.setBackground(rounded(gpuSelected ? ACCENT : Color.parseColor("#252b36"), 11));
        gpu.setTextColor(gpuSelected ? Color.WHITE : TEXT);
    }

    /** 当前输入方式在按钮取值里的字符串。 */
    private String selectedInputMode() {
        return mouseMode ? "mouse" : "touch";
    }

    /**
     * 任意「取值 → 高亮」按钮组:选中项用 ACCENT,其余用卡片底色。
     *
     * <p>与 {@link #refreshGpuButtons} 同一视觉语言,但取值与文案都不写死,
     * 供编码/存档层等新增条目复用。
     */
    private void refreshChoiceButtons(Button[] buttons, String[] values, String selected) {
        for (int i = 0; i < buttons.length && i < values.length; i++) {
            if (buttons[i] == null) {
                continue;
            }
            boolean on = values[i].equals(selected);
            buttons[i].setBackground(rounded(on ? ACCENT : Color.parseColor("#252b36"), 11));
            buttons[i].setTextColor(on ? Color.WHITE : TEXT);
        }
    }

    /** 一行小字说明(卡片内提示)。 */
    private TextView hintText(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextSize(11f);
        view.setTextColor(MUTED);
        view.setPadding(0, dp(8), 0, 0);
        return view;
    }

    /** 卡片内的小节标签(与卡片标题同色,带顶部留白)。 */
    private TextView fieldLabel(String text) {
        TextView view = sectionTitle(text);
        view.setPadding(0, dp(12), 0, 0);
        return view;
    }

    /** GPU 接口三选一高亮。 */
    private void refreshGpuButtons(Button auto, Button gles, Button vulkan) {
        Button[] all = {auto, gles, vulkan};
        String[] values = {"auto", "gles", "vulkan"};
        for (int i = 0; i < all.length; i++) {
            boolean on = values[i].equals(gpuBackend);
            all[i].setBackground(rounded(on ? ACCENT : Color.parseColor("#252b36"), 11));
            all[i].setTextColor(on ? Color.WHITE : TEXT);
        }
    }

    /** 视频解码能力摘要:内置不需要探测;自动/外置显示引擎与设备两侧能力。 */
    private void refreshMovieProbe() {
        if (movieProbeLabel == null) {
            return;
        }
        if ("builtin".equals(movieBackend)) {
            movieProbeLabel.setText("内置:MPEG-1 program stream + MP2 音频;不依赖设备解码器。");
            return;
        }
        // 探测会加载 so + 枚举 codec;只做一次,避免每次点按钮都付代价。
        if (movieProbeText == null) {
            movieProbeText = MovieCodecProbe.summary();
        }
        movieProbeLabel.setText(movieProbeText);
    }

    /** 帧率按钮高亮。 */
    private void refreshFpsButtons() {
        if (fpsButtons == null) {
            return;
        }
        String[] values = new String[FPS_VALUES.length];
        for (int i = 0; i < FPS_VALUES.length; i++) {
            values[i] = String.valueOf(FPS_VALUES[i]);
        }
        refreshChoiceButtons(fpsButtons, values, String.valueOf(fps));
    }

    private void refreshLabels() {
        if (gameLabel != null) {
            gameLabel.setText(gameDir == null ? "尚未选择" : gameDir);
            gameLabel.setTextColor(gameDir == null ? MUTED : TEXT);
        }
        if (saveLabel != null) {
            saveLabel.setText(saveDir == null ? "游戏目录/save" : saveDir);
            saveLabel.setTextColor(MUTED);
        }
        if (startButton != null) {
            boolean ready = gameDir != null;
            startButton.setEnabled(ready);
            startButton.setAlpha(ready ? 1f : 0.45f);
        }
    }

    /** 选存档目录(系统目录选择器)。 */
    private void pickSaveDirectory() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        startActivityForResult(intent, REQUEST_SAVE_DIR);
    }

    private void loadSystemFonts() {
        new Thread(() -> {
            List<String> families = new ArrayList<>();
            try {
                String raw = NativeHost.nativeListFonts();
                if (raw != null && !raw.isEmpty()) {
                    for (String line : raw.split("\\n")) {
                        String name = line.trim();
                        if (!name.isEmpty()) {
                            families.add(name);
                        }
                    }
                }
            } catch (Throwable error) {
                Log.w(TAG, "nativeListFonts failed", error);
            }
            List<String> collected = families;
            runOnUiThread(() -> {
                systemFonts = collected;
                List<String> entries = new ArrayList<>();
                entries.add("系统默认");
                entries.addAll(systemFonts);
                // 已导入的字体作为下拉里的最后一项(选中即生效,不再单独显示)。
                if (importedFontName != null) {
                    entries.add(importedFontName);
                }
                ArrayAdapter<String> adapter = new ArrayAdapter<>(
                        this, android.R.layout.simple_spinner_dropdown_item, entries);
                fontSpinner.setAdapter(adapter);
                fontSpinner.setEnabled(true);
                // 恢复上次选择(游戏目录同级持久化)。
                int restore = indexOfFamily(savedFontFamily, entries);
                fontSpinner.setSelection(restore);
                // 外部启动若在等字体枚举,恢复完成后再进游戏,避免丢字体选择。
                Runnable afterFonts = pendingAfterFonts;
                pendingAfterFonts = null;
                if (afterFonts != null) {
                    afterFonts.run();
                }
            });
        }, "yu-font-list").start();
    }

    /** 已导入字体的显示名(下拉最后一项);`null` = 没有导入过。 */
    private String importedFontName;

    /** 在下拉条目里找 family 的索引;找不到回 0(系统默认)。 */
    private int indexOfFamily(String family, List<String> entries) {
        if (family == null || family.isEmpty()) {
            return 0;
        }
        for (int i = 1; i < entries.size(); i++) {
            if (family.equals(entries.get(i))) {
                return i;
            }
        }
        return 0;
    }

    private void launchGame() {
        if (gameDir == null) {
            return;
        }
        // 手动点「开始游戏」同样消费掉待处理的外部启动,避免从游戏返回后重入。
        pendingExternalLaunch = false;
        pendingAfterFonts = null;
        // 下拉索引 0 = 系统默认;1..N = 系统字体;最后一项(若有)= 导入字体。
        String family = "";
        String chosenFontFile = null;
        int position = fontSpinner.getSelectedItemPosition();
        if (position > 0 && position - 1 < systemFonts.size()) {
            family = systemFonts.get(position - 1);
        } else if (importedFontName != null && position == systemFonts.size() + 1) {
            // 选了导入字体:把它当作 family 交给内核内部加载的文件路径。
            chosenFontFile = fontPath;
        }
        if (position >= 0) {
            savedFontFamily = family.isEmpty()
                    ? (importedFontName != null && position == systemFonts.size() + 1
                        ? importedFontName : "")
                    : family;
        }
        String effectiveSave = saveDir != null ? saveDir : new File(gameDir, "save").getAbsolutePath();
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                .putString("game_dir", gameDir)
                .putString("save_dir", saveDir)
                .putString("font_family", savedFontFamily)
                // 导入字体的文件路径必须持久化:否则重启后 importedFontName
                // 重建不出来,下拉最后一项消失、选中恢复失败(实测)。
                .putString("font_path", fontPath)
                .putString("backend", backend)
                .putString("gpu_backend", gpuBackend)
                .putString("patch", patch)
                .putString("save_codec", saveCodec)
                .putString("movie_backend", movieBackend)
                // 输入方式的启动值来自本页;游戏内切换不持久化。
                .putBoolean("launch_mouse_mode", mouseMode)
                .putInt("fps", fps)
                .putBoolean("debug_log", debugLog)
                .putBoolean("show_hud", showHud)
                .putBoolean("version_badge", versionBadge)
                .putBoolean("prefetch", prefetch)
                .putBoolean("snapshot_defer", snapshotDefer)
                .putBoolean("bilinear_scale", bilinearScale)
                .putBoolean("font_fallback", fontFallback)
                .putBoolean("vm_profile", vmProfile)
                .putBoolean("asset_timing", assetTiming)
                .putBoolean("tick_split", tickSplit)
                .putBoolean("cg_timing", cgTiming)
                .apply();
        Intent intent = new Intent(this, MainActivity.class);
        intent.putExtra(MainActivity.EXTRA_GAME_DIR, gameDir);
        intent.putExtra(MainActivity.EXTRA_SAVE_DIR, effectiveSave);
        intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, family);
        intent.putExtra(MainActivity.EXTRA_BACKEND, backend);
        intent.putExtra(MainActivity.EXTRA_GPU_BACKEND, gpuBackend);
        intent.putExtra(MainActivity.EXTRA_MOVIE_BACKEND, movieBackend);
        // 输入方式:带 Extra = 壳的显式选择,MainActivity 以它为准。
        intent.putExtra(MainActivity.EXTRA_MOUSE_MODE, mouseMode);
        intent.putExtra(MainActivity.EXTRA_FPS, fps);
        intent.putExtra(MainActivity.EXTRA_DEBUG_LOG, debugLog);
        intent.putExtra(MainActivity.EXTRA_SHOW_HUD, showHud);
        intent.putExtra(MainActivity.EXTRA_VERSION_BADGE, versionBadge);
        intent.putExtra(MainActivity.EXTRA_PREFETCH, prefetch);
        intent.putExtra(MainActivity.EXTRA_SNAPSHOT_DEFER, snapshotDefer);
        intent.putExtra(MainActivity.EXTRA_BILINEAR_SCALE, bilinearScale);
        intent.putExtra(MainActivity.EXTRA_FONT_FALLBACK, fontFallback);
        intent.putExtra(MainActivity.EXTRA_VM_PROFILE, vmProfile);
        intent.putExtra(MainActivity.EXTRA_ASSET_TIMING, assetTiming);
        intent.putExtra(MainActivity.EXTRA_TICK_SPLIT, tickSplit);
        intent.putExtra(MainActivity.EXTRA_CG_TIMING, cgTiming);
        intent.putExtra(MainActivity.EXTRA_PATCH, patch);
        if (!saveCodec.isEmpty()) {
            intent.putExtra(MainActivity.EXTRA_SAVE_CODEC, saveCodec);
        }
        String fontFileToUse = chosenFontFile != null ? chosenFontFile : fontPath;
        if (fontFileToUse != null) {
            intent.putExtra(MainActivity.EXTRA_FONT_PATH, fontFileToUse);
        }
        // 用 forResult:GPU 等启动失败时把原因带回来显示(不必重开配置界面)。
        startActivityForResult(intent, REQUEST_PLAY);
        overridePendingTransition(android.R.anim.fade_in, android.R.anim.fade_out);
    }

    private void playEntrance(View content) {
        content.setAlpha(0f);
        content.setTranslationY(dp(18));
        content.animate().alpha(1f).translationY(0f)
                .setDuration(280).setInterpolator(new DecelerateInterpolator()).start();
    }

    private LinearLayout card() {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        GradientDrawable background = new GradientDrawable();
        background.setColor(CARD);
        background.setCornerRadius(dp(14));
        card.setBackground(background);
        card.setPadding(dp(16), dp(14), dp(16), dp(14));
        return card;
    }

    private LinearLayout.LayoutParams cardParams() {
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT);
        params.topMargin = dp(12);
        return params;
    }

    private TextView sectionTitle(String text) {
        TextView view = new TextView(this);
        view.setText(text);
        view.setTextColor(MUTED);
        view.setTextSize(11f);
        view.setLetterSpacing(0.16f);
        return view;
    }

    private TextView bodyText() {
        TextView view = new TextView(this);
        view.setTextColor(TEXT);
        view.setTextSize(13f);
        view.setPadding(0, dp(6), 0, dp(2));
        return view;
    }

    private Button primaryButton(String text) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextColor(Color.WHITE);
        button.setTextSize(15f);
        button.setAllCaps(false);
        button.setBackground(rounded(ACCENT, 11));
        button.setStateListAnimator(null);
        attachPressAnimation(button);
        return button;
    }

    private Button secondaryButton(String text) {
        Button button = new Button(this);
        button.setText(text);
        button.setTextColor(TEXT);
        button.setTextSize(13f);
        button.setAllCaps(false);
        button.setBackground(rounded(Color.parseColor("#252b36"), 11));
        button.setStateListAnimator(null);
        attachPressAnimation(button);
        return button;
    }

    /** 布尔开关按钮:点击翻转并回写字段;`initial` 决定初始文案/高亮。 */
    private Button toggleButton(String onText, String offText, boolean initial,
                                java.util.function.Consumer<Boolean> setter) {
        Button button = secondaryButton(initial ? onText : offText);
        button.setTextColor(initial ? ACCENT : TEXT);
        final boolean[] state = { initial };
        button.setOnClickListener(v -> {
            state[0] = !state[0];
            setter.accept(state[0]);
            button.setText(state[0] ? onText : offText);
            button.setTextColor(state[0] ? ACCENT : TEXT);
        });
        return button;
    }

    private LinearLayout.LayoutParams buttonParams() {
        LinearLayout.LayoutParams params =
                new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(48));
        params.topMargin = dp(8);
        return params;
    }

    private GradientDrawable rounded(int color, int radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(radiusDp));
        return drawable;
    }

    private void attachPressAnimation(View view) {
        view.setOnTouchListener((v, event) -> {
            switch (event.getActionMasked()) {
                case android.view.MotionEvent.ACTION_DOWN:
                    v.animate().scaleX(0.97f).scaleY(0.97f).setDuration(90).start();
                    break;
                case android.view.MotionEvent.ACTION_UP:
                case android.view.MotionEvent.ACTION_CANCEL:
                    v.animate().scaleX(1f).scaleY(1f).setDuration(120).start();
                    break;
                default:
                    break;
            }
            return false;
        });
    }

    private int dp(int value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }
}
