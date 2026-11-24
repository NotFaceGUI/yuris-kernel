package org.yuris.kernel

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.Composable
import top.yukonga.miuix.kmp.theme.MiuixTheme
import top.yukonga.miuix.kmp.theme.darkColorScheme
import top.yukonga.miuix.kmp.theme.defaultTextStyles
import top.yukonga.miuix.kmp.theme.lightColorScheme

/** 全壳统一主题:库页/配置页共用,跟随系统深浅色。 */
@Composable
fun YuTheme(content: @Composable () -> Unit) {
    val dark = isSystemInDarkTheme()
    MiuixTheme(
        colors = if (dark) darkColorScheme() else lightColorScheme(),
        textStyles = defaultTextStyles(),
        content = content,
    )
}
