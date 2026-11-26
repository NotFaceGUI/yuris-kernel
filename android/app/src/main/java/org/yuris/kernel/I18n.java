package org.yuris.kernel;

import android.app.Activity;
import android.content.Context;
import android.content.ContextWrapper;
import android.content.SharedPreferences;

import org.json.JSONObject;

import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * 安卓壳的界面语言(只影响壳的菜单/设置/帮助,<b>不涉及游戏正文</b>)。
 *
 * <p>词条以中文原文为键,放在 {@code assets/i18n/<tag>.json};简体中文本身就是
 * 键,不另存翻译表。缺条目时先查英文、再回落中文原文,所以新增文案即使还没
 * 翻译也不会显示空串。带 {@code %s/%d} 的词条按 {@link String#format} 处理。
 *
 * <p>语言选择持久化在 {@code yu_kernel} 首选项:{@link #AUTO} = 跟随系统,
 * 其余是 {@link #SUPPORTED} 里的标签。切换后由调用方重建 Activity 让全部
 * 界面生效。
 *
 * <p><b>为什么是 Java</b>:轻量 Tyranor 运行时模块(:runtime)复用
 * {@code app/src/main/java} 的 MainActivity/DialogPresenter 但不编译 Kotlin;
 * 游戏页的启动文案也走这里,所以实现必须留在 Java 源集。
 */
public final class I18n {
    public static final String PREFS = "yu_kernel";
    public static final String KEY_LANGUAGE = "ui_language";

    /** 跟随系统。 */
    public static final String AUTO = "";

    /** 支持的语言标签;顺序就是弹窗顺序。zh-Hans 不加载翻译表。 */
    public static final List<String> SUPPORTED = Collections.unmodifiableList(Arrays.asList(
            "en",
            "zh-Hans",
            "zh-Hant",
            "ja",
            "ko",
            "es",
            "fr",
            "de",
            "pt-BR",
            "ru"));

    private static volatile String tag = "en";
    private static volatile Map<String, String> table = Collections.emptyMap();
    private static volatile Map<String, String> fallback = Collections.emptyMap();
    private static volatile boolean loaded = false;

    private I18n() {
    }

    /** 按首选项(或系统语言)加载当前语言的词条。每个 Activity 启动时调用。 */
    public static void init(Context context) {
        apply(context, storedTag(context));
    }

    /** 首选项里的原始选择({@code ""} = 跟随系统);非法值按跟随系统处理。 */
    public static String storedTag(Context context) {
        SharedPreferences prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        String stored = prefs.getString(KEY_LANGUAGE, AUTO);
        if (stored == null) {
            stored = AUTO;
        }
        return SUPPORTED.contains(stored) ? stored : AUTO;
    }

    /** 当前实际生效的语言标签(跟随系统时是识别结果)。 */
    public static String currentTag() {
        return tag;
    }

    /**
     * 写入语言选择并立即生效。
     *
     * <p>传入 Activity 时顺手 {@code recreate()}:Compose/Java 界面都会按新语言
     * 重新构建;非 Activity 上下文只更新词条。
     */
    public static void setLanguage(Context context, String languageTag) {
        String value = SUPPORTED.contains(languageTag) ? languageTag : AUTO;
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .edit()
                .putString(KEY_LANGUAGE, value)
                .apply();
        loaded = false;
        apply(context, value);
        Activity activity = findActivity(context);
        if (activity != null) {
            activity.recreate();
        }
    }

    /** 取词条;args 非空时按格式串展开。 */
    public static String t(String key, Object... args) {
        Map<String, String> current = table;
        String pattern = current.get(key);
        if (pattern == null) {
            pattern = fallback.get(key);
        }
        if (pattern == null) {
            pattern = key;
        }
        if (args.length == 0) {
            return pattern;
        }
        try {
            return String.format(Locale.ROOT, pattern, args);
        } catch (Throwable ignored) {
            return pattern;
        }
    }

    /** 弹窗里显示的语言名一律用母语(不跟随当前界面语言翻译)。 */
    public static String displayName(String languageTag) {
        if (languageTag == null) {
            return "";
        }
        switch (languageTag) {
            case AUTO:
                return "";
            case "en":
                return "English";
            case "zh-Hans":
                return "简体中文";
            case "zh-Hant":
                return "繁體中文";
            case "ja":
                return "日本語";
            case "ko":
                return "한국어";
            case "es":
                return "Español";
            case "fr":
                return "Français";
            case "de":
                return "Deutsch";
            case "pt-BR":
                return "Português (Brasil)";
            case "ru":
                return "Русский";
            default:
                return languageTag;
        }
    }

    // ------------------------------------------------------------ 内部

    /** Compose 的 LocalContext 可能是包着 Activity 的 ContextWrapper。 */
    private static Activity findActivity(Context context) {
        Context current = context;
        while (current instanceof ContextWrapper) {
            if (current instanceof Activity) {
                return (Activity) current;
            }
            current = ((ContextWrapper) current).getBaseContext();
        }
        return null;
    }

    private static void apply(Context context, String stored) {
        String resolved = SUPPORTED.contains(stored) ? stored : detectSystem();
        if (loaded && resolved.equals(tag)) {
            return;
        }
        table = "zh-Hans".equals(resolved)
                ? Collections.<String, String>emptyMap()
                : readAsset(context, resolved);
        // 中文(简/繁)缺失时直接回落中文原文键;其他语言先回落英文。
        fallback = resolved.startsWith("zh") || "en".equals(resolved)
                ? Collections.<String, String>emptyMap()
                : readAsset(context, "en");
        tag = resolved;
        loaded = true;
    }

    private static Map<String, String> readAsset(Context context, String languageTag) {
        String path = "i18n/" + languageTag + ".json";
        try (InputStream stream = context.getAssets().open(path);
             BufferedReader reader = new BufferedReader(
                     new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            StringBuilder text = new StringBuilder();
            char[] buffer = new char[4096];
            int read;
            while ((read = reader.read(buffer)) >= 0) {
                text.append(buffer, 0, read);
            }
            JSONObject json = new JSONObject(text.toString());
            Map<String, String> map = new HashMap<>(json.length());
            java.util.Iterator<String> keys = json.keys();
            while (keys.hasNext()) {
                String key = keys.next();
                map.put(key, json.optString(key));
            }
            return Collections.unmodifiableMap(map);
        } catch (Throwable ignored) {
            // 翻译表缺失/损坏不挡界面:回落中文原文。
            return Collections.emptyMap();
        }
    }

    /** 系统语言 → 支持的语言;中文按脚本/地区区分简繁。 */
    private static String detectSystem() {
        Locale locale = Locale.getDefault();
        String language = locale.getLanguage().toLowerCase(Locale.ROOT);
        switch (language) {
            case "zh":
                return isTraditionalChinese(locale) ? "zh-Hant" : "zh-Hans";
            case "en":
                return "en";
            case "ja":
                return "ja";
            case "ko":
                return "ko";
            case "es":
                return "es";
            case "fr":
                return "fr";
            case "de":
                return "de";
            case "pt":
                return "pt-BR";
            case "ru":
                return "ru";
            default:
                return "en";
        }
    }

    private static boolean isTraditionalChinese(Locale locale) {
        String script = locale.getScript();
        if (script != null && !script.isEmpty()) {
            return "Hant".equalsIgnoreCase(script);
        }
        switch (locale.getCountry().toUpperCase(Locale.ROOT)) {
            case "TW":
            case "HK":
            case "MO":
                return true;
            default:
                return false;
        }
    }
}
