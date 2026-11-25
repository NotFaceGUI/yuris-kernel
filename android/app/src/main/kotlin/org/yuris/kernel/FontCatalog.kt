package org.yuris.kernel

import android.util.Log

/** 系统字体族枚举(内核 fontdb;游戏详情页与全局设置页共用)。 */
object FontCatalog {
    fun load(): List<String> {
        val families = ArrayList<String>()
        try {
            NativeHost.nativeListFonts()?.split('\n')?.forEach { line ->
                line.trim().takeIf { it.isNotEmpty() }?.let(families::add)
            }
        } catch (error: Throwable) {
            Log.w("yu-android", "nativeListFonts failed", error)
        }
        return families
    }
}
