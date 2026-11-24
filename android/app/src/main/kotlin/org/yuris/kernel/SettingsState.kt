package org.yuris.kernel

import android.content.SharedPreferences
import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.io.File

/**
 * 配置页的可变状态(Compose 观察)。
 *
 * <p>持久化在点「开始游戏」时统一写(prefs 键表见 `SetupActivity.persistSettings`);
 * 目录/字体等运行期选择只改这里,不每改一项就写盘。
 */
class SettingsState(prefs: SharedPreferences, incomingGameDir: String?) {
    var gameDir by mutableStateOf(
        incomingGameDir?.trim()?.takeIf { it.isNotEmpty() }
            ?: prefs.getString("game_dir", null),
    )
    var saveDir by mutableStateOf(prefs.getString("save_dir", null))
    var fontPath by mutableStateOf(prefs.getString("font_path", null))
    var importedFontName by mutableStateOf(fontPath?.let { File(it).name })
    var fontFamily by mutableStateOf(prefs.getString("font_family", "") ?: "")
    var mouseMode by mutableStateOf(prefs.getBoolean("launch_mouse_mode", false))
    var fps by mutableStateOf(
        prefs.getInt("fps", 60).let {
            if (it == 30 || it == 60 || it == 120) it else 60
        },
    )
    var backend by mutableStateOf("wgpu")
    var gpuBackend by mutableStateOf(prefs.getString("gpu_backend", "auto") ?: "auto")
    var patch by mutableStateOf(prefs.getString("patch", "auto") ?: "auto")
    var saveCodec by mutableStateOf(prefs.getString("save_codec", "") ?: "")
    var movieBackend by mutableStateOf(
        (prefs.getString("movie_backend", "auto") ?: "auto").let {
            if (it == "builtin" || it == "external") it else "auto"
        },
    )
    var debugLog by mutableStateOf(prefs.getBoolean("debug_log", false))
    var showHud by mutableStateOf(prefs.getBoolean("show_hud", false))
    var versionBadge by mutableStateOf(prefs.getBoolean("version_badge", true))
    var prefetch by mutableStateOf(prefs.getBoolean("prefetch", false))
    var snapshotDefer by mutableStateOf(prefs.getBoolean("snapshot_defer", false))
    var bilinearScale by mutableStateOf(prefs.getBoolean("bilinear_scale", true))
    var fontFallback by mutableStateOf(prefs.getBoolean("font_fallback", false))
    var vmProfile by mutableStateOf(prefs.getBoolean("vm_profile", false))
    var assetTiming by mutableStateOf(prefs.getBoolean("asset_timing", false))
    var tickSplit by mutableStateOf(prefs.getBoolean("tick_split", false))
    var cgTiming by mutableStateOf(prefs.getBoolean("cg_timing", false))

    /** 详情页头图的 exe 图标(异步加载;null = 没有)。 */
    var gameIcon by mutableStateOf<Bitmap?>(null)

    /** 运行时翻译文件(translate.json 绝对路径;null = 不加载)。 */
    var translatePath by mutableStateOf(
        prefs.getString("translate_path", null)?.takeIf { it.isNotEmpty() },
    )
    /** 翻译文件绑定的游戏根(避免 A 游戏的译文串到 B 游戏)。 */
    var translateGame by mutableStateOf(
        prefs.getString("translate_game", null)?.takeIf { it.isNotEmpty() },
    )

    /** 当前游戏真正生效的翻译文件(默认 null = 不加载)。 */
    fun activeTranslatePath(): String? =
        translatePath?.takeIf { translateGame != null && translateGame == gameDir }

    var systemFonts by mutableStateOf<List<String>>(emptyList())
    var fontsLoaded by mutableStateOf(false)
    var movieProbeText by mutableStateOf<String?>(null)
    var status by mutableStateOf("")
    var hasStorage by mutableStateOf(false)
    var pendingExternalLaunch by mutableStateOf(false)
    var externalSession by mutableStateOf(false)
}
