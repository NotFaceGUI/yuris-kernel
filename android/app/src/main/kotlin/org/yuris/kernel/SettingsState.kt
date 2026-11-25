package org.yuris.kernel

import android.content.Context
import android.content.SharedPreferences
import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.io.File

/**
 * 配置页的可变状态(Compose 观察)。
 *
 * <p>负载值 = 全局默认 + 当前游戏的覆盖(见 [SettingsPrefs]);页面上看到
 * 的就是**这个游戏真正会用的值**。改动只写当前游戏的覆盖,未改的项继续
 * 跟随全局默认(全局在游戏库右上角齿轮里编辑)。
 *
 * <p>持久化时机:配置页 `onPause` 与「开始游戏」前统一写覆盖;目录/字体等
 * 运行期选择只改这里,不每改一项就写盘。
 */
class SettingsState(
    private val prefs: SharedPreferences,
    private val context: Context,
    incomingGameDir: String?,
) {
    var gameDir by mutableStateOf(
        incomingGameDir?.trim()?.takeIf { it.isNotEmpty() },
    )
    var saveDir by mutableStateOf<String?>(null)
    var fontPath by mutableStateOf<String?>(null)
    var importedFontName by mutableStateOf<String?>(null)
    var fontFamily by mutableStateOf("")
    var mouseMode by mutableStateOf(false)
    var fps by mutableStateOf(60)
    var backend by mutableStateOf("wgpu")
    var gpuBackend by mutableStateOf("auto")
    var patch by mutableStateOf("auto")
    var saveCodec by mutableStateOf("")
    var movieBackend by mutableStateOf("auto")
    var debugLog by mutableStateOf(false)
    var showHud by mutableStateOf(false)
    var versionBadge by mutableStateOf(true)
    var prefetch by mutableStateOf(false)
    var snapshotDefer by mutableStateOf(false)
    var bilinearScale by mutableStateOf(true)
    var fontFallback by mutableStateOf(false)
    var vmProfile by mutableStateOf(false)
    var assetTiming by mutableStateOf(false)
    var tickSplit by mutableStateOf(false)
    var cgTiming by mutableStateOf(false)

    /** 详情页头图的 exe 图标(异步加载;null = 没有)。 */
    var gameIcon by mutableStateOf<Bitmap?>(null)

    /** 运行时翻译文件(translate.json 绝对路径;null = 不加载)。 */
    var translatePath by mutableStateOf<String?>(null)

    /** 当前游戏真正生效的翻译文件(默认 null = 不加载)。 */
    fun activeTranslatePath(): String? = translatePath

    var systemFonts by mutableStateOf<List<String>>(emptyList())
    var fontsLoaded by mutableStateOf(false)
    var movieProbeText by mutableStateOf<String?>(null)
    var status by mutableStateOf("")
    var hasStorage by mutableStateOf(false)
    var pendingExternalLaunch by mutableStateOf(false)
    var externalSession by mutableStateOf(false)

    init {
        reload()
    }

    /** 重新读全局默认,并叠加当前游戏的覆盖。 */
    fun reload() {
        apply(SettingsPrefs.resolve(prefs, context, gameDir))
    }

    /** 切到另一个游戏根:先把当前游戏的覆盖落盘,再载入目标游戏的覆盖。 */
    fun loadForGame(path: String?) {
        if (!externalSession) {
            saveOverrides()
        }
        gameDir = path
        reload()
    }

    /** 只载入全局默认(Tyranor 外部会话:配置由宿主提供)。 */
    fun loadGlobalOnly() {
        apply(SettingsPrefs.resolve(prefs, context, null))
    }

    /** 当前页面值 → 快照(含存档目录/翻译文件等单游戏键)。 */
    fun snapshot(): SettingsSnapshot = SettingsSnapshot(
        fontPath = fontPath,
        fontFamily = fontFamily,
        mouseMode = mouseMode,
        fps = fps,
        backend = backend,
        gpuBackend = gpuBackend,
        patch = patch,
        saveCodec = saveCodec,
        movieBackend = movieBackend,
        debugLog = debugLog,
        showHud = showHud,
        versionBadge = versionBadge,
        prefetch = prefetch,
        snapshotDefer = snapshotDefer,
        bilinearScale = bilinearScale,
        fontFallback = fontFallback,
        vmProfile = vmProfile,
        assetTiming = assetTiming,
        tickSplit = tickSplit,
        cgTiming = cgTiming,
        saveDir = saveDir,
        translatePath = translatePath,
    )

    /** 把当前游戏与全局不同的项写成覆盖(外部会话不落盘)。 */
    fun saveOverrides() {
        val root = gameDir ?: return
        if (externalSession) {
            return
        }
        SettingsPrefs.saveGameOverrides(prefs, context, root, snapshot())
    }

    /** 清除当前游戏的覆盖,全部回到全局默认。 */
    fun resetToGlobal() {
        val root = gameDir
        if (root != null) {
            SettingsPrefs.clearGameOverrides(context, root)
        }
        reload()
    }

    private fun apply(settings: SettingsSnapshot) {
        fontPath = settings.fontPath
        importedFontName = settings.fontPath?.let { File(it).name }
        fontFamily = settings.fontFamily
        mouseMode = settings.mouseMode
        fps = settings.fps
        backend = settings.backend
        gpuBackend = settings.gpuBackend
        patch = settings.patch
        saveCodec = settings.saveCodec
        movieBackend = settings.movieBackend
        debugLog = settings.debugLog
        showHud = settings.showHud
        versionBadge = settings.versionBadge
        prefetch = settings.prefetch
        snapshotDefer = settings.snapshotDefer
        bilinearScale = settings.bilinearScale
        fontFallback = settings.fontFallback
        vmProfile = settings.vmProfile
        assetTiming = settings.assetTiming
        tickSplit = settings.tickSplit
        cgTiming = settings.cgTiming
        saveDir = settings.saveDir
        translatePath = settings.translatePath
    }
}
