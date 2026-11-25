package org.yuris.kernel

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import top.yukonga.miuix.kmp.basic.Icon
import top.yukonga.miuix.kmp.basic.IconButton
import top.yukonga.miuix.kmp.basic.Scaffold
import top.yukonga.miuix.kmp.basic.Switch
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.theme.MiuixTheme
import java.io.File

/**
 * 全局默认设置(游戏库右上角齿轮)。
 *
 * <p>这里只放「对所有游戏生效」的默认值;游戏详情页的修改是**单游戏覆盖**,
 * 覆盖优先于本页。字体也在这里设置一次即可,不需要每个游戏重复导入。
 */
class GlobalSettingsActivity : ComponentActivity() {
    internal lateinit var state: GlobalSettingsState
    private var pendingStorageAction: Runnable? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        NativeHost.nativeInitAndroidContext(applicationContext)
        state = GlobalSettingsState(applicationContext)
        SettingsPrefs.migrateLegacy(
            getSharedPreferences(SettingsPrefs.PREFS, MODE_PRIVATE),
            applicationContext,
        )
        state.reload()
        setContent { YuTheme { GlobalSettingsScreen(this) } }
        Thread({
            val families = FontCatalog.load()
            runOnUiThread {
                state.systemFonts = families
                state.fontsLoaded = true
            }
        }, "yu-font-list-global").start()
    }

    override fun onResume() {
        super.onResume()
        val next = pendingStorageAction
        if (next != null && hasStorage()) {
            pendingStorageAction = null
            next.run()
        }
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
        if (!granted) {
            state.status = "没有存储权限:无法读取字体文件"
            return
        }
        next?.run()
    }

    fun ensureStorageThen(next: () -> Unit) {
        if (hasStorage()) {
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

    fun onFontPicked(uri: Uri) {
        val result = StoragePaths.resolveFile(this, uri)
        if (result.path == null) {
            val message = result.error ?: "无法解析该位置的路径"
            state.status = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
        } else {
            state.setFontPath(result.path)
            state.status = ""
        }
    }

    private fun hasStorage(): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return android.os.Environment.isExternalStorageManager()
        }
        return ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.READ_EXTERNAL_STORAGE,
        ) == PackageManager.PERMISSION_GRANTED
    }

    companion object {
        private const val REQUEST_STORAGE = 2002
    }
}

/** 全局默认的可变状态(改动即写盘;库页/配置页下次读取时生效)。 */
class GlobalSettingsState(context: Context) {
    private val prefs = context.getSharedPreferences(SettingsPrefs.PREFS, Context.MODE_PRIVATE)

    var settings by mutableStateOf(SettingsPrefs.global(prefs))
        private set
    var systemFonts by mutableStateOf<List<String>>(emptyList())
    var fontsLoaded by mutableStateOf(false)
    var status by mutableStateOf("")

    val importedFontName: String?
        get() = settings.fontPath?.let { File(it).name }

    fun reload() {
        settings = SettingsPrefs.global(prefs)
    }

    fun update(transform: (SettingsSnapshot) -> SettingsSnapshot) {
        settings = transform(settings)
        SettingsPrefs.writeGlobal(prefs, settings)
    }

    fun setFontPath(path: String) {
        update { it.copy(fontPath = path, fontFamily = File(path).name) }
    }

    fun setFontFamily(name: String) {
        update { it.copy(fontPath = null, fontFamily = name) }
    }

    fun reset() {
        SettingsPrefs.clearGlobal(prefs)
        reload()
        status = ""
    }
}

@Composable
private fun GlobalSettingsScreen(activity: GlobalSettingsActivity) {
    val state = activity.state
    val settings = state.settings
    var showFont by remember { mutableStateOf(false) }
    var showInput by remember { mutableStateOf(false) }
    var showGpu by remember { mutableStateOf(false) }
    var showFps by remember { mutableStateOf(false) }
    var showMovie by remember { mutableStateOf(false) }
    var showPatch by remember { mutableStateOf(false) }
    var showCodec by remember { mutableStateOf(false) }
    var advancedOpen by remember { mutableStateOf(false) }

    val fontPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri -> uri?.let(activity::onFontPicked) }

    Scaffold(
        topBar = {
            YuTopBar(
                title = "全局设置",
                subtitle = "对所有游戏生效;单个游戏的设置优先",
                leading = {
                    IconButton(onClick = { activity.finish() }) {
                        Icon(
                            painter = painterResource(R.drawable.ic_arrow_back),
                            contentDescription = "返回",
                        )
                    }
                },
            )
        },
    ) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize().padding(padding),
            contentPadding = PaddingValues(
                start = YuSpacing.screen,
                end = YuSpacing.screen,
                top = YuSpacing.gap,
                bottom = YuSpacing.section,
            ),
            verticalArrangement = Arrangement.spacedBy(YuSpacing.section),
        ) {
            item {
                SettingsGroup("默认字体") {
                    SettingRow(
                        label = "导入字体文件",
                        value = "设置一次,所有游戏默认沿用",
                        onClick = {
                            activity.ensureStorageThen {
                                fontPicker.launch(arrayOf("*/*"))
                            }
                        },
                        trailing = {
                            Icon(
                                painter = painterResource(R.drawable.ic_add),
                                contentDescription = null,
                                modifier = Modifier.size(18.dp),
                                tint = MiuixTheme.colorScheme.onBackgroundVariant,
                            )
                        },
                    )
                    SettingsDivider()
                    SettingRow(
                        label = "字体",
                        value = globalFontLabel(state),
                        onClick = { showFont = true },
                        trailing = {
                            Icon(
                                painter = painterResource(R.drawable.ic_chevron_right),
                                contentDescription = null,
                                modifier = Modifier.size(18.dp),
                                tint = MiuixTheme.colorScheme.onBackgroundVariant,
                            )
                        },
                    )
                    SettingsDivider()
                    Text(
                        "推荐导入等宽字体;单个游戏需要换字体时,在游戏详情页覆盖即可。",
                        style = MiuixTheme.textStyles.footnote1,
                        color = MiuixTheme.colorScheme.onBackgroundVariant,
                        modifier = Modifier.padding(YuSpacing.cardH),
                    )
                }
            }
            item {
                SettingsGroup("显示与输入") {
                    DropdownRow(
                        label = "操作方式",
                        value = if (settings.mouseMode) "虚拟光标" else "触摸",
                        onClick = { showInput = true },
                    )
                }
            }
            item {
                SettingsGroup("运行") {
                    SettingRow(
                        label = "渲染后端",
                        value = "GPU(安卓壳固定;CPU 不开放)",
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "GPU 接口",
                        value = gpuLabel(settings.gpuBackend),
                        onClick = { showGpu = true },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "帧率上限",
                        value = "${settings.fps} fps",
                        onClick = { showFps = true },
                    )
                    SettingsDivider()
                    SettingRow(
                        label = "高级设置",
                        value = if (advancedOpen) "诊断/实验开关" else "默认全关",
                        onClick = { advancedOpen = !advancedOpen },
                        trailing = {
                            Icon(
                                painter = painterResource(R.drawable.ic_expand_more),
                                contentDescription = null,
                                modifier = Modifier
                                    .size(20.dp)
                                    .rotate(if (advancedOpen) 180f else 0f),
                                tint = MiuixTheme.colorScheme.onBackgroundVariant,
                            )
                        },
                    )
                    if (advancedOpen) {
                        SettingsDivider()
                        SwitchRow("状态浮层", "帧率/计数", settings.showHud) {
                            state.update { s -> s.copy(showHud = it) }
                        }
                        SettingsDivider()
                        SwitchRow("版本名角标", "左下角引擎版本", settings.versionBadge) {
                            state.update { s -> s.copy(versionBadge = it) }
                        }
                        SettingsDivider()
                        SwitchRow("调试日志", "写游戏目录 debug/*.jsonl", settings.debugLog) {
                            state.update { s -> s.copy(debugLog = it) }
                        }
                        SettingsDivider()
                        SwitchRow("资产预取", "后台解码", settings.prefetch) {
                            state.update { s -> s.copy(prefetch = it) }
                        }
                        SettingsDivider()
                        SwitchRow("快照延迟", "菜单少拷贝", settings.snapshotDefer) {
                            state.update { s -> s.copy(snapshotDefer = it) }
                        }
                        SettingsDivider()
                        SwitchRow(
                            "SIZE 双线性",
                            "手机关闭时按官方最近邻",
                            settings.bilinearScale,
                        ) {
                            state.update { s -> s.copy(bilinearScale = it) }
                        }
                        SettingsDivider()
                        SwitchRow("缺字补全", "其它字体补字形", settings.fontFallback) {
                            state.update { s -> s.copy(fontFallback = it) }
                        }
                        SettingsDivider()
                        SwitchRow("VM 归因", null, settings.vmProfile) {
                            state.update { s -> s.copy(vmProfile = it) }
                        }
                        SettingsDivider()
                        SwitchRow("资产计时", null, settings.assetTiming) {
                            state.update { s -> s.copy(assetTiming = it) }
                        }
                        SettingsDivider()
                        SwitchRow("tick 分段", null, settings.tickSplit) {
                            state.update { s -> s.copy(tickSplit = it) }
                        }
                        SettingsDivider()
                        SwitchRow("CG 计时", null, settings.cgTiming) {
                            state.update { s -> s.copy(cgTiming = it) }
                        }
                    }
                }
            }
            item {
                SettingsGroup("影片与文本") {
                    DropdownRow(
                        label = "视频解码",
                        value = movieLabel(settings.movieBackend),
                        onClick = { showMovie = true },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "文本编码",
                        value = patchLabel(settings.patch),
                        onClick = { showPatch = true },
                    )
                }
            }
            item {
                SettingsGroup("存档") {
                    DropdownRow(
                        label = "存档压缩",
                        value = codecLabel(settings.saveCodec),
                        onClick = { showCodec = true },
                    )
                    SettingsDivider()
                    Text(
                        "只影响写盘;读侧始终按实际流分流。原版带 YSSnp.dll 的作品是 snappy。",
                        style = MiuixTheme.textStyles.footnote1,
                        color = MiuixTheme.colorScheme.onBackgroundVariant,
                        modifier = Modifier.padding(YuSpacing.cardH),
                    )
                }
            }
            item {
                Column {
                    if (state.status.isNotEmpty()) {
                        Text(
                            state.status,
                            style = MiuixTheme.textStyles.footnote1,
                            color = MiuixTheme.colorScheme.onBackgroundVariant,
                            modifier = Modifier.padding(horizontal = YuSpacing.gapS),
                        )
                    }
                    SettingsGroup {
                        SettingRow(
                            label = "恢复默认设置",
                            value = "只清全局默认;单游戏设置保留",
                            onClick = { state.reset() },
                            trailing = {
                                Icon(
                                    painter = painterResource(R.drawable.ic_refresh),
                                    contentDescription = null,
                                    modifier = Modifier.size(18.dp),
                                    tint = MiuixTheme.colorScheme.onBackgroundVariant,
                                )
                            },
                        )
                    }
                }
            }
        }
    }

    if (showFont) {
        val imported = state.importedFontName
        val options = buildList {
            state.systemFonts.forEach { add("f:$it" to it) }
            if (imported != null) {
                add("file" to imported)
            }
        }
        val selected = when {
            settings.fontPath != null -> "file"
            settings.fontFamily.isNotEmpty() -> "f:${settings.fontFamily}"
            else -> ""
        }
        ChoiceDialog(
            title = "默认字体",
            options = options,
            selected = selected,
            onDismiss = { showFont = false },
            onSelect = { value ->
                when {
                    value == "file" -> {
                        settings.fontPath?.let(state::setFontPath)
                    }
                    value.startsWith("f:") -> {
                        state.setFontFamily(value.substring(2))
                    }
                }
            },
            footerLabel = "导入自己的字体",
            onFooter = {
                showFont = false
                activity.ensureStorageThen { fontPicker.launch(arrayOf("*/*")) }
            },
        )
    }
    if (showInput) {
        ChoiceDialog(
            title = "操作方式",
            options = listOf("touch" to "触摸", "mouse" to "虚拟光标"),
            selected = if (settings.mouseMode) "mouse" else "touch",
            onDismiss = { showInput = false },
            onSelect = { state.update { s -> s.copy(mouseMode = it == "mouse") } },
        )
    }
    if (showGpu) {
        ChoiceDialog(
            title = "GPU 接口",
            options = listOf("auto" to "自动", "gles" to "GLES", "vulkan" to "Vulkan"),
            selected = settings.gpuBackend,
            onDismiss = { showGpu = false },
            onSelect = { state.update { s -> s.copy(gpuBackend = it) } },
        )
    }
    if (showFps) {
        ChoiceDialog(
            title = "帧率上限",
            options = listOf("30" to "30 fps", "60" to "60 fps", "120" to "120 fps"),
            selected = settings.fps.toString(),
            onDismiss = { showFps = false },
            onSelect = { value -> state.update { s -> s.copy(fps = value.toInt()) } },
        )
    }
    if (showMovie) {
        ChoiceDialog(
            title = "视频解码",
            options = listOf(
                "auto" to "自动(推荐)",
                "builtin" to "内置(MPEG-1)",
                "external" to "外置(设备 MediaCodec)",
            ),
            selected = settings.movieBackend,
            onDismiss = { showMovie = false },
            onSelect = { state.update { s -> s.copy(movieBackend = it) } },
        )
    }
    if (showPatch) {
        ChoiceDialog(
            title = "文本编码",
            options = listOf(
                "auto" to "自动(推荐)",
                "original" to "原版(CP932)",
                "gbk" to "简体(GBK)",
                "big5" to "繁体(Big5)",
                "tunnel" to "隧道(sjis_ext)",
            ),
            selected = settings.patch,
            onDismiss = { showPatch = false },
            onSelect = { state.update { s -> s.copy(patch = it) } },
        )
    }
    if (showCodec) {
        ChoiceDialog(
            title = "存档压缩",
            options = listOf("" to "自动判定", "zlib" to "zlib", "snappy" to "snappy"),
            selected = settings.saveCodec,
            onDismiss = { showCodec = false },
            onSelect = { state.update { s -> s.copy(saveCodec = it) } },
        )
    }
}

@Composable
private fun SwitchRow(
    label: String,
    hint: String?,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
) {
    SettingRow(
        label = label,
        value = hint,
        trailing = { Switch(checked, onCheckedChange = onChange) },
    )
}

private fun globalFontLabel(state: GlobalSettingsState): String {
    val settings = state.settings
    return when {
        settings.fontPath != null ->
            state.importedFontName ?: File(settings.fontPath).name
        settings.fontFamily.isNotEmpty() -> settings.fontFamily
        else -> "未选择(游戏详情页需先选字体)"
    }
}

private fun gpuLabel(value: String): String = when (value) {
    "gles" -> "GLES"
    "vulkan" -> "Vulkan"
    else -> "自动"
}

private fun patchLabel(value: String): String = when (value) {
    "original" -> "原版(CP932)"
    "gbk" -> "简体(GBK)"
    "big5" -> "繁体(Big5)"
    "tunnel" -> "隧道(sjis_ext)"
    else -> "自动(推荐)"
}

private fun movieLabel(value: String): String = when (value) {
    "builtin" -> "内置(MPEG-1)"
    "external" -> "外置(设备 MediaCodec)"
    else -> "自动(推荐)"
}

private fun codecLabel(value: String): String = when (value) {
    "zlib" -> "zlib"
    "snappy" -> "snappy"
    else -> "自动判定"
}
