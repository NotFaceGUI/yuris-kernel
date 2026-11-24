package org.yuris.kernel

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.theme.MiuixTheme

/**
 * 单行顶栏:标题在左、动作 icon 在右,垂直居中对齐。
 *
 * <p>不用 Miuix 的大标题 TopAppBar(会把 icon 和标题拆成两行)。
 */
@Composable
fun YuTopBar(
    title: String,
    subtitle: String? = null,
    leading: (@Composable () -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .heightIn(min = 56.dp)
            .padding(horizontal = YuSpacing.screen, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leading != null) {
            leading()
            Spacer(Modifier.width(YuSpacing.gapS))
        }
        Column(Modifier.weight(1f)) {
            Text(title, style = MiuixTheme.textStyles.title2)
            if (!subtitle.isNullOrEmpty()) {
                Text(
                    subtitle,
                    style = MiuixTheme.textStyles.footnote1,
                    color = MiuixTheme.colorScheme.onBackgroundVariant,
                )
            }
        }
        Row(
            horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = actions,
        )
    }
}
