package org.yuris.kernel

import android.content.Intent
import android.graphics.Bitmap
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import top.yukonga.miuix.kmp.basic.Button
import top.yukonga.miuix.kmp.basic.ButtonDefaults
import top.yukonga.miuix.kmp.basic.Card
import top.yukonga.miuix.kmp.basic.Icon
import top.yukonga.miuix.kmp.basic.IconButton
import top.yukonga.miuix.kmp.basic.Scaffold
import top.yukonga.miuix.kmp.basic.Switch
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.basic.TextField
import top.yukonga.miuix.kmp.theme.MiuixTheme
import java.io.File

/**
 * 游戏详情 + 设置同页:头图在上,设置分组在下,底部固定开始栏。
 * 枚举项 = 当前值 + chevron 打开选择列表;布尔项 = 开关。
 */
@Composable
fun SetupScreen(activity: SetupActivity) {
    val state = activity.state
    var showManualPath by remember { mutableStateOf(false) }
    var showFont by remember { mutableStateOf(false) }
    var showInput by remember { mutableStateOf(false) }
    var showGpu by remember { mutableStateOf(false) }
    var showBackend by remember { mutableStateOf(false) }
    var showFps by remember { mutableStateOf(false) }
    var showMovie by remember { mutableStateOf(false) }
    var showPatch by remember { mutableStateOf(false) }
    var showCodec by remember { mutableStateOf(false) }
    var advancedOpen by remember { mutableStateOf(false) }

    val gamePicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri -> uri?.let(activity::onGameDirPicked) }
    val savePicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri -> uri?.let(activity::onSaveDirPicked) }
    val fontPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri -> uri?.let(activity::onFontPicked) }
    val translatePicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri -> uri?.let(activity::onTranslatePicked) }
    val exportPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocumentTree(),
    ) { uri -> uri?.let(activity::onExportDirPicked) }

    LaunchedEffect(state.pendingExternalLaunch, state.fontsLoaded, state.hasStorage) {
        activity.maybeLaunchExternal()
    }
    LaunchedEffect(state.gameDir) {
        val dir = state.gameDir
        state.gameIcon = null
        if (dir != null) {
            state.gameIcon = withContext(Dispatchers.IO) { loadGameIcon(dir) }
        }
    }
    LaunchedEffect(Unit) {
        state.movieProbeText = "正在探测设备解码器…"
        state.movieProbeText = try {
            withContext(Dispatchers.IO) { MovieCodecProbe.summary() }
        } catch (error: Throwable) {
            "设备解码探测失败: ${error.message}"
        }
    }

    val fontReady = hasFont(state)
    Scaffold(
        topBar = {
            YuTopBar(
                title = "游戏详情",
                actions = {
                    IconButton(
                        onClick = {
                            activity.startActivity(
                                Intent(activity, GlobalSettingsActivity::class.java),
                            )
                        },
                    ) {
                        Icon(
                            painter = painterResource(R.drawable.ic_settings),
                            contentDescription = "全局设置",
                        )
                    }
                    IconButton(
                        onClick = {
                            activity.startActivity(
                                Intent(activity, HelpActivity::class.java),
                            )
                        },
                    ) {
                        Icon(
                            painter = painterResource(R.drawable.ic_help),
                            contentDescription = "参数说明",
                        )
                    }
                },
            )
        },
        bottomBar = {
            PrimaryBar(
                text = when {
                    state.gameDir == null -> "先选择游戏目录"
                    !fontReady -> "先选择字体"
                    else -> "开始游戏"
                },
                enabled = state.gameDir != null && fontReady,
                onClick = { activity.ensureStorageThen { activity.launchGame() } },
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
            item { Hero(state) }
            item {
                Text(
                    "本页设置只对当前游戏生效,未修改的项跟随全局默认;" +
                        "全局默认在游戏库右上角齿轮里编辑。",
                    style = MiuixTheme.textStyles.footnote1,
                    color = MiuixTheme.colorScheme.onBackgroundVariant,
                    modifier = Modifier.padding(horizontal = YuSpacing.gapS),
                )
            }
            item {
                SettingsGroup("游戏") {
                    SettingRow(
                        label = "游戏目录",
                        value = state.gameDir ?: "尚未选择",
                        onClick = { activity.ensureStorageThen { gamePicker.launch(null) } },
                        trailing = { Chevron() },
                    )
                    SettingsDivider()
                    SettingRow(
                        label = "手动输入路径",
                        value = "目录选择器不可用时使用",
                        onClick = { showManualPath = true },
                        trailing = { Chevron() },
                    )
                    SettingsDivider()
                    SettingRow(
                        label = "存档目录",
                        value = state.saveDir ?: "游戏目录/save",
                        onClick = { activity.ensureStorageThen { savePicker.launch(null) } },
                        trailing = { Chevron() },
                    )
                    if (state.saveDir != null) {
                        SettingsDivider()
                        SettingRow(
                            label = "恢复默认存档目录",
                            value = "游戏目录/save",
                            onClick = { state.saveDir = null },
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
            item {
                SettingsGroup("显示与输入") {
                    SettingRow(
                        label = "导入字体文件",
                        value = "推荐:导入自己的字体(等宽)",
                        onClick = {
                            activity.ensureStorageThen { fontPicker.launch(arrayOf("*/*")) }
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
                        value = fontLabel(state),
                        onClick = { showFont = true },
                        trailing = { Chevron() },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "操作方式",
                        value = if (state.mouseMode) "虚拟光标" else "触摸",
                        onClick = { showInput = true },
                    )
                }
            }
            item {
                SettingsGroup("运行") {
                    DropdownRow(
                        label = "渲染后端",
                        value = backendLabel(state.backend),
                        onClick = { showBackend = true },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "GPU 接口",
                        value = gpuLabel(state.gpuBackend),
                        onClick = { showGpu = true },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "帧率上限",
                        value = "${state.fps} fps",
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
                        SwitchRow("状态浮层", "帧率/计数", state.showHud) { state.showHud = it }
                        SettingsDivider()
                        SwitchRow("版本名角标", "左下角引擎版本", state.versionBadge) {
                            state.versionBadge = it
                        }
                        SettingsDivider()
                        SwitchRow("调试日志", "写游戏目录 debug/*.jsonl", state.debugLog) {
                            state.debugLog = it
                        }
                        SettingsDivider()
                        SwitchRow("资产预取", "后台解码", state.prefetch) { state.prefetch = it }
                        SettingsDivider()
                        SwitchRow("快照延迟", "菜单少拷贝", state.snapshotDefer) {
                            state.snapshotDefer = it
                        }
                        SettingsDivider()
                        SwitchRow("SIZE 双线性", "手机关闭时按官方最近邻", state.bilinearScale) {
                            state.bilinearScale = it
                        }
                        SettingsDivider()
                        SwitchRow("缺字补全", "其它字体补字形", state.fontFallback) {
                            state.fontFallback = it
                        }
                        SettingsDivider()
                        SwitchRow("VM 归因", null, state.vmProfile) { state.vmProfile = it }
                        SettingsDivider()
                        SwitchRow("资产计时", null, state.assetTiming) { state.assetTiming = it }
                        SettingsDivider()
                        SwitchRow("tick 分段", null, state.tickSplit) { state.tickSplit = it }
                        SettingsDivider()
                        SwitchRow("CG 计时", null, state.cgTiming) { state.cgTiming = it }
                    }
                }
            }
            item {
                SettingsGroup("影片与文本") {
                    DropdownRow(
                        label = "视频解码",
                        value = movieLabel(state.movieBackend),
                        onClick = { showMovie = true },
                    )
                    SettingsDivider()
                    DropdownRow(
                        label = "文本编码",
                        value = patchLabel(state.patch),
                        onClick = { showPatch = true },
                    )
                    SettingsDivider()
                    val activeTranslate = state.activeTranslatePath()
                    SettingRow(
                        label = "翻译文件",
                        value = when {
                            activeTranslate != null -> File(activeTranslate).name
                            else -> "未启用(内核默认不加载)"
                        },
                        onClick = {
                            activity.ensureStorageThen {
                                translatePicker.launch(arrayOf("*/*"))
                            }
                        },
                        trailing = { Chevron() },
                    )
                    if (activeTranslate != null) {
                        SettingsDivider()
                        SettingRow(
                            label = "清除翻译文件",
                            value = "只清除配置,不删除文件",
                            onClick = {
                                state.translatePath = null
                            },
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
                    SettingsDivider()
                    SettingRow(
                        label = "导出翻译文件",
                        value = "translate.json(角色/选项/正文)",
                        onClick = {
                            activity.ensureStorageThen { exportPicker.launch(null) }
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
                    val probe = if (state.movieBackend == "builtin") {
                        "内置:MPEG-1 program stream + MP2 音频;不依赖设备解码器。"
                    } else {
                        state.movieProbeText
                    }
                    if (!probe.isNullOrEmpty()) {
                        SettingsDivider()
                        Text(
                            probe,
                            style = MiuixTheme.textStyles.footnote1,
                            color = MiuixTheme.colorScheme.onBackgroundVariant,
                            modifier = Modifier.padding(YuSpacing.cardH),
                        )
                    }
                }
            }
            item {
                SettingsGroup("存档") {
                    DropdownRow(
                        label = "存档压缩",
                        value = codecLabel(state.saveCodec),
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
                SettingsGroup("库管理") {
                    SettingRow(
                        label = "从游戏库移除",
                        value = "只删导入记录,不会删除游戏文件",
                        onClick = { activity.removeFromLibrary() },
                        trailing = {
                            Icon(
                                painter = painterResource(R.drawable.ic_delete),
                                contentDescription = null,
                                modifier = Modifier.size(18.dp),
                                tint = MiuixTheme.colorScheme.error,
                            )
                        },
                    )
                }
            }
            item {
                Column(Modifier.fillMaxWidth()) {
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
                            label = "恢复全局默认",
                            value = "只清当前游戏的设置,不改全局默认",
                            onClick = { activity.resetSettings() },
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
        val imported = state.importedFontName ?: state.fontPath?.let { File(it).name }
        val options = buildList {
            state.systemFonts.forEach { add("f:$it" to it) }
            if (imported != null) {
                add("file" to imported)
            }
        }
        val selected = when {
            state.fontPath != null -> "file"
            state.fontFamily.isNotEmpty() -> "f:${state.fontFamily}"
            else -> ""
        }
        ChoiceDialog(
            title = "字体",
            options = options,
            selected = selected,
            onDismiss = { showFont = false },
            onSelect = { value ->
                when {
                    value == "file" -> {
                        state.fontFamily = imported ?: ""
                    }
                    value.startsWith("f:") -> {
                        state.fontPath = null
                        state.importedFontName = null
                        state.fontFamily = value.substring(2)
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
    if (showManualPath) {
        ManualPathDialog(
            initial = state.gameDir ?: "",
            onDismiss = { showManualPath = false },
            onConfirm = {
                activity.applyManualGameDir(it)
                showManualPath = false
            },
        )
    }
    if (showInput) {
        ChoiceDialog(
            title = "操作方式",
            options = listOf("touch" to "触摸", "mouse" to "虚拟光标"),
            selected = if (state.mouseMode) "mouse" else "touch",
            onDismiss = { showInput = false },
            onSelect = { state.mouseMode = it == "mouse" },
        )
    }
    if (showBackend) {
        ChoiceDialog(
            title = "渲染后端",
            options = listOf(
                "wgpu" to "GPU",
                "soft" to "CPU",
            ),
            selected = state.backend,
            disabled = setOf("soft"),
            onDismiss = { showBackend = false },
            onSelect = { state.backend = it },
        )
    }
    if (showGpu) {
        ChoiceDialog(
            title = "GPU 接口",
            options = listOf("auto" to "自动", "gles" to "GLES", "vulkan" to "Vulkan"),
            selected = state.gpuBackend,
            onDismiss = { showGpu = false },
            onSelect = { state.gpuBackend = it },
        )
    }
    if (showFps) {
        ChoiceDialog(
            title = "帧率上限",
            options = listOf("30" to "30 fps", "60" to "60 fps", "120" to "120 fps"),
            selected = state.fps.toString(),
            onDismiss = { showFps = false },
            onSelect = { state.fps = it.toInt() },
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
            selected = state.movieBackend,
            onDismiss = { showMovie = false },
            onSelect = { state.movieBackend = it },
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
            selected = state.patch,
            onDismiss = { showPatch = false },
            onSelect = { state.patch = it },
        )
    }
    if (showCodec) {
        ChoiceDialog(
            title = "存档压缩",
            options = listOf("" to "自动判定", "zlib" to "zlib", "snappy" to "snappy"),
            selected = state.saveCodec,
            onDismiss = { showCodec = false },
            onSelect = { state.saveCodec = it },
        )
    }
}

@Composable
private fun Hero(state: SettingsState) {
    val title = state.gameDir?.let { File(it).name } ?: "尚未选择游戏"
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(96.dp)
                .clip(RoundedCornerShape(26.dp))
                .background(MiuixTheme.colorScheme.surfaceVariant),
            contentAlignment = Alignment.Center,
        ) {
            val icon = state.gameIcon
            if (icon != null) {
                Image(
                    bitmap = icon.asImageBitmap(),
                    contentDescription = title,
                    modifier = Modifier.fillMaxSize(),
                )
            } else {
                Text(title.take(1), style = MiuixTheme.textStyles.headline1)
            }
        }
        Spacer(Modifier.width(YuSpacing.screen))
        Column(Modifier.weight(1f)) {
            Text(
                title,
                style = MiuixTheme.textStyles.title2,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                state.gameDir ?: "从游戏库导入或手动输入路径",
                style = MiuixTheme.textStyles.footnote1,
                color = MiuixTheme.colorScheme.onBackgroundVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            Spacer(Modifier.height(YuSpacing.gapS))
            Row(horizontalArrangement = Arrangement.spacedBy(YuSpacing.gapS)) {
                MetaChip("YU-RIS")
                if (state.gameDir != null) {
                    MetaChip("已选择")
                }
            }
        }
    }
}

@Composable
private fun Chevron() {
    Icon(
        painter = painterResource(R.drawable.ic_chevron_right),
        contentDescription = null,
        modifier = Modifier.size(18.dp),
        tint = MiuixTheme.colorScheme.onBackgroundVariant,
    )
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

@Composable
private fun ManualPathDialog(
    initial: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var text by remember { mutableStateOf(initial) }
    Dialog(onDismissRequest = onDismiss) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(YuSpacing.cardH)) {
                Text("游戏目录路径", style = MiuixTheme.textStyles.title3)
                Spacer(Modifier.height(YuSpacing.gap))
                TextField(
                    value = text,
                    onValueChange = { text = it },
                    label = "绝对路径",
                )
                Spacer(Modifier.height(YuSpacing.section))
                Button(
                    onClick = { onConfirm(text) },
                    modifier = Modifier.fillMaxWidth(),
                    colors = ButtonDefaults.buttonColorsPrimary(),
                ) { Text("确定") }
            }
        }
    }
}

private fun loadGameIcon(dir: String): Bitmap? {
    val raw = try {
        NativeHost.nativeGameIcon(dir)
    } catch (_: Throwable) {
        null
    } ?: return null
    if (raw.size < 2) {
        return null
    }
    val width = raw[0]
    val height = raw[1]
    if (width <= 0 || height <= 0 || raw.size < 2 + width * height) {
        return null
    }
    return try {
        Bitmap.createBitmap(
            raw.copyOfRange(2, 2 + width * height),
            width,
            height,
            Bitmap.Config.ARGB_8888,
        )
    } catch (_: Throwable) {
        null
    }
}

private fun fontLabel(state: SettingsState): String {
    val path = state.fontPath
    return when {
        path != null -> state.importedFontName ?: File(path).name
        state.fontFamily.isNotEmpty() -> state.fontFamily
        else -> "未选择(必须选择或导入)"
    }
}

private fun hasFont(state: SettingsState): Boolean =
    state.fontPath != null || state.fontFamily.isNotEmpty()

private fun backendLabel(value: String): String = when (value) {
    "soft" -> "CPU"
    else -> "GPU"
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
