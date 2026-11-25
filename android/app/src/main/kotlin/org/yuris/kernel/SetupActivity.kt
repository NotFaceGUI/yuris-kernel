package org.yuris.kernel

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.util.Log
import android.widget.Toast
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.core.app.ActivityCompat
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject
import top.yukonga.miuix.kmp.basic.Button
import top.yukonga.miuix.kmp.basic.ButtonDefaults
import top.yukonga.miuix.kmp.basic.Card
import top.yukonga.miuix.kmp.basic.Scaffold
import top.yukonga.miuix.kmp.basic.Switch
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.basic.TextButton
import top.yukonga.miuix.kmp.basic.TextField
import top.yukonga.miuix.kmp.basic.TopAppBar
import top.yukonga.miuix.kmp.theme.MiuixTheme
import top.yukonga.miuix.kmp.theme.darkColorScheme
import top.yukonga.miuix.kmp.theme.defaultTextStyles
import top.yukonga.miuix.kmp.theme.lightColorScheme
import java.io.File

/**
 * 单游戏配置页(Miuix/Compose)。
 *
 * <p>目录/存档/字体/操作/运行/高级/视频/编码/Tyranor 入口与旧 Java 页
 * **行为一一对应**;路径解析抽到 [StoragePaths],启动 Intent 构造与
 * [LibraryActivity] 直启共用 [buildGameIntent] 的键表。
 */
class SetupActivity : AppCompatActivity() {
    companion object {
        private const val TAG = "yu-android"

        /** 首选项文件名(壳内共享:配置页/库页/游戏页都读写它)。 */
        const val PREFS = "yu_kernel"

        /** 游戏库点卡片时带过来的游戏根(真实路径)。 */
        const val EXTRA_GAME_DIR = "game_dir"

        private const val REQUEST_STORAGE = 1005

        private const val TYRANOR_ACTION_LAUNCH = "com.tyranor.runtime.action.LAUNCH"
        private const val TYRANOR_EXTRA_FOLDER_PATH =
            "com.tyranor.runtime.extra.FOLDER_PATH"
        private const val TYRANOR_EXTRA_GAME_TITLE =
            "com.tyranor.runtime.extra.GAME_TITLE"
        private const val TYRANOR_EXTRA_PROTOCOL_VERSION =
            "com.tyranor.runtime.extra.PROTOCOL_VERSION"
        private const val TYRANOR_EXTRA_SETTINGS =
            "com.tyranor.runtime.extra.SETTINGS"
        private const val TYRANOR_PROTOCOL_VERSION = 1

        /**
         * 游戏库直启:按「全局默认 + 该游戏覆盖」构造 [MainActivity] 的 Intent。
         *
         * <p>不经过配置页;存档目录、字体、翻译文件都取该游戏真正生效的值,
         * A 游戏的设置不会串到 B 游戏,也不会覆盖全局默认。
         */
        @JvmStatic
        fun buildGameIntent(context: Context, gameDir: String): Intent {
            val prefs = context.getSharedPreferences(PREFS, MODE_PRIVATE)
            SettingsPrefs.migrateLegacy(prefs, context.applicationContext)
            val settings = SettingsPrefs.resolve(prefs, context.applicationContext, gameDir)
            val intent = Intent(context, MainActivity::class.java)
            intent.putExtra(MainActivity.EXTRA_GAME_DIR, gameDir)
            intent.putExtra(
                MainActivity.EXTRA_SAVE_DIR,
                settings.saveDir ?: File(gameDir, "save").absolutePath,
            )
            intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, settings.fontFamily)
            settings.fontPath?.takeIf { it.isNotEmpty() }?.let {
                intent.putExtra(MainActivity.EXTRA_FONT_PATH, it)
            }
            intent.putExtra(
                MainActivity.EXTRA_BACKEND,
                if (settings.backend == "soft") "soft" else "wgpu",
            )
            intent.putExtra(MainActivity.EXTRA_GPU_BACKEND, settings.gpuBackend)
            intent.putExtra(MainActivity.EXTRA_MOVIE_BACKEND, settings.movieBackend)
            intent.putExtra(MainActivity.EXTRA_MOUSE_MODE, settings.mouseMode)
            intent.putExtra(MainActivity.EXTRA_FPS, settings.fps)
            intent.putExtra(MainActivity.EXTRA_DEBUG_LOG, settings.debugLog)
            intent.putExtra(MainActivity.EXTRA_SHOW_HUD, settings.showHud)
            intent.putExtra(MainActivity.EXTRA_VERSION_BADGE, settings.versionBadge)
            intent.putExtra(MainActivity.EXTRA_PREFETCH, settings.prefetch)
            intent.putExtra(MainActivity.EXTRA_SNAPSHOT_DEFER, settings.snapshotDefer)
            intent.putExtra(MainActivity.EXTRA_BILINEAR_SCALE, settings.bilinearScale)
            intent.putExtra(MainActivity.EXTRA_FONT_FALLBACK, settings.fontFallback)
            intent.putExtra(MainActivity.EXTRA_VM_PROFILE, settings.vmProfile)
            intent.putExtra(MainActivity.EXTRA_ASSET_TIMING, settings.assetTiming)
            intent.putExtra(MainActivity.EXTRA_TICK_SPLIT, settings.tickSplit)
            intent.putExtra(MainActivity.EXTRA_CG_TIMING, settings.cgTiming)
            intent.putExtra(MainActivity.EXTRA_PATCH, settings.patch)
            if (settings.saveCodec.isNotEmpty()) {
                intent.putExtra(MainActivity.EXTRA_SAVE_CODEC, settings.saveCodec)
            }
            settings.translatePath?.let {
                intent.putExtra(MainActivity.EXTRA_TRANSLATE, it)
            }
            return intent
        }
    }

    internal lateinit var state: SettingsState
    private lateinit var dialogPresenter: DialogPresenter
    private var pendingStorageAction: Runnable? = null

    private val gameLauncher =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            handleGameResult(result.data)
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        NativeHost.nativeInitAndroidContext(this)
        val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
        SettingsPrefs.migrateLegacy(prefs, applicationContext)
        val incoming = intent.getStringExtra(EXTRA_GAME_DIR)
        state = SettingsState(prefs, applicationContext, incoming)
        state.hasStorage = StoragePaths.hasStorageAccess(this)
        dialogPresenter = DialogPresenter(this)
        setContent { YuTheme { SetupScreen(this) } }
        loadFonts()
        handleExternalLaunch(intent)
    }

    override fun onPause() {
        // 切去全局设置/启动游戏都会先走到这里:单游戏覆盖在这里落盘,
        // 未修改的项保持「跟随全局」。外部会话由宿主配置,不写覆盖。
        if (!state.externalSession) {
            state.saveOverrides()
        }
        super.onPause()
    }

    override fun onResume() {
        super.onResume()
        state.hasStorage = StoragePaths.hasStorageAccess(this)
        // 从全局设置页返回时同步新的全局默认(当前游戏的覆盖仍然优先)。
        if (!state.externalSession && !state.pendingExternalLaunch) {
            state.reload()
        }
        val next = pendingStorageAction
        if (state.hasStorage && next != null) {
            pendingStorageAction = null
            next.run()
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleExternalLaunch(intent)
    }

    override fun onDestroy() {
        dialogPresenter.dismissAll()
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_STORAGE) {
            return
        }
        val next = pendingStorageAction
        pendingStorageAction = null
        val granted = grantResults.isNotEmpty() &&
            grantResults[0] == PackageManager.PERMISSION_GRANTED
        state.hasStorage = granted
        if (!granted) {
            state.status = "没有存储权限:内核无法按真实路径读写游戏目录"
            return
        }
        next?.run()
    }

    // ------------------------------------------------------------ 权限

    /** 访问真实路径前确保存储权限;授权后执行 [next]。 */
    fun ensureStorageThen(next: () -> Unit) {
        if (StoragePaths.hasStorageAccess(this)) {
            state.hasStorage = true
            next()
            return
        }
        pendingStorageAction = Runnable(next)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val request = Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:$packageName"),
            )
            try {
                startActivity(request)
            } catch (_: Exception) {
                startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
            }
        } else {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(
                    Manifest.permission.READ_EXTERNAL_STORAGE,
                    Manifest.permission.WRITE_EXTERNAL_STORAGE,
                ),
                REQUEST_STORAGE,
            )
        }
    }

    // ------------------------------------------------------------ 选择回调

    fun onGameDirPicked(uri: Uri) {
        val result = StoragePaths.resolveGameRoot(this, uri)
        if (result.path == null) {
            state.status = result.error ?: "无法解析该目录的路径"
        } else {
            applyGameDirIfSupported(result.path)
        }
    }

    fun onSaveDirPicked(uri: Uri) {
        val result = StoragePaths.resolveDirectory(this, uri)
        if (result.path == null) {
            state.status = result.error ?: "无法解析该目录的路径"
        } else {
            state.saveDir = result.path
            state.status = ""
        }
    }

    fun onFontPicked(uri: Uri) {
        val result = StoragePaths.resolveFile(this, uri)
        if (result.path == null) {
            state.status = result.error ?: "无法解析该位置的路径"
        } else {
            val file = File(result.path)
            state.fontPath = file.absolutePath
            state.importedFontName = file.name
            state.fontFamily = file.name
            state.status = ""
        }
    }

    fun applyManualGameDir(text: String) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) {
            state.loadForGame(null)
            return
        }
        val picked = File(trimmed)
        val dir = if (picked.isDirectory) picked else picked.parentFile
        val next = dir?.let { StoragePaths.findGameRoot(it) } ?: picked.absolutePath
        applyGameDirIfSupported(next)
    }

    private var pendingGameDir: String? = null

    /**
     * 只接受内核 probe 认得的 YU-RIS 游戏根(不按版本号拒绝)。
     *
     * <p>不是 YU-RIS → 直接拒绝:不动 `state.gameDir`,只弹胶囊
     * 并把原因写进状态行。probe 是同步文件 IO(`Project::discover`),放后台
     * 线程;结果只认最后一次选择,避免连点两次时旧结果覆盖新结果。
     */
    private fun applyGameDirIfSupported(path: String) {
        pendingGameDir = path
        Thread({
            val info = StoragePaths.probeInfo(File(path))
            val version = info?.optInt("version", 0) ?: 0
            Log.i(TAG, "probe root=$path ok=${info != null} version=$version")
            val error = if (info == null) {
                "不是 YU-RIS 游戏:未找到 data.ypf 或 ysbin 目录"
            } else {
                null
            }
            runOnUiThread {
                if (pendingGameDir != path) {
                    return@runOnUiThread
                }
                pendingGameDir = null
                if (error != null) {
                    state.status = error
                    Toast.makeText(this, error, Toast.LENGTH_LONG).show()
                } else {
                    // 切换到该游戏自己的覆盖(未改项继续跟随全局默认)。
                    state.loadForGame(path)
                    state.status = ""
                }
            }
        }, "yu-probe-game-root").start()
    }

    /** 导入运行时翻译文件(translate.json;校验 schema 后按游戏根绑定)。 */
    fun onTranslatePicked(uri: Uri) {
        val gameDir = state.gameDir
        if (gameDir == null) {
            Toast.makeText(this, "先选择游戏目录", Toast.LENGTH_LONG).show()
            return
        }
        val result = StoragePaths.resolveFile(this, uri)
        if (result.path == null) {
            val message = result.error ?: "无法解析该位置的路径"
            state.status = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        val path = result.path
        Thread({
            val error = try {
                val json = JSONObject(File(path).readText())
                if (json.optString("schema") != "yuris-translate-1") {
                    "不是 yuris-translate-1 翻译文件"
                } else {
                    null
                }
            } catch (error: Exception) {
                "读取翻译文件失败: ${error.message}"
            }
            runOnUiThread {
                if (error != null) {
                    state.status = error
                    Toast.makeText(this, error, Toast.LENGTH_LONG).show()
                } else {
                    state.translatePath = path
                    state.status = ""
                    Toast.makeText(
                        this,
                        "已导入翻译文件: ${File(path).name}",
                        Toast.LENGTH_SHORT,
                    ).show()
                }
            }
        }, "yu-translate-import").start()
    }

    /** 导出 translate.json(角色/选项/正文)到所选目录;纯文件 IO 放后台。 */
    fun onExportDirPicked(uri: Uri) {
        val gameDir = state.gameDir
        if (gameDir == null) {
            Toast.makeText(this, "先选择游戏目录", Toast.LENGTH_LONG).show()
            return
        }
        val result = StoragePaths.resolveDirectory(this, uri)
        if (result.path == null) {
            val message = result.error ?: "无法解析该目录的路径"
            state.status = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        val outDir = result.path
        state.status = "正在导出翻译…"
        Toast.makeText(this, "正在导出翻译,可能需要几秒…", Toast.LENGTH_SHORT).show()
        Thread({
            val reply = NativeHost.nativeExportText(gameDir, outDir, "translate-json", "auto", "")
            Log.i(TAG, "export translate root=$gameDir out=$outDir reply=$reply")
            runOnUiThread {
                state.status = reply
                val message = if (reply.startsWith("OK:")) {
                    "翻译已导出到 $outDir"
                } else {
                    reply
                }
                Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            }
        }, "yu-translate-export").start()
    }

    // ------------------------------------------------------------ 启动 / 结果

    fun launchGame() {
        val gameDir = state.gameDir ?: return
        // 安卓壳不再提供系统默认字体;手动启动必须先选/导入字体。
        // Tyranor 外部会话由宿主决定配置,不在这里拦。
        if (!state.externalSession &&
            state.fontPath == null &&
            state.fontFamily.isEmpty()
        ) {
            val message = "开始游戏前必须选择或导入字体"
            state.status = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        state.pendingExternalLaunch = false
        // 单游戏覆盖落盘:只存与全局不同的项,换游戏互不影响。
        state.saveOverrides()
        val effectiveSave = state.saveDir
            ?: File(gameDir, "save").absolutePath
        val intent = Intent(this, MainActivity::class.java)
        intent.putExtra(MainActivity.EXTRA_GAME_DIR, gameDir)
        intent.putExtra(MainActivity.EXTRA_SAVE_DIR, effectiveSave)
        intent.putExtra(MainActivity.EXTRA_FONT_FAMILY, state.fontFamily)
        intent.putExtra(MainActivity.EXTRA_BACKEND, state.backend)
        intent.putExtra(MainActivity.EXTRA_GPU_BACKEND, state.gpuBackend)
        intent.putExtra(MainActivity.EXTRA_MOVIE_BACKEND, state.movieBackend)
        intent.putExtra(MainActivity.EXTRA_MOUSE_MODE, state.mouseMode)
        intent.putExtra(MainActivity.EXTRA_FPS, state.fps)
        intent.putExtra(MainActivity.EXTRA_DEBUG_LOG, state.debugLog)
        intent.putExtra(MainActivity.EXTRA_SHOW_HUD, state.showHud)
        intent.putExtra(MainActivity.EXTRA_VERSION_BADGE, state.versionBadge)
        intent.putExtra(MainActivity.EXTRA_PREFETCH, state.prefetch)
        intent.putExtra(MainActivity.EXTRA_SNAPSHOT_DEFER, state.snapshotDefer)
        intent.putExtra(MainActivity.EXTRA_BILINEAR_SCALE, state.bilinearScale)
        intent.putExtra(MainActivity.EXTRA_FONT_FALLBACK, state.fontFallback)
        intent.putExtra(MainActivity.EXTRA_VM_PROFILE, state.vmProfile)
        intent.putExtra(MainActivity.EXTRA_ASSET_TIMING, state.assetTiming)
        intent.putExtra(MainActivity.EXTRA_TICK_SPLIT, state.tickSplit)
        intent.putExtra(MainActivity.EXTRA_CG_TIMING, state.cgTiming)
        intent.putExtra(MainActivity.EXTRA_PATCH, state.patch)
        if (state.saveCodec.isNotEmpty()) {
            intent.putExtra(MainActivity.EXTRA_SAVE_CODEC, state.saveCodec)
        }
        state.activeTranslatePath()?.let {
            intent.putExtra(MainActivity.EXTRA_TRANSLATE, it)
        }
        state.fontPath?.takeIf { it.isNotEmpty() }?.let {
            intent.putExtra(MainActivity.EXTRA_FONT_PATH, it)
        }
        gameLauncher.launch(intent)
        overridePendingTransition(android.R.anim.fade_in, android.R.anim.fade_out)
    }

    private fun handleGameResult(data: Intent?) {
        val error = data?.getStringExtra(MainActivity.EXTRA_START_ERROR)
        if (!error.isNullOrEmpty()) {
            state.status = error
            // CPU(soft) 在安卓壳里不开放,不再自动降级;保持 GPU 重试。
            val dialogs = NativeHost.nativePendingDialogs()
            if (dialogs.isNotEmpty()) {
                dialogPresenter.publish(dialogs)
            }
            return
        }
        if (state.externalSession) {
            state.externalSession = false
            finish()
        }
    }

    /** 从游戏库移除当前游戏(只删导入记录,不动游戏文件与单游戏设置)。 */
    fun removeFromLibrary() {
        val dir = state.gameDir ?: return
        LibraryStore.remove(this, dir)
        finish()
    }

    /** 清除当前游戏的覆盖:全部回到全局默认(全局设置本身不动)。 */
    fun resetSettings() {
        state.resetToGlobal()
        state.status = ""
    }

    // ------------------------------------------------------------ 字体 / 探测

    private fun loadFonts() {
        Thread({
            val families = FontCatalog.load()
            runOnUiThread {
                state.systemFonts = families
                state.fontsLoaded = true
                maybeLaunchExternal()
            }
        }, "yu-font-list").start()
    }

    fun setMovieProbe(text: String) {
        state.movieProbeText = text
    }

    // ------------------------------------------------------------ Tyranor

    private fun handleExternalLaunch(intent: Intent?) {
        if (intent?.action != TYRANOR_ACTION_LAUNCH) {
            return
        }
        val protocol = intent.getIntExtra(
            TYRANOR_EXTRA_PROTOCOL_VERSION,
            TYRANOR_PROTOCOL_VERSION,
        )
        if (protocol != TYRANOR_PROTOCOL_VERSION) {
            state.status = "不支持的 Tyranor 模块协议版本: $protocol"
            return
        }
        val folder = intent.getStringExtra(TYRANOR_EXTRA_FOLDER_PATH)
        if (folder.isNullOrBlank()) {
            state.status = "Tyranor 未提供游戏目录"
            return
        }
        val dir = File(folder)
        if (!dir.isAbsolute || !dir.isDirectory) {
            state.status = "Tyranor 指定的游戏目录不存在: $folder"
            return
        }
        val nextGameDir = dir.absolutePath
        // 外部会话的配置由宿主(Tyranor 模块)提供:只继承全局默认,
        // 不载入该游戏的本地覆盖,也不把模块设置写回覆盖。
        state.loadGlobalOnly()
        state.gameDir = nextGameDir
        intent.getStringExtra(TYRANOR_EXTRA_GAME_TITLE)
            ?.takeIf { it.isNotEmpty() }
            ?.let { state.status = "来自 Tyranor: $it" }
        applyTyranorSettings(intent.getStringExtra(TYRANOR_EXTRA_SETTINGS))
        state.pendingExternalLaunch = true
        state.hasStorage = StoragePaths.hasStorageAccess(this)
        if (!state.hasStorage) {
            ensureStorageThen { state.hasStorage = true }
        }
    }

    private fun applyTyranorSettings(raw: String?) {
        if (raw.isNullOrEmpty()) {
            return
        }
        val payload = try {
            JSONObject(raw)
        } catch (_: Exception) {
            return
        }
        if (payload.optInt("version", -1) != 1) {
            return
        }
        val values = payload.optJSONObject("values") ?: return
        when (optString(values, "fps")) {
            "30" -> state.fps = 30
            "60" -> state.fps = 60
            "120" -> state.fps = 120
        }
        if (values.opt("mouse_mode") is Boolean) {
            state.mouseMode = values.optBoolean("mouse_mode", state.mouseMode)
        }
        if (values.opt("bilinear_scale") is Boolean) {
            state.bilinearScale = values.optBoolean("bilinear_scale", state.bilinearScale)
        }
        if (values.opt("font_fallback") is Boolean) {
            state.fontFallback = values.optBoolean("font_fallback", state.fontFallback)
        }
        if (values.opt("debug_log") is Boolean) {
            state.debugLog = values.optBoolean("debug_log", state.debugLog)
        }
        if (values.opt("show_hud") is Boolean) {
            state.showHud = values.optBoolean("show_hud", state.showHud)
        }
        optString(values, "patch")?.takeIf {
            it == "auto" || it == "original" || it == "gbk" ||
                it == "big5" || it == "tunnel"
        }?.let { state.patch = it }
        optString(values, "gpu_backend")?.takeIf {
            it == "auto" || it == "gles" || it == "vulkan"
        }?.let { state.gpuBackend = it }
        optString(values, "movie_backend")?.takeIf {
            it == "auto" || it == "builtin" || it == "external"
        }?.let { state.movieBackend = it }
        optString(values, "font")?.let(::applyTyranorFont)
    }

    private fun applyTyranorFont(value: String) {
        val trimmed = value.trim()
        if (trimmed.isEmpty()) {
            // 宿主没有 per-game 覆盖:保留模块自己的字体选择。
            return
        }
        if (trimmed.endsWith(".ttf") || trimmed.endsWith(".otf") ||
            trimmed.endsWith(".ttc") || trimmed.contains('/') ||
            trimmed.contains('\\')
        ) {
            val file = File(trimmed)
            if (file.isFile) {
                state.fontPath = file.absolutePath
                state.importedFontName = file.name
                state.fontFamily = file.name
            } else {
                state.fontPath = null
                state.importedFontName = null
                state.fontFamily = ""
            }
        } else {
            state.fontFamily = trimmed
            state.fontPath = null
            state.importedFontName = null
        }
    }

    private fun optString(values: JSONObject, key: String): String? =
        values.opt(key) as? String

    fun maybeLaunchExternal() {
        if (!state.pendingExternalLaunch || !state.fontsLoaded || !state.hasStorage) {
            return
        }
        state.pendingExternalLaunch = false
        state.externalSession = true
        launchGame()
    }
}

