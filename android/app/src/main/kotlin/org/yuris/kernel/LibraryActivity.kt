package org.yuris.kernel

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import top.yukonga.miuix.kmp.basic.Button
import top.yukonga.miuix.kmp.basic.ButtonDefaults
import top.yukonga.miuix.kmp.basic.Icon
import top.yukonga.miuix.kmp.basic.IconButton
import top.yukonga.miuix.kmp.basic.Scaffold
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.theme.MiuixTheme
import java.io.File

/**
 * 游戏库:只展示**用户导入**的游戏根(不再全盘扫描)。
 *
 * <p>「导入游戏」= SAF 选目录 → 还原真实路径 → 内核 probe 判据确认是
 * YU-RIS 数据入口 → 记录 + 载入 exe 图标。点卡片进游戏详情/设置。
 */
class LibraryActivity : ComponentActivity() {
    private var games by mutableStateOf<List<GameLibrary.Game>>(emptyList())
    private var loading by mutableStateOf(false)
    private var error by mutableStateOf<String?>(null)
    private var needsPermission by mutableStateOf(false)
    private var lastRefreshAt = 0L
    private var pendingStorageAction: Runnable? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        needsPermission = !hasStoragePermission()
        setContent { YuTheme { LibraryScreen() } }
    }

    override fun onResume() {
        super.onResume()
        needsPermission = !hasStoragePermission()
        if (!needsPermission) {
            val next = pendingStorageAction
            if (next != null) {
                pendingStorageAction = null
                next.run()
            }
            if (System.currentTimeMillis() - lastRefreshAt > 2000) {
                refresh(force = false)
            }
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
        needsPermission = !granted
        if (!granted) {
            error = "没有存储权限:无法导入游戏目录"
            return
        }
        next?.run()
    }

    @androidx.compose.runtime.Composable
    private fun LibraryScreen() {
        val importPicker = rememberLauncherForActivityResult(
            ActivityResultContracts.OpenDocumentTree(),
        ) { uri -> uri?.let(::importGame) }
        Scaffold(
            topBar = {
                YuTopBar(
                    title = "游戏库",
                    subtitle = when {
                        needsPermission -> "需要文件权限"
                        loading -> "载入中…"
                        else -> "${games.size} 个游戏"
                    },
                    actions = {
                        IconButton(
                            onClick = {
                                if (needsPermission) {
                                    requestStorage()
                                } else {
                                    ensureStorageThen { importPicker.launch(null) }
                                }
                            },
                        ) {
                            Icon(
                                painter = painterResource(R.drawable.ic_add),
                                contentDescription = "导入游戏",
                            )
                        }
                    },
                )
            },
        ) { padding ->
            when {
                needsPermission -> PermissionPanel(
                    modifier = Modifier.fillMaxSize().padding(padding),
                    onGrant = { requestStorage() },
                )
                loading && games.isEmpty() -> CenterText(
                    text = "正在载入游戏…",
                    modifier = Modifier.fillMaxSize().padding(padding),
                )
                games.isEmpty() -> EmptyPanel(
                    error = error,
                    modifier = Modifier.fillMaxSize().padding(padding),
                    onImport = {
                        if (needsPermission) {
                            requestStorage()
                        } else {
                            ensureStorageThen { importPicker.launch(null) }
                        }
                    },
                )
                else -> GameGrid(
                    games = games,
                    modifier = Modifier.fillMaxSize().padding(padding),
                            onOpen = { game ->
                                if (!game.supported) {
                                    Toast.makeText(
                                        this,
                                        "YU-RIS ${versionText(game.version)} 不支持,无法打开",
                                        Toast.LENGTH_LONG,
                                    ).show()
                                } else {
                                    openSetup(game.root)
                                }
                            },
                )
            }
        }
    }

    private fun ensureStorageThen(next: () -> Unit) {
        if (hasStoragePermission()) {
            needsPermission = false
            next()
            return
        }
        pendingStorageAction = Runnable(next)
        requestStorage()
    }

    private fun requestStorage() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val intent = Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:$packageName"),
            )
            try {
                startActivity(intent)
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

    private fun importGame(uri: Uri) {
        val resolved = StoragePaths.resolveGameRoot(this, uri)
        val root = resolved.path
        if (root == null) {
            val message = resolved.error ?: "无法解析该目录的路径"
            error = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        val info = StoragePaths.probeInfo(File(root))
        if (info == null) {
            val message = "不是 YU-RIS 游戏:未找到 data.ypf 或 ysbin 目录"
            error = message
            android.util.Log.i("yu-library", "import root=$root not-yuris")
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        val version = info.optInt("version", 0)
        val supported = info.optBoolean("supported", false)
        android.util.Log.i(
            "yu-library",
            "import root=$root version=$version supported=$supported",
        )
        if (!supported) {
            val message = "YU-RIS ${versionText(version)} 不支持,已拒绝导入"
            error = message
            Toast.makeText(this, message, Toast.LENGTH_LONG).show()
            return
        }
        LibraryStore.add(this, root)
        error = null
        Toast.makeText(this, "已导入", Toast.LENGTH_SHORT).show()
        refresh(force = true)
    }

    private fun refresh(force: Boolean) {
        if (loading) {
            return
        }
        if (!force && System.currentTimeMillis() - lastRefreshAt < 2000) {
            return
        }
        val roots = LibraryStore.roots(this)
        if (roots.isEmpty()) {
            games = emptyList()
            error = null
            return
        }
        loading = true
        lastRefreshAt = System.currentTimeMillis()
        Thread({
            val loaded = ArrayList<GameLibrary.Game>()
            roots.forEach { root ->
                GameLibrary.load(this, root)?.let(loaded::add)
            }
            runOnUiThread {
                games = loaded
                loading = false
                error = if (loaded.isEmpty()) {
                    "导入的目录已失效或不是 YU-RIS 游戏根"
                } else {
                    null
                }
            }
        }, "yu-library-load").start()
    }

    private fun openSetup(gameRoot: String) {
        startActivity(
            Intent(this, SetupActivity::class.java)
                .putExtra(SetupActivity.EXTRA_GAME_DIR, gameRoot),
        )
    }

    private fun hasStoragePermission(): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager()
        }
        return ContextCompat.checkSelfPermission(
            this,
            Manifest.permission.READ_EXTERNAL_STORAGE,
        ) == PackageManager.PERMISSION_GRANTED
    }

    companion object {
        private const val REQUEST_STORAGE = 2001
    }
}

internal fun versionText(version: Int): String =
    if (version > 0) {
        String.format(java.util.Locale.ROOT, "%d.%03d", version / 1000, version % 1000)
    } else {
        "未知"
    }

@androidx.compose.runtime.Composable
private fun GameGrid(
    games: List<GameLibrary.Game>,
    modifier: Modifier = Modifier,
    onOpen: (GameLibrary.Game) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Fixed(4),
        modifier = modifier,
        contentPadding = PaddingValues(YuSpacing.gapL),
        horizontalArrangement = Arrangement.spacedBy(YuSpacing.gapL),
        verticalArrangement = Arrangement.spacedBy(YuSpacing.gapL),
    ) {
        items(games, key = { it.root }) { game ->
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onOpen(game) }
                    .padding(2.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                val icon = game.icon
                if (icon != null) {
                    Image(
                        bitmap = icon.asImageBitmap(),
                        contentDescription = game.title,
                        modifier = Modifier
                            .fillMaxWidth(0.86f)
                            .aspectRatio(1f)
                            .clip(RoundedCornerShape(16.dp)),
                    )
                } else {
                    Box(
                        modifier = Modifier
                            .fillMaxWidth(0.86f)
                            .aspectRatio(1f)
                            .clip(RoundedCornerShape(16.dp))
                            .background(MiuixTheme.colorScheme.surfaceVariant),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(
                            text = game.title.take(1),
                            style = MiuixTheme.textStyles.title3,
                        )
                    }
                }
                Spacer(Modifier.height(YuSpacing.gapS))
                Text(
                    text = game.title,
                    style = MiuixTheme.textStyles.footnote1,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@androidx.compose.runtime.Composable
private fun CenterText(text: String, modifier: Modifier = Modifier) {
    Box(modifier = modifier, contentAlignment = Alignment.Center) {
        Text(text)
    }
}

@androidx.compose.runtime.Composable
private fun PermissionPanel(modifier: Modifier = Modifier, onGrant: () -> Unit) {
    Column(
        modifier = modifier.padding(YuSpacing.screen),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("需要「所有文件访问」权限才能导入游戏", style = MiuixTheme.textStyles.title3)
        Spacer(Modifier.height(YuSpacing.gap))
        Text(
            "内核按真实路径读写游戏目录,不能使用 content:// URI",
            style = MiuixTheme.textStyles.body2,
            color = MiuixTheme.colorScheme.onBackgroundVariant,
        )
        Spacer(Modifier.height(YuSpacing.section))
        Button(
            onClick = onGrant,
            colors = ButtonDefaults.buttonColorsPrimary(),
        ) { Text("去授权") }
    }
}

@androidx.compose.runtime.Composable
private fun EmptyPanel(
    error: String?,
    modifier: Modifier = Modifier,
    onImport: () -> Unit,
) {
    Column(
        modifier = modifier.padding(YuSpacing.screen),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("还没有导入游戏", style = MiuixTheme.textStyles.title2)
        Spacer(Modifier.height(YuSpacing.gap))
        Text(
            error ?: "选择游戏根目录(含 data.ypf / ysbin 的目录)",
            style = MiuixTheme.textStyles.body2,
            color = MiuixTheme.colorScheme.onBackgroundVariant,
        )
        Spacer(Modifier.height(YuSpacing.section))
        Button(
            onClick = onImport,
            modifier = Modifier.width(220.dp),
            colors = ButtonDefaults.buttonColorsPrimary(),
        ) { Text("导入游戏") }
    }
}
