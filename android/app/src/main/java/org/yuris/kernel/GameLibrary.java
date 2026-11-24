package org.yuris.kernel;

import android.content.Context;
import android.graphics.Bitmap;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;

import org.json.JSONObject;

import java.io.File;
import java.io.FileOutputStream;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Set;

/**
 * 游戏库扫描:目录发现 + exe 图标提取 + 本地缓存。
 *
 * <p>判据不在这里猜:候选目录先做廉价过滤(有 exe/ypf/ysbin),再交给
 * {@link NativeHost#nativeProbeGameRoot(String)}(与内核启动同一份
 * {@code Project::discover});图标由内核的 PE 资源解析
 * ({@link NativeHost#nativeGameIcon(String)})提供。
 *
 * <p>全部在后台线程执行;结果回调到主线程。图标按
 * {@code 根路径哈希 + exe 最大 mtime} 缓存成 PNG,避免每次启动都读 exe。
 */
final class GameLibrary {
    private static final String TAG = "yu-library";
    private static final int MAX_DEPTH = 3;
    /** 进入目录后的最大深度(根 = 0)。 */
    private static final int THUMB_PX = 192;

    private static final String[] DEFAULT_ROOTS = {
            "Download", "Games", "Documents", "yu-ris", "Galgame", "galgame", "HGAME",
    };

    /** 一个已识别的游戏。 */
    static final class Game {
        final String root;
        final String title;
        /** `Project::discover` 的入口类型:packed / loose。 */
        final String kind;
        final int packs;
        /** `ysc.ybn` 容器头版本(0 = 未知)。 */
        final int version;
        /** 是否在官方 BETA 4 主线支持范围内(450..=495)。 */
        final boolean supported;
        final Bitmap icon;

        Game(String root, String title, String kind, int packs, int version,
                boolean supported, Bitmap icon) {
            this.root = root;
            this.title = title;
            this.kind = kind;
            this.packs = packs;
            this.version = version;
            this.supported = supported;
            this.icon = icon;
        }
    }

    /** `nativeProbeGameRoot` 的最小投影。 */
    private static final class Probe {
        final String kind;
        final int packs;
        final int version;
        final boolean supported;

        Probe(String kind, int packs, int version, boolean supported) {
            this.kind = kind;
            this.packs = packs;
            this.version = version;
            this.supported = supported;
        }
    }

    interface Callback {
        void onResult(List<Game> games, String error);
    }

    private GameLibrary() {
    }

    /**
     * 载入一个**已导入**的游戏根(probe + exe 图标)。
     *
     * <p>库页不再全盘扫描;每个根由用户通过 SAF 导入。返回 null = 目录已
     * 失效/不是数据入口。可在后台线程调用(会读 exe 提取图标)。
     */
    static Game load(Context context, String root) {
        File dir = new File(root);
        if (!dir.isDirectory()) {
            return null;
        }
        Probe probe = probeGameRoot(root);
        if (probe == null) {
            return null;
        }
        Log.i(TAG, "load root=" + root + " version=" + probe.version
                + " supported=" + probe.supported);
        File iconDir = new File(context.getFilesDir(), "gameicons");
        return makeGame(dir, root, probe, iconDir);
    }

    /** 后台扫描;{@code extraRoot} 可为 null。 */
    static void scan(Context context, String extraRoot, Callback callback) {
        Handler main = new Handler(Looper.getMainLooper());
        Context app = context.getApplicationContext();
        Thread worker = new Thread(() -> {
            List<Game> games = new ArrayList<>();
            String error = null;
            try {
                scanBlocking(app, extraRoot, games);
            } catch (Throwable failure) {
                Log.w(TAG, "扫描失败", failure);
                error = failure.getMessage() == null
                        ? failure.getClass().getSimpleName()
                        : failure.getMessage();
            }
            List<Game> result = games;
            String message = error;
            main.post(() -> callback.onResult(result, message));
        }, "yu-library-scan");
        worker.setDaemon(true);
        worker.start();
    }

    private static void scanBlocking(Context context, String extraRoot, List<Game> out) {
        File storage = Environment.getExternalStorageDirectory();
        List<File> roots = new ArrayList<>();
        for (String name : DEFAULT_ROOTS) {
            roots.add(new File(storage, name));
        }
        if (extraRoot != null && !extraRoot.trim().isEmpty()) {
            roots.add(new File(extraRoot.trim()));
        }

        Set<String> seen = new HashSet<>();
        File iconDir = new File(context.getFilesDir(), "gameicons");
        for (File root : roots) {
            if (!root.isDirectory() || !root.canRead()) {
                continue;
            }
            walk(root, 0, seen, iconDir, out);
        }
        out.sort((a, b) -> a.title.compareToIgnoreCase(b.title));
    }

    private static void walk(File dir, int depth, Set<String> seen, File iconDir, List<Game> out) {
        if (depth > MAX_DEPTH) {
            return;
        }
        String name = dir.getName();
        if (name.startsWith(".") || isBlacklisted(name)) {
            return;
        }
        String path;
        try {
            path = dir.getCanonicalPath();
        } catch (Exception ignored) {
            path = dir.getAbsolutePath();
        }
        if (!seen.add(path)) {
            return;
        }
        Probe probe = looksLikeGame(dir) ? probeGameRoot(path) : null;
        if (probe != null) {
            out.add(makeGame(dir, path, probe, iconDir));
            // 游戏根内部还有 ysbin/cg/save 等子目录;不再向下走,避免把
            // 资源目录误认成第二个游戏。
            return;
        }
        File[] children = dir.listFiles();
        if (children == null) {
            return;
        }
        Arrays.sort(children, (a, b) -> a.getName().compareToIgnoreCase(b.getName()));
        for (File child : children) {
            if (child.isDirectory()) {
                walk(child, depth + 1, seen, iconDir, out);
            }
        }
    }

    /** 只对看起来有游戏入口的目录调用 JNI,避免对整棵相册/下载树做探测。 */
    private static boolean looksLikeGame(File dir) {
        File[] children = dir.listFiles();
        if (children == null) {
            return false;
        }
        boolean hasBin = false;
        for (File child : children) {
            String name = child.getName().toLowerCase(Locale.ROOT);
            if (child.isDirectory()) {
                if (name.equals("ysbin") || name.equals("pac") || name.equals("data")) {
                    hasBin = true;
                }
                continue;
            }
            if (name.endsWith(".exe") || name.endsWith(".ypf")) {
                return true;
            }
        }
        return hasBin && hasExe(dir);
    }

    private static boolean hasExe(File dir) {
        File[] children = dir.listFiles();
        if (children == null) {
            return false;
        }
        for (File child : children) {
            if (child.isFile()
                    && child.getName().toLowerCase(Locale.ROOT).endsWith(".exe")) {
                return true;
            }
        }
        return false;
    }

    private static Probe probeGameRoot(String path) {
        try {
            String reply = NativeHost.nativeProbeGameRoot(path);
            if (reply == null || reply.isEmpty()) {
                return null;
            }
            JSONObject json = new JSONObject(reply);
            if (!json.optBoolean("ok", false)) {
                return null;
            }
            return new Probe(
                    json.optString("kind", ""),
                    json.optInt("packs", 0),
                    json.optInt("version", 0),
                    json.optBoolean("supported", false));
        } catch (Throwable failure) {
            Log.w(TAG, "probe 失败: " + path, failure);
            return null;
        }
    }

    private static Game makeGame(File dir, String path, Probe probe, File iconDir) {
        String title = dir.getName();
        if (title.isEmpty()) {
            title = path;
        }
        Bitmap icon = loadIcon(dir, path, iconDir);
        return new Game(path, title, probe.kind, probe.packs, probe.version,
                probe.supported, icon);
    }

    private static Bitmap loadIcon(File dir, String root, File iconDir) {
        String key = iconKey(dir, root);
        File cached = new File(iconDir, key + ".png");
        if (cached.isFile()) {
            Bitmap bitmap = android.graphics.BitmapFactory.decodeFile(cached.getAbsolutePath());
            if (bitmap != null) {
                return bitmap;
            }
        }
        int[] raw;
        try {
            raw = NativeHost.nativeGameIcon(root);
        } catch (Throwable failure) {
            Log.w(TAG, "读图标失败: " + root, failure);
            return null;
        }
        if (raw == null || raw.length < 2) {
            return null;
        }
        int width = raw[0];
        int height = raw[1];
        int count = width * height;
        if (width <= 0 || height <= 0 || raw.length < 2 + count) {
            return null;
        }
        Bitmap full;
        try {
            full = Bitmap.createBitmap(
                    Arrays.copyOfRange(raw, 2, 2 + count),
                    width,
                    height,
                    Bitmap.Config.ARGB_8888);
        } catch (Throwable failure) {
            Log.w(TAG, "图标位图创建失败: " + root, failure);
            return null;
        }
        Bitmap scaled = full;
        int max = Math.max(width, height);
        if (max > THUMB_PX) {
            float scale = THUMB_PX / (float) max;
            scaled = Bitmap.createScaledBitmap(
                    full,
                    Math.max(1, Math.round(width * scale)),
                    Math.max(1, Math.round(height * scale)),
                    true);
            if (scaled != full) {
                full.recycle();
            }
        }
        savePng(scaled, cached);
        return scaled;
    }

    /** 根路径 + 最大 exe mtime 作为缓存键:换 exe/YSIcon 改图标后自动失效。 */
    private static String iconKey(File dir, String root) {
        long newest = 0;
        File[] children = dir.listFiles();
        if (children != null) {
            for (File child : children) {
                if (child.isFile()
                        && child.getName().toLowerCase(Locale.ROOT).endsWith(".exe")) {
                    newest = Math.max(newest, child.lastModified());
                }
            }
        }
        return sha1(root + "|" + newest);
    }

    private static String sha1(String text) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-1");
            byte[] bytes = digest.digest(text.getBytes(java.nio.charset.StandardCharsets.UTF_8));
            StringBuilder out = new StringBuilder(bytes.length * 2);
            for (byte value : bytes) {
                out.append(String.format(Locale.ROOT, "%02x", value & 0xFF));
            }
            return out.toString();
        } catch (Exception failure) {
            return Integer.toHexString(text.hashCode());
        }
    }

    private static void savePng(Bitmap bitmap, File target) {
        File parent = target.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            return;
        }
        try (FileOutputStream out = new FileOutputStream(target)) {
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out);
        } catch (Exception failure) {
            Log.w(TAG, "图标缓存写入失败: " + target, failure);
        }
    }

    private static boolean isBlacklisted(String name) {
        switch (name) {
            case "Android":
            case "DCIM":
            case "Pictures":
            case "Music":
            case "Movies":
            case "Podcasts":
            case "Ringtones":
            case "Alarms":
            case "Notifications":
            case "save":
            case "ysbin":
            case "data":
            case "pac":
            case "cg":
            case "bgm":
            case "se":
            case "voice":
            case "movie":
            case "font":
            case "fonts":
                return true;
            default:
                return false;
        }
    }
}
