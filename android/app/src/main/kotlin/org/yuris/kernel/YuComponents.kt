package org.yuris.kernel

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import top.yukonga.miuix.kmp.basic.Button
import top.yukonga.miuix.kmp.basic.ButtonDefaults
import top.yukonga.miuix.kmp.basic.Card
import top.yukonga.miuix.kmp.basic.Icon
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.basic.TextButton
import top.yukonga.miuix.kmp.theme.MiuixTheme

/**
 * 设置页统一组件。
 *
 * <p>设计语言:
 * - 分组 = 小标题 + 一张卡;
 * - 卡内是行;枚举值一律「当前值 + chevron」打开下拉选择,
 *   布尔值一律开关;不再用一排大胶囊按钮;
 * - 间距全部来自 [YuSpacing]。
 */

@Composable
fun SettingsGroup(
    title: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(Modifier.fillMaxWidth()) {
        if (title != null) {
            Text(
                title,
                style = MiuixTheme.textStyles.subtitle,
                color = MiuixTheme.colorScheme.onBackgroundVariant,
                modifier = Modifier.padding(start = YuSpacing.gapS, bottom = YuSpacing.gap),
            )
        }
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.fillMaxWidth()) {
                content()
            }
        }
    }
}

@Composable
fun SettingRow(
    label: String,
    value: String? = null,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val modifier = Modifier
        .fillMaxWidth()
        .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
        .padding(horizontal = YuSpacing.cardH, vertical = YuSpacing.rowV)
    Row(modifier = modifier, verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, style = MiuixTheme.textStyles.body1)
            if (!value.isNullOrEmpty()) {
                Text(
                    value,
                    style = MiuixTheme.textStyles.footnote1,
                    color = MiuixTheme.colorScheme.onBackgroundVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        when {
            trailing != null -> {
                Spacer(Modifier.width(YuSpacing.gapL))
                trailing()
            }
            actionLabel != null && onAction != null -> {
                Spacer(Modifier.width(YuSpacing.gapL))
                TextButton(actionLabel, onClick = onAction)
            }
        }
    }
}

/** 枚举项统一入口:当前值 + chevron,点击打开单选列表。 */
@Composable
fun DropdownRow(
    label: String,
    value: String,
    onClick: () -> Unit,
) {
    SettingRow(
        label = label,
        value = value,
        onClick = onClick,
        trailing = {
            Icon(
                painter = painterResource(R.drawable.ic_chevron_right),
                contentDescription = null,
                modifier = Modifier.size(18.dp),
                tint = MiuixTheme.colorScheme.onBackgroundVariant,
            )
        },
    )
}

@Composable
fun SettingsDivider() {
    Box(
        Modifier
            .fillMaxWidth()
            .padding(start = YuSpacing.cardH, end = YuSpacing.cardH)
            .height(1.dp)
            .background(MiuixTheme.colorScheme.onBackgroundVariant.copy(alpha = 0.06f)),
    )
}

/**
 * 单选/下拉弹窗(所有枚举项共用)。
 *
 * <p>行式列表 + 选中勾;底部可选一个动作行(如「导入字体文件」),
 * 不放文本按钮。点外部/返回即取消。
 */
@Composable
fun ChoiceDialog(
    title: String,
    options: List<Pair<String, String>>,
    selected: String,
    onDismiss: () -> Unit,
    onSelect: (String) -> Unit,
    footerLabel: String? = null,
    onFooter: (() -> Unit)? = null,
    disabled: Set<String> = emptySet(),
) {
    Dialog(onDismissRequest = onDismiss) {
        Card(Modifier.fillMaxWidth().heightIn(max = 520.dp)) {
            Column(Modifier.padding(vertical = YuSpacing.gap)) {
                Text(
                    title,
                    style = MiuixTheme.textStyles.title3,
                    modifier = Modifier.padding(horizontal = YuSpacing.cardH),
                )
                Spacer(Modifier.height(YuSpacing.gap))
                LazyColumn(Modifier.weight(1f, fill = false)) {
                    itemsIndexed(options) { index, (value, label) ->
                        val enabled = value !in disabled
                        Row(
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable(enabled = enabled) {
                                    onSelect(value)
                                    onDismiss()
                                }
                                .padding(
                                    horizontal = YuSpacing.cardH,
                                    vertical = YuSpacing.rowV,
                                ),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                label,
                                style = MiuixTheme.textStyles.body1,
                                color = if (enabled) {
                                    Color.Unspecified
                                } else {
                                    MiuixTheme.colorScheme.onBackgroundVariant
                                        .copy(alpha = 0.38f)
                                },
                                modifier = Modifier.weight(1f),
                            )
                            if (value == selected) {
                                Icon(
                                    painter = painterResource(R.drawable.ic_check),
                                    contentDescription = null,
                                    modifier = Modifier.size(18.dp),
                                    tint = if (enabled) {
                                        MiuixTheme.colorScheme.primary
                                    } else {
                                        MiuixTheme.colorScheme.onBackgroundVariant
                                            .copy(alpha = 0.38f)
                                    },
                                )
                            }
                        }
                        if (index != options.lastIndex) {
                            SettingsDivider()
                        }
                    }
                }
                if (footerLabel != null && onFooter != null) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable(onClick = onFooter)
                            .padding(
                                horizontal = YuSpacing.cardH,
                                vertical = YuSpacing.rowV,
                            ),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            footerLabel,
                            style = MiuixTheme.textStyles.body1,
                            color = MiuixTheme.colorScheme.primary,
                            modifier = Modifier.weight(1f),
                        )
                        Icon(
                            painter = painterResource(R.drawable.ic_add),
                            contentDescription = null,
                            modifier = Modifier.size(18.dp),
                            tint = MiuixTheme.colorScheme.primary,
                        )
                    }
                }
            }
        }
    }
}

/** 底部固定主操作栏(开始游戏)。 */
@Composable
fun PrimaryBar(text: String, enabled: Boolean, onClick: () -> Unit) {
    Box(
        Modifier
            .fillMaxWidth()
            .background(MiuixTheme.colorScheme.surface)
            .navigationBarsPadding()
            .padding(horizontal = YuSpacing.screen, vertical = YuSpacing.cardV),
    ) {
        Button(
            onClick = onClick,
            enabled = enabled,
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColorsPrimary(),
        ) {
            Text(text)
        }
    }
}

/**
 * 界面语言选择弹窗(游戏库右上角地球/全局设置共用)。
 *
 * <p>「跟随系统」= 清空首选项;其余按 [I18n.SUPPORTED] 顺序,语言名用母语
 * 显示。选择后 [I18n.setLanguage] 会重建当前 Activity,弹窗随旧界面销毁。
 */
@Composable
fun LanguageDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val options = buildList {
        add(I18n.AUTO to t("跟随系统"))
        I18n.SUPPORTED.forEach { languageTag ->
            add(languageTag to languageDisplayName(languageTag))
        }
    }
    ChoiceDialog(
        title = t("界面语言"),
        options = options,
        selected = I18n.storedTag(context),
        onDismiss = onDismiss,
        onSelect = { I18n.setLanguage(context, it) },
    )
}

/** 小号元信息 chip(封包/散盘/包数)。 */
@Composable
fun MetaChip(text: String) {
    Box(
        Modifier
            .clip(RoundedCornerShape(8.dp))
            .background(MiuixTheme.colorScheme.surfaceVariant)
            .padding(horizontal = 8.dp, vertical = 3.dp),
    ) {
        Text(
            text,
            style = MiuixTheme.textStyles.footnote2,
            color = MiuixTheme.colorScheme.onBackgroundVariant,
        )
    }
}
