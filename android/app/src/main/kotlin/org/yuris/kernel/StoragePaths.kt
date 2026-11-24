package org.yuris.kernel

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.DocumentsContract
import android.util.Log
import androidx.core.content.ContextCompat
import org.json.JSONObject
import java.io.File

/**
 * SAF/真实路径解析(配置页与库页共用)。
 *
 * <p>内核按真实文件路径读写,content:// URI 用不了;这里把目录/文件选择器
 * 的 tree/document URI 还原成绝对路径。判据与旧 SetupActivity 完全一致,
 * 只是从 1600 行 Java 壳里抽出来,Compose 配置页直接复用。
 */
object StoragePaths {
    private const val TAG = "yu-android"

    data class PathResult(val path: String?, val error: String?)

    /** 当前是否已拿到内核读写真实路径所需的存储权限。 */
    fun hasStorageAccess(context: Context): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            return Environment.isExternalStorageManager()
        }
        return ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.READ_EXTERNAL_STORAGE,
        ) == PackageManager.PERMISSION_GRANTED
    }

    /**
     * 选中的目录 → 游戏根。
     *
     * <p>选目录不是门槛:能还原真实路径就接受,自动向上/向下一层定位
     * 只是便利;认不出来就把用户选中的目录交内核判。
     */
    fun resolveGameRoot(context: Context, uri: Uri): PathResult {
        var picked = realPathOf(context, uri)?.let(::File)
        if (picked == null) {
            return PathResult(null, "无法解析该目录的路径,请换用「手动输入路径…」")
        }
        if (!picked.isDirectory) {
            picked = picked.parentFile
        }
        if (picked == null) {
            return PathResult(null, "无法解析该目录的路径,请换用「手动输入路径…」")
        }
        val root = findGameRoot(picked)
            ?: singleChildGameRoot(picked)?.absolutePath
            ?: picked.absolutePath
        return PathResult(root, null)
    }

    /** 选中的目录 → 存档目录(传文件时取父目录)。 */
    fun resolveDirectory(context: Context, uri: Uri): PathResult {
        val path = realPathOf(context, uri)
            ?: return PathResult(null, "无法解析该目录的路径")
        var picked = File(path)
        if (!picked.isDirectory) {
            picked = picked.parentFile ?: return PathResult(null, "无法解析该目录的路径")
        }
        return PathResult(picked.absolutePath, null)
    }

    /** 选中的文件 → 真实路径(导入字体)。 */
    fun resolveFile(context: Context, uri: Uri): PathResult {
        val path = realPathOf(context, uri)
            ?: return PathResult(null, "无法解析该位置的路径,请从「内部存储 / 下载」里选")
        return PathResult(path, null)
    }

    /** 从选中位置向上找能当数据入口的目录;找不到回 null。 */
    fun findGameRoot(start: File): String? {
        var cursor: File? = start
        for (depth in 0 until 4) {
            if (cursor == null) {
                break
            }
            val root = probeGameRoot(cursor)
            if (root != null) {
                return root
            }
            cursor = cursor.parentFile
        }
        return null
    }

    /** 直接子目录里只有**一个**能当数据入口时返回它;0 个或多个都回 null。 */
    fun singleChildGameRoot(dir: File): File? {
        val children = dir.listFiles() ?: return null
        var found: File? = null
        for (child in children) {
            if (child.isDirectory && probeGameRoot(child) != null) {
                if (found != null) {
                    return null
                }
                found = child
            }
        }
        return found
    }

    /** 内核判据(`Project::discover` 同一份代码);不是入口回 null。 */
    /** 内核 probe 的完整 JSON(ok/root/kind/packs/version/supported)。 */
    fun probeInfo(dir: File?): JSONObject? {
        if (dir == null) {
            return null
        }
        val reply = try {
            NativeHost.nativeProbeGameRoot(dir.absolutePath)
        } catch (error: Throwable) {
            Log.w(TAG, "nativeProbeGameRoot failed", error)
            return null
        }
        return try {
            val json = JSONObject(reply ?: "")
            if (json.optBoolean("ok", false)) json else null
        } catch (error: Exception) {
            Log.w(TAG, "nativeProbeGameRoot reply is not JSON: $reply", error)
            null
        }
    }

    fun probeGameRoot(dir: File?): String? {
        if (dir == null) {
            return null
        }
        val json = probeInfo(dir) ?: return null
        return json.optString("root", "").ifEmpty { dir.absolutePath }
    }

    /** document / tree URI → 真实路径(文件或目录)。 */
    fun realPathOf(context: Context, uri: Uri): String? {
        if ("file" == uri.scheme) {
            return uri.path
        }
        queryDataPath(context, uri)?.let { return it }
        val docId = try {
            documentIdOf(uri)
        } catch (_: Exception) {
            return null
        } ?: return null
        if (isTreeUri(uri)) {
            try {
                val document = DocumentsContract.buildDocumentUriUsingTree(uri, docId)
                queryDataPath(context, document)?.let { return it }
            } catch (_: Exception) {
                // provider 不支持 document 形态,继续按 docId 拼。
            }
        }
        if (docId.startsWith("raw:")) {
            return docId.substring(4)
        }
        if ("downloads" == docId) {
            return Environment.getExternalStoragePublicDirectory(
                Environment.DIRECTORY_DOWNLOADS,
            ).absolutePath
        }
        val colon = docId.indexOf(':')
        if (colon < 0) {
            return null
        }
        val volume = docId.substring(0, colon)
        var relative = docId.substring(colon + 1)
        if (relative.startsWith("raw:")) {
            return relative.substring(4)
        }
        if ("home" == volume) {
            return null
        }
        val base = if ("primary" == volume) {
            Environment.getExternalStorageDirectory()
        } else {
            File("/storage", volume)
        }
        if (relative.isEmpty()) {
            return if (base.isDirectory) base.absolutePath else null
        }
        if ("primary" != volume && volume.length < 2) {
            return null
        }
        val resolved = File(base, relative)
        return if (resolved.exists()) resolved.absolutePath else null
    }

    private fun documentIdOf(uri: Uri): String? {
        val segments = uri.pathSegments
        val tree = segments.size >= 2 && "tree" == segments[0]
        return if (tree) {
            DocumentsContract.getTreeDocumentId(uri)
        } else {
            DocumentsContract.getDocumentId(uri)
        }
    }

    private fun isTreeUri(uri: Uri): Boolean {
        val segments = uri.pathSegments
        return segments.size >= 2 && "tree" == segments[0]
    }

    private fun queryDataPath(context: Context, uri: Uri): String? {
        return try {
            context.contentResolver.query(uri, arrayOf("_data"), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    cursor.getString(0)?.takeIf { it.isNotEmpty() }
                } else {
                    null
                }
            }
        } catch (_: Exception) {
            null
        }
    }
}
