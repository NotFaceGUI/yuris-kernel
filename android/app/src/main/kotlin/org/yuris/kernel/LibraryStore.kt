package org.yuris.kernel

import android.content.Context

/** 已导入的游戏根列表(有序;壳不再全盘扫描)。 */
object LibraryStore {
    private const val KEY = "library_roots"

    fun roots(context: Context): List<String> {
        val stored = readStored(context)
        if (stored.isNotEmpty()) {
            return stored
        }
        // 首次升级:把旧配置页选过的游戏目录迁进来,避免用户重新导入。
        val legacy = prefs(context).getString("game_dir", null)
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?: return emptyList()
        write(context, listOf(legacy))
        return listOf(legacy)
    }

    fun add(context: Context, root: String) {
        val trimmed = root.trim()
        if (trimmed.isEmpty()) {
            return
        }
        write(context, (readStored(context) + trimmed).distinct())
    }

    fun remove(context: Context, root: String) {
        write(context, readStored(context).filter { it != root })
    }

    private fun readStored(context: Context): List<String> =
        prefs(context).getString(KEY, "")
            ?.lineSequence()
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() }
            ?.toList()
            ?: emptyList()

    private fun write(context: Context, roots: List<String>) {
        prefs(context).edit()
            .putString(KEY, roots.joinToString("\n"))
            .apply()
    }

    private fun prefs(context: Context) =
        context.getSharedPreferences(SetupActivity.PREFS, Context.MODE_PRIVATE)
}
