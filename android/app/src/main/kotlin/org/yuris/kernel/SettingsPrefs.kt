package org.yuris.kernel

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONObject

/**
 * 全局默认 / 单游戏覆盖的两层配置口径。
 *
 * <p>层级:单游戏覆盖 > 全局默认 > 内置缺省。游戏详情页只写**当前游戏**
 * 的覆盖(与全局相同的项自动丢弃 = 回到继承),换游戏互不影响;
 * 全局默认在游戏库右上角齿轮里编辑。
 *
 * <p>存储:
 * - 全局默认沿用 `yu_kernel` prefs 的历史键名(升级零迁移;旧配置自动成为
 *   全局默认);
 * - 单游戏覆盖写在独立的 `yu_kernel_games` prefs 里,prefs 键 = 游戏根,
 *   值 = 只含覆盖项的 JSON。字体按「path/family 成对」覆盖,不会出现
 *   「换系统字体后仍沿用旧文件」的半覆盖。
 */
data class SettingsSnapshot(
    val fontPath: String? = null,
    val fontFamily: String = "",
    val mouseMode: Boolean = false,
    val fps: Int = 60,
    val backend: String = "wgpu",
    val gpuBackend: String = "auto",
    val patch: String = "auto",
    val saveCodec: String = "",
    val movieBackend: String = "auto",
    val debugLog: Boolean = false,
    val showHud: Boolean = false,
    val versionBadge: Boolean = true,
    val prefetch: Boolean = false,
    val snapshotDefer: Boolean = false,
    val bilinearScale: Boolean = true,
    val fontFallback: Boolean = false,
    val vmProfile: Boolean = false,
    val assetTiming: Boolean = false,
    val tickSplit: Boolean = false,
    val cgTiming: Boolean = false,
    /** 只在单游戏层有意义(全局没有默认存档目录)。 */
    val saveDir: String? = null,
    /** 只在单游戏层有意义(翻译文件按游戏根绑定)。 */
    val translatePath: String? = null,
)

object SettingsPrefs {
    /** 全局键所在 prefs 文件(与壳的历史键表一致)。 */
    const val PREFS = SetupActivity.PREFS

    private const val GAMES_PREFS = "yu_kernel_games"

    private const val K_GAME_DIR = "game_dir"
    private const val K_SAVE_DIR = "save_dir"
    private const val K_TRANSLATE_PATH = "translate_path"
    private const val K_TRANSLATE_GAME = "translate_game"
    private const val K_MIGRATED = "settings_layered_migrated"

    private const val K_FONT_PATH = "font_path"
    private const val K_FONT_FAMILY = "font_family"
    private const val K_MOUSE = "launch_mouse_mode"
    private const val K_FPS = "fps"
    private const val K_BACKEND = "backend"
    private const val K_GPU = "gpu_backend"
    private const val K_PATCH = "patch"
    private const val K_CODEC = "save_codec"
    private const val K_MOVIE = "movie_backend"
    private const val K_DEBUG_LOG = "debug_log"
    private const val K_SHOW_HUD = "show_hud"
    private const val K_VERSION_BADGE = "version_badge"
    private const val K_PREFETCH = "prefetch"
    private const val K_SNAPSHOT_DEFER = "snapshot_defer"
    private const val K_BILINEAR = "bilinear_scale"
    private const val K_FONT_FALLBACK = "font_fallback"
    private const val K_VM_PROFILE = "vm_profile"
    private const val K_ASSET_TIMING = "asset_timing"
    private const val K_TICK_SPLIT = "tick_split"
    private const val K_CG_TIMING = "cg_timing"

    /** 读全局默认(缺省值 = 内置缺省)。 */
    fun global(prefs: SharedPreferences): SettingsSnapshot = SettingsSnapshot(
        fontPath = prefs.getString(K_FONT_PATH, null)?.takeIf { it.isNotEmpty() },
        fontFamily = prefs.getString(K_FONT_FAMILY, "") ?: "",
        mouseMode = prefs.getBoolean(K_MOUSE, false),
        fps = validateFps(prefs.getInt(K_FPS, 60)),
        backend = validateBackend(prefs.getString(K_BACKEND, "wgpu")),
        gpuBackend = validateGpu(prefs.getString(K_GPU, "auto")),
        patch = validatePatch(prefs.getString(K_PATCH, "auto")),
        saveCodec = validateCodec(prefs.getString(K_CODEC, "")),
        movieBackend = validateMovie(prefs.getString(K_MOVIE, "auto")),
        debugLog = prefs.getBoolean(K_DEBUG_LOG, false),
        showHud = prefs.getBoolean(K_SHOW_HUD, false),
        versionBadge = prefs.getBoolean(K_VERSION_BADGE, true),
        prefetch = prefs.getBoolean(K_PREFETCH, false),
        snapshotDefer = prefs.getBoolean(K_SNAPSHOT_DEFER, false),
        bilinearScale = prefs.getBoolean(K_BILINEAR, true),
        fontFallback = prefs.getBoolean(K_FONT_FALLBACK, false),
        vmProfile = prefs.getBoolean(K_VM_PROFILE, false),
        assetTiming = prefs.getBoolean(K_ASSET_TIMING, false),
        tickSplit = prefs.getBoolean(K_TICK_SPLIT, false),
        cgTiming = prefs.getBoolean(K_CG_TIMING, false),
    )

    /** 全局默认写盘(不动游戏库列表/目录/翻译等单游戏键)。 */
    fun writeGlobal(prefs: SharedPreferences, snapshot: SettingsSnapshot) {
        prefs.edit()
            .putString(K_FONT_PATH, snapshot.fontPath)
            .putString(K_FONT_FAMILY, snapshot.fontFamily)
            .putBoolean(K_MOUSE, snapshot.mouseMode)
            .putInt(K_FPS, snapshot.fps)
            .putString(K_BACKEND, snapshot.backend)
            .putString(K_GPU, snapshot.gpuBackend)
            .putString(K_PATCH, snapshot.patch)
            .putString(K_CODEC, snapshot.saveCodec)
            .putString(K_MOVIE, snapshot.movieBackend)
            .putBoolean(K_DEBUG_LOG, snapshot.debugLog)
            .putBoolean(K_SHOW_HUD, snapshot.showHud)
            .putBoolean(K_VERSION_BADGE, snapshot.versionBadge)
            .putBoolean(K_PREFETCH, snapshot.prefetch)
            .putBoolean(K_SNAPSHOT_DEFER, snapshot.snapshotDefer)
            .putBoolean(K_BILINEAR, snapshot.bilinearScale)
            .putBoolean(K_FONT_FALLBACK, snapshot.fontFallback)
            .putBoolean(K_VM_PROFILE, snapshot.vmProfile)
            .putBoolean(K_ASSET_TIMING, snapshot.assetTiming)
            .putBoolean(K_TICK_SPLIT, snapshot.tickSplit)
            .putBoolean(K_CG_TIMING, snapshot.cgTiming)
            .apply()
    }

    /** 单游戏覆盖 JSON(没有覆盖 = null)。 */
    fun gameOverrides(context: Context, root: String): JSONObject? =
        gamesPrefs(context).getString(root, null)?.let {
            try {
                JSONObject(it)
            } catch (_: Exception) {
                null
            }
        }

    /** 当前游戏真正生效的配置(全局 + 覆盖;含存档目录/翻译文件)。 */
    fun resolve(
        prefs: SharedPreferences,
        context: Context,
        root: String?,
    ): SettingsSnapshot {
        val global = global(prefs)
        if (root == null) {
            return global
        }
        val json = gameOverrides(context, root)
        var merged = merge(global, json)
        if (json == null || !json.has(K_SAVE_DIR)) {
            merged = merged.copy(saveDir = legacySaveDir(prefs, root))
        }
        if (json == null || !json.has(K_TRANSLATE_PATH)) {
            merged = merged.copy(translatePath = legacyTranslate(prefs, root))
        }
        return merged
    }

    /** 只存与全局不同的项;全同 = 删除该游戏的覆盖(完全继承)。 */
    fun saveGameOverrides(
        prefs: SharedPreferences,
        context: Context,
        root: String,
        current: SettingsSnapshot,
    ) {
        val global = global(prefs)
        val json = JSONObject()
        if (current.fontPath != global.fontPath ||
            current.fontFamily != global.fontFamily
        ) {
            json.put(K_FONT_PATH, current.fontPath ?: "")
            json.put(K_FONT_FAMILY, current.fontFamily)
        }
        if (current.mouseMode != global.mouseMode) json.put(K_MOUSE, current.mouseMode)
        if (current.fps != global.fps) json.put(K_FPS, current.fps)
        if (current.backend != global.backend) json.put(K_BACKEND, current.backend)
        if (current.gpuBackend != global.gpuBackend) json.put(K_GPU, current.gpuBackend)
        if (current.patch != global.patch) json.put(K_PATCH, current.patch)
        if (current.saveCodec != global.saveCodec) json.put(K_CODEC, current.saveCodec)
        if (current.movieBackend != global.movieBackend) {
            json.put(K_MOVIE, current.movieBackend)
        }
        if (current.debugLog != global.debugLog) json.put(K_DEBUG_LOG, current.debugLog)
        if (current.showHud != global.showHud) json.put(K_SHOW_HUD, current.showHud)
        if (current.versionBadge != global.versionBadge) {
            json.put(K_VERSION_BADGE, current.versionBadge)
        }
        if (current.prefetch != global.prefetch) json.put(K_PREFETCH, current.prefetch)
        if (current.snapshotDefer != global.snapshotDefer) {
            json.put(K_SNAPSHOT_DEFER, current.snapshotDefer)
        }
        if (current.bilinearScale != global.bilinearScale) {
            json.put(K_BILINEAR, current.bilinearScale)
        }
        if (current.fontFallback != global.fontFallback) {
            json.put(K_FONT_FALLBACK, current.fontFallback)
        }
        if (current.vmProfile != global.vmProfile) json.put(K_VM_PROFILE, current.vmProfile)
        if (current.assetTiming != global.assetTiming) {
            json.put(K_ASSET_TIMING, current.assetTiming)
        }
        if (current.tickSplit != global.tickSplit) json.put(K_TICK_SPLIT, current.tickSplit)
        if (current.cgTiming != global.cgTiming) json.put(K_CG_TIMING, current.cgTiming)
        current.saveDir?.let { json.put(K_SAVE_DIR, it) }
        current.translatePath?.let { json.put(K_TRANSLATE_PATH, it) }

        val editor = gamesPrefs(context).edit()
        if (json.length() == 0) {
            editor.remove(root)
        } else {
            editor.putString(root, json.toString())
        }
        editor.apply()
    }

    /** 清除单游戏覆盖(= 全部回到全局默认)。 */
    fun clearGameOverrides(context: Context, root: String) {
        gamesPrefs(context).edit().remove(root).apply()
    }

    /** 清除全局默认(游戏库列表不动)。 */
    fun clearGlobal(prefs: SharedPreferences) {
        prefs.edit()
            .remove(K_FONT_PATH)
            .remove(K_FONT_FAMILY)
            .remove(K_MOUSE)
            .remove(K_FPS)
            .remove(K_BACKEND)
            .remove(K_GPU)
            .remove(K_PATCH)
            .remove(K_CODEC)
            .remove(K_MOVIE)
            .remove(K_DEBUG_LOG)
            .remove(K_SHOW_HUD)
            .remove(K_VERSION_BADGE)
            .remove(K_PREFETCH)
            .remove(K_SNAPSHOT_DEFER)
            .remove(K_BILINEAR)
            .remove(K_FONT_FALLBACK)
            .remove(K_VM_PROFILE)
            .remove(K_ASSET_TIMING)
            .remove(K_TICK_SPLIT)
            .remove(K_CG_TIMING)
            .apply()
    }

    /**
     * 一次性迁移:旧的全局 `save_dir` / `translate_path` 其实是「上次打开
     * 的那个游戏」的单游戏值,新模型下要落到对应游戏的覆盖里,否则会被
     * 所有游戏当成全局默认继承。
     */
    fun migrateLegacy(prefs: SharedPreferences, context: Context) {
        if (prefs.getBoolean(K_MIGRATED, false)) {
            return
        }
        val saveRoot = prefs.getString(K_GAME_DIR, null)
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
        val legacySave = prefs.getString(K_SAVE_DIR, null)?.takeIf { it.isNotEmpty() }
        val legacyTranslate = prefs.getString(K_TRANSLATE_PATH, null)
            ?.takeIf { it.isNotEmpty() }
        val translateRoot = prefs.getString(K_TRANSLATE_GAME, null)
            ?.trim()
            ?.takeIf { it.isNotEmpty() }

        if (saveRoot != null && legacySave != null) {
            mergeOverrides(context, saveRoot) { it.put(K_SAVE_DIR, legacySave) }
        }
        if (translateRoot != null && legacyTranslate != null) {
            mergeOverrides(context, translateRoot) {
                it.put(K_TRANSLATE_PATH, legacyTranslate)
            }
        }
        prefs.edit()
            .putBoolean(K_MIGRATED, true)
            .remove(K_SAVE_DIR)
            .remove(K_TRANSLATE_PATH)
            .remove(K_TRANSLATE_GAME)
            .apply()
    }

    private fun mergeOverrides(
        context: Context,
        root: String,
        transform: (JSONObject) -> Unit,
    ) {
        val json = gameOverrides(context, root) ?: JSONObject()
        transform(json)
        gamesPrefs(context).edit().putString(root, json.toString()).apply()
    }

    /** 旧全局存档目录只对「当时配置的那个游戏根」有效。 */
    private fun legacySaveDir(prefs: SharedPreferences, root: String): String? {
        val configured = prefs.getString(K_GAME_DIR, null)
        return prefs.getString(K_SAVE_DIR, null)
            ?.takeIf { configured == root && it.isNotEmpty() }
    }

    /** 旧全局翻译文件只对导入时绑定的游戏根有效。 */
    private fun legacyTranslate(prefs: SharedPreferences, root: String): String? {
        val bound = prefs.getString(K_TRANSLATE_GAME, null)
        return prefs.getString(K_TRANSLATE_PATH, null)
            ?.takeIf { bound == root && it.isNotEmpty() }
    }

    private fun merge(base: SettingsSnapshot, json: JSONObject?): SettingsSnapshot {
        if (json == null) {
            return base
        }
        val hasFont = json.has(K_FONT_PATH) || json.has(K_FONT_FAMILY)
        return base.copy(
            fontPath = if (hasFont) {
                json.optString(K_FONT_PATH, "").takeIf { it.isNotEmpty() }
            } else {
                base.fontPath
            },
            fontFamily = if (hasFont) {
                json.optString(K_FONT_FAMILY, "")
            } else {
                base.fontFamily
            },
            mouseMode = bool(json, K_MOUSE, base.mouseMode),
            fps = if (json.has(K_FPS)) validateFps(json.optInt(K_FPS, base.fps)) else base.fps,
            backend = if (json.has(K_BACKEND)) {
                validateBackend(json.optString(K_BACKEND, base.backend))
            } else {
                base.backend
            },
            gpuBackend = if (json.has(K_GPU)) {
                validateGpu(json.optString(K_GPU, base.gpuBackend))
            } else {
                base.gpuBackend
            },
            patch = if (json.has(K_PATCH)) {
                validatePatch(json.optString(K_PATCH, base.patch))
            } else {
                base.patch
            },
            saveCodec = if (json.has(K_CODEC)) {
                validateCodec(json.optString(K_CODEC, base.saveCodec))
            } else {
                base.saveCodec
            },
            movieBackend = if (json.has(K_MOVIE)) {
                validateMovie(json.optString(K_MOVIE, base.movieBackend))
            } else {
                base.movieBackend
            },
            debugLog = bool(json, K_DEBUG_LOG, base.debugLog),
            showHud = bool(json, K_SHOW_HUD, base.showHud),
            versionBadge = bool(json, K_VERSION_BADGE, base.versionBadge),
            prefetch = bool(json, K_PREFETCH, base.prefetch),
            snapshotDefer = bool(json, K_SNAPSHOT_DEFER, base.snapshotDefer),
            bilinearScale = bool(json, K_BILINEAR, base.bilinearScale),
            fontFallback = bool(json, K_FONT_FALLBACK, base.fontFallback),
            vmProfile = bool(json, K_VM_PROFILE, base.vmProfile),
            assetTiming = bool(json, K_ASSET_TIMING, base.assetTiming),
            tickSplit = bool(json, K_TICK_SPLIT, base.tickSplit),
            cgTiming = bool(json, K_CG_TIMING, base.cgTiming),
            saveDir = if (json.has(K_SAVE_DIR)) {
                json.optString(K_SAVE_DIR, "").takeIf { it.isNotEmpty() }
            } else {
                base.saveDir
            },
            translatePath = if (json.has(K_TRANSLATE_PATH)) {
                json.optString(K_TRANSLATE_PATH, "").takeIf { it.isNotEmpty() }
            } else {
                base.translatePath
            },
        )
    }

    private fun bool(json: JSONObject, key: String, fallback: Boolean): Boolean =
        if (json.has(key)) json.optBoolean(key, fallback) else fallback

    private fun validateFps(value: Int): Int =
        if (value == 30 || value == 60 || value == 120) value else 60

    private fun validateBackend(value: String?): String =
        if (value == "soft") "soft" else "wgpu"

    private fun validateGpu(value: String?): String = when (value) {
        "gles", "vulkan" -> value
        else -> "auto"
    }

    private fun validatePatch(value: String?): String = when (value) {
        "original", "gbk", "big5", "tunnel" -> value
        "cp949", "uhc", "euc-kr", "euckr", "kr" -> "cp949"
        "cp1252", "windows-1252", "win1252", "es", "spanish" -> "cp1252"
        "utf8", "utf-8" -> "utf8"
        else -> "auto"
    }

    private fun validateCodec(value: String?): String = when (value) {
        "zlib", "snappy" -> value
        else -> ""
    }

    private fun validateMovie(value: String?): String = when (value) {
        "builtin", "external" -> value
        else -> "auto"
    }

    private fun gamesPrefs(context: Context): SharedPreferences =
        context.getSharedPreferences(GAMES_PREFS, Context.MODE_PRIVATE)
}
