package org.yuris.kernel;

import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
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
    private static final String PREFS = "yu_kernel";
    private static final int REQUEST_GAME_DIR = 1001;
    private static final int REQUEST_FONT = 1002;
    private static final int REQUEST_PLAY = 1003;

    private static final int BG = Color.parseColor("#0e1014");
    private static final int CARD = Color.parseColor("#1a1e26");
    private static final int ACCENT = Color.parseColor("#4a90d9");
    private static final int TEXT = Color.parseColor("#e8ecf2");
    private static final int MUTED = Color.parseColor("#8b93a3");

    private TextView gameLabel;
    private TextView saveLabel;
    private TextView fontLabel;
    private TextView status;
    private Spinner fontSpinner;
    private Button startButton;

    /** 游戏根目录(真实路径,内核直接读)。 */
    private String gameDir;
    /** 自定义存档目录;null = 游戏目录/save。 */
    private String saveDir;
    private String fontPath;
    /** 渲染后端:"soft"(CPU)/ "wgpu"(GPU)。 */
    private String backend = "soft";
    /** GPU 底层接口:"auto" / "gles" / "vulkan"。 */
    private String gpuBackend = "auto";
    /** 写调试日志到游戏目录(debug/*.jsonl)。 */
    private boolean debugLog;
    private List<String> systemFonts = new ArrayList<>();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        gameDir = prefs.getString("game_dir", null);
        saveDir = prefs.getString("save_dir", null);
        fontPath = prefs.getString("font_path", null);
        backend = prefs.getString("backend", "soft");
        gpuBackend = prefs.getString("gpu_backend", "auto");
        debugLog = prefs.getBoolean("debug_log", false);

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
        saveCustom.setOnClickListener(v -> editSaveDir());
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

        fontLabel = bodyText();
        fontLabel.setTextSize(12f);
        fontCard.addView(fontLabel);

        Button importFont = secondaryButton("导入字体文件(ttf / otf / ttc)");
        importFont.setOnClickListener(v -> pickFont());
        fontCard.addView(importFont, buttonParams());
        root.addView(fontCard, cardParams());

        // ---- 运行(后端 / 调试) ----
        LinearLayout runCard = card();
        runCard.addView(sectionTitle("运行"));

        LinearLayout backendRow = new LinearLayout(this);
        backendRow.setOrientation(LinearLayout.HORIZONTAL);
        Button cpuButton = secondaryButton("CPU");
        Button gpuButton = secondaryButton("GPU");
        cpuButton.setOnClickListener(v -> {
            backend = "soft";
            refreshRunButtons(cpuButton, gpuButton);
        });
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

        // GPU 底层接口三选一(GPU 模式下有意义)。Android 模拟器的 Vulkan
        // 驱动会在提交流程内崩溃,GLES 更稳;真机两者都可。
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

        Button debugToggle = secondaryButton("调试日志:关");
        debugToggle.setOnClickListener(v -> {
            debugLog = !debugLog;
            debugToggle.setText(debugLog ? "调试日志:开(写游戏目录)" : "调试日志:关");
            debugToggle.setTextColor(debugLog ? ACCENT : TEXT);
        });
        runCard.addView(debugToggle, buttonParams());
        if (debugLog) {
            debugToggle.setText("调试日志:开(写游戏目录)");
            debugToggle.setTextColor(ACCENT);
        }
        root.addView(runCard, cardParams());
        refreshRunButtons(cpuButton, gpuButton);

        // ---- 开始 ----
        startButton = primaryButton("开始游戏");
        startButton.setOnClickListener(v -> launchGame());
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
            refreshLabels();
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
        loadSystemFonts();
        playEntrance(root);
    }

    // ------------------------------------------------------------ 权限与目录

    /** 没有「所有文件访问」时先去申请;已授权才打开目录选择。 */
    private void ensureStorageThen(Runnable next) {
        if (Environment.isExternalStorageManager()) {
            next.run();
        } else {
            status.setText("需要「所有文件访问」权限才能直接读写游戏目录");
            try {
                Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
                intent.setData(Uri.parse("package:" + getPackageName()));
                startActivity(intent);
            } catch (Exception error) {
                startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
            }
        }
    }

    /**
     * 选游戏目录。
     *
     * <p>用「打开文档」而不是 SAF 目录树:后者只给 content:// URI,内核无法
     * 按路径读;这里要的是**真实路径**。用户从系统文件管理器里进到游戏目录,
     * 随便点一个文件,取它的父目录作为游戏根。
     */
    private void pickDirectory() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        startActivityForResult(intent, REQUEST_GAME_DIR);
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
            if (data != null) {
                String error = data.getStringExtra(MainActivity.EXTRA_START_ERROR);
                if (error != null && !error.isEmpty()) {
                    status.setText(error);
                    // 失败后自动切回 CPU,避免用户反复踩同一个坑。
                    backend = "soft";
                }
            }
            return;
        }
        if (resultCode != RESULT_OK || data == null || data.getData() == null) {
            return;
        }
        Uri uri = data.getData();
        String realPath = realPathOf(uri);
        if (realPath == null) {
            status.setText("无法解析该位置的路径,请从「内部存储 / 下载」里选");
            return;
        }
        File picked = new File(realPath);
        if (requestCode == REQUEST_GAME_DIR) {
            // 选中的是文件 → 用其父目录;若父目录不像游戏根,再看它自己。
            File candidate = picked.isDirectory() ? picked : picked.getParentFile();
            String root = findGameRoot(candidate);
            if (root == null) {
                status.setText("这个目录里没有 data.ypf / data/,请选游戏根目录");
                return;
            }
            gameDir = root;
            refreshLabels();
        } else if (requestCode == REQUEST_FONT) {
            fontPath = picked.getAbsolutePath();
            refreshLabels();
        }
    }

    /** 从选中位置向上找游戏根(含 data.ypf 或 data/ 的目录)。 */
    private String findGameRoot(File start) {
        File cursor = start;
        for (int depth = 0; cursor != null && depth < 4; depth++) {
            if (isGameRoot(cursor)) {
                return cursor.getAbsolutePath();
            }
            cursor = cursor.getParentFile();
        }
        return null;
    }

    /** 把 document URI 还原成真实文件路径(需要所有文件访问权限)。 */
    private String realPathOf(Uri uri) {
        if ("file".equals(uri.getScheme())) {
            return uri.getPath();
        }
        try (android.database.Cursor cursor = getContentResolver().query(uri,
                new String[]{"_data"}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                String path = cursor.getString(0);
                if (path != null && !path.isEmpty()) {
                    return path;
                }
            }
        } catch (Exception ignored) {
            // 部分 provider 不提供 _data,继续用下面的规则拼。
        }
        // downloads provider 的常见形态:content://.../document/primary:Download/yugame/x
        String docId = null;
        try {
            docId = android.provider.DocumentsContract.getDocumentId(uri);
        } catch (Exception ignored) {
            // 不是 DocumentsContract URI。
        }
        if (docId == null) {
            return null;
        }
        int colon = docId.indexOf(':');
        if (colon < 0) {
            return null;
        }
        String volume = docId.substring(0, colon);
        String relative = docId.substring(colon + 1);
        File base = "primary".equals(volume)
                ? Environment.getExternalStorageDirectory()
                : new File("/storage", volume);
        return new File(base, relative).getAbsolutePath();
    }

    private static boolean isGameRoot(File dir) {
        if (dir == null || !dir.isDirectory()) {
            return false;
        }
        File[] files = dir.listFiles();
        if (files == null) {
            return false;
        }
        for (File file : files) {
            String name = file.getName().toLowerCase();
            if (file.isFile() && name.endsWith(".ypf")) {
                return true;
            }
            if (file.isDirectory() && name.equals("data")) {
                return true;
            }
        }
        return false;
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

    private void refreshLabels() {
        if (gameLabel != null) {
            gameLabel.setText(gameDir == null ? "尚未选择" : gameDir);
            gameLabel.setTextColor(gameDir == null ? MUTED : TEXT);
        }
        if (saveLabel != null) {
            saveLabel.setText(saveDir == null ? "游戏目录/save" : saveDir);
            saveLabel.setTextColor(MUTED);
        }
        if (fontLabel != null) {
            fontLabel.setText(fontPath == null ? "" : new File(fontPath).getName());
        }
        if (startButton != null) {
            boolean ready = gameDir != null;
            startButton.setEnabled(ready);
            startButton.setAlpha(ready ? 1f : 0.45f);
        }
    }

    private void editSaveDir() {
        final android.widget.EditText input = new android.widget.EditText(this);
        input.setText(saveDir != null ? saveDir : "");
        input.setHint("留空 = 游戏目录/save");
        input.setSingleLine(true);
        new androidx.appcompat.app.AlertDialog.Builder(this)
                .setTitle("另指定存档目录")
                .setView(input)
                .setPositiveButton("确定", (dialog, which) -> {
                    String text = input.getText().toString().trim();
                    saveDir = text.isEmpty() ? null : text;
                    refreshLabels();
                })
                .setNegativeButton("取消", null)
                .setNeutralButton("恢复默认", (dialog, which) -> {
                    saveDir = null;
                    refreshLabels();
                })
                .show();
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
                ArrayAdapter<String> adapter = new ArrayAdapter<>(
                        this, android.R.layout.simple_spinner_dropdown_item, entries);
                fontSpinner.setAdapter(adapter);
                fontSpinner.setEnabled(!systemFonts.isEmpty());
                fontSpinner.setSelection(0);
            });
        }, "yu-font-list").start();
    }

    private void launchGame() {
        if (gameDir == null) {
            return;
        }
        String family = "";
        int position = fontSpinner.getSelectedItemPosition();
        if (position > 0 && position - 1 < systemFonts.size()) {
            family = systemFonts.get(position - 1);
        }
        String effectiveSave = saveDir != null ? saveDir : new File(gameDir, "save").getAbsolutePath();
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                .putString("game_dir", gameDir)
                .putString("save_dir", saveDir)
                .putString("font_family", family)
                .putString("backend", backend)
                .putString("gpu_backend", gpuBackend)
                .putBoolean("debug_log", debugLog)
                .apply();
        Intent intent = new Intent(this, MainActivity.class);
        intent.putExtra(MainActivity.EXTRA_GAME_DIR, gameDir);
        intent.putExtra(MainActivity.EXTRA_SAVE_DIR, effectiveSave);
        intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, family);
        intent.putExtra(MainActivity.EXTRA_BACKEND, backend);
        intent.putExtra(MainActivity.EXTRA_GPU_BACKEND, gpuBackend);
        intent.putExtra(MainActivity.EXTRA_DEBUG_LOG, debugLog);
        if (fontPath != null) {
            intent.putExtra(MainActivity.EXTRA_FONT_PATH, fontPath);
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
