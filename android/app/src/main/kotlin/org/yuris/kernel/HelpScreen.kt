package org.yuris.kernel

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.clickable
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import top.yukonga.miuix.kmp.basic.Card
import top.yukonga.miuix.kmp.basic.Icon
import top.yukonga.miuix.kmp.basic.IconButton
import top.yukonga.miuix.kmp.basic.Scaffold
import top.yukonga.miuix.kmp.basic.Text
import top.yukonga.miuix.kmp.theme.MiuixTheme

/**
 * 参数说明页(游戏详情右上角问号)。
 *
 * <p>正文用最小的富文本标记:`**…**` 显示为半粗体;不引入 Markdown 依赖。
 */
private data class HelpSection(
    val title: String,
    val points: List<String>,
    val links: List<Pair<String, String>> = emptyList(),
)

private val HELP_SECTIONS = listOf(
    HelpSection(
        "全局设置与单游戏配置",
        listOf(
            "游戏库右上角**齿轮** = 全局默认:字体、渲染、帧率、文本编码等,对所有游戏生效。",
            "游戏详情页 = **单游戏配置**:只影响当前游戏;未修改的项继续跟随全局默认。",
            "同一项两边都设置时**单游戏配置优先**;详情页最下方「恢复全局默认」只清当前游戏的覆盖。",
            "字体只需在全局设置里配一次;个别游戏要换字体时,再在详情页覆盖。",
        ),
    ),
    HelpSection(
        "字体(必选)",
        listOf(
            "**必选**:开始游戏前必须导入字体文件,或在系统字体列表里选一个;未选择时开始按钮保持禁用。",
            "推荐**自己导入等宽字体**(Noto Sans Mono CJK、思源等宽、更纱黑体 Mono 等);系统字体列表只作备选,比例字体在固定字宽排版下容易错位。",
            "游戏自带字体不一定出现在系统字体列表里;找不到就导入 **.ttf / .otf / .ttc**。",
            "汉化或运行时翻译时,主字体要覆盖目标汉字;缺字可打开「高级设置 → 缺字补全」。",
            "字体默认在**游戏库右上角齿轮 → 全局设置**里配置一次,所有游戏沿用;单游戏可在详情页覆盖。",
        ),
    ),
    HelpSection(
        "渲染",
        listOf(
            "「渲染后端」:选择 **GPU**;CPU 置灰不可选。",
            "「GPU 接口」:自动优先 Vulkan,失败回退 GLES;出现**花屏、黑屏、驱动崩溃**时,手动切 GLES 或 Vulkan 对比。",
            "「帧率上限」:30 / 60 / 120;发热或耗电明显时降到 30 或 60。",
        ),
    ),
    HelpSection(
        "文本编码",
        listOf(
            "**自动(推荐)**:按脚本内容采样判断编码,原版与大多数汉化包直接可用。",
            "**原版(CP932)**:日文原版,强制按 Shift-JIS 解码。",
            "**简体(GBK)/ 繁体(Big5)**:汉化补丁整包是 GBK / Big5 时选;选错会乱码或丢字。",
            "**韩文(CP949)**:韩文化补丁或韩文本地化版整包文本是 CP949(UHC / EUC-KR 扩展)时选;日文原版与 GBK/Big5 补丁不要选。",
            "**西班牙语(CP1252)**:西班牙语等西欧本地化版整包文本是 CP1252(Windows-1252,单字节)时选;不支持假名/汉字等双字节字符。",
            "**UTF-8(本地化)**:2025 年后部分本地化发行版(如 Kagura 简中/英文)整包文本是 UTF-8 时选;自动识别通常能判定,乱码时再手动选。",
            "**隧道(sjis_ext)**:CP932 + 扩展隧道码的汉化路线,需要游戏根有 sjis_ext.bin;普通 GBK 补丁不要选。",
            "判断方法:看补丁目录是否带 sc_cn_ / sc_tw_ 前缀;拿不准就先用自动,乱码再按补丁说明切换。",
            "部分汉化版用**加壳 / 注入 DLL** 改写文本,内核读不到这类文本,进入游戏可能没有文字。",
            "遇到这种情况:从「影片与文本 → 导出翻译文件」导出 translate.json,按翻译说明译好,再从「翻译文件」导入。",
        ),
        links = listOf(
            "翻译说明(GitHub)" to
                "https://github.com/NotFaceGUI/yuris-kernel/blob/master/.agents/skills/yuris-translation/SKILL.md",
        ),
    ),
    HelpSection(
        "存档",
        listOf(
            "「存档目录」默认是**游戏目录/save**;自定义目录只对当前游戏生效,换游戏会回到该游戏自己的 save。",
            "「存档压缩」:自动判定(推荐)。zlib / snappy 只在需要与特定工具链对齐时手动指定;**读档始终按实际数据流识别**,选错不会导致旧档读不出来。",
            "读档失败先确认没有把不同版本的存档混在同一个目录;内核以与原版字节一致为验收目标。",
        ),
    ),
    HelpSection(
        "影片与文本 / 翻译",
        listOf(
            "「视频解码」:自动(推荐);内置只支持 MPEG-1 + MP2,设备解码器异常时可用;外置走 MediaCodec,编码兼容更广但受机型影响。",
            "「翻译文件」:导入 **translate.json** 才启用运行时替换,默认不加载;文件按游戏根绑定,换游戏不会串用。",
            "「导出翻译文件」生成带空译文槽的 translate.json(角色 / 选项 / 正文);翻译完从同一入口导入即可。",
        ),
    ),
    HelpSection(
        "操作方式",
        listOf(
            "**触摸**:直接点按,适合大多数作品。",
            "**虚拟光标**:模拟鼠标指针,适合需要悬停 / 右键,或点击区域很小的老游戏。",
        ),
    ),
    HelpSection(
        "高级设置",
        listOf(
            "「缺字补全」:主字体缺字时用系统其它字体补字形;翻译后出现方块字时打开。",
            "「SIZE 双线性」:手机放大时更平滑;关闭更接近官方最近邻的像素观感。",
            "状态浮层、调试日志、VM 归因、资产计时、tick 分段、CG 计时都是**排查用**,默认全关,不影响正常游玩。",
        ),
    ),
    HelpSection(
        "遇到问题先查这里",
        listOf(
            "开始按钮点不动:先看是否选了字体;按钮文案会提示缺游戏目录还是缺字体。",
            "启动失败:看游戏界面弹出的错误,再回本页按关键词调整。",
            "乱码 → 文本编码;花屏 / 黑屏 → GPU 接口;读档失败 → 存档目录与压缩。",
            "仍然解决不了就点下面的链接**提交 Issue**,附上游戏名、YU-RIS 版本和错误弹窗截图。",
        ),
        links = listOf(
            "提交 Issue(GitHub)" to
                "https://github.com/NotFaceGUI/yuris-kernel/issues/new",
        ),
    ),
)

@Composable
fun HelpScreen(onBack: () -> Unit) {
    BackHandler(onBack = onBack)
    val uriHandler = LocalUriHandler.current
    Scaffold(
        topBar = {
            YuTopBar(
                title = t("参数说明"),
                leading = {
                    IconButton(onClick = onBack) {
                        Icon(
                            painter = painterResource(R.drawable.ic_arrow_back),
                            contentDescription = t("返回"),
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
                Text(
                    richText(t("下面这些参数会直接影响启动、排版和存档;正文里的**加粗**是常见坑。")),
                    style = MiuixTheme.textStyles.body2,
                    color = MiuixTheme.colorScheme.onBackgroundVariant,
                    lineHeight = 21.sp,
                )
            }
            items(HELP_SECTIONS) { section ->
                Card(Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(YuSpacing.cardH),
                        verticalArrangement = Arrangement.spacedBy(YuSpacing.gap),
                    ) {
                        Text(t(section.title), style = MiuixTheme.textStyles.title3)
                        section.points.forEach { point ->
                            Row(Modifier.fillMaxWidth()) {
                                Text(
                                    "•",
                                    style = MiuixTheme.textStyles.body2,
                                    color = MiuixTheme.colorScheme.primary,
                                    modifier = Modifier.padding(end = 8.dp),
                                )
                                Text(
                                    richText(t(point)),
                                    style = MiuixTheme.textStyles.body2,
                                    color = MiuixTheme.colorScheme.onBackgroundVariant,
                                    lineHeight = 21.sp,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                        section.links.forEach { (label, url) ->
                            Text(
                                t(label),
                                style = MiuixTheme.textStyles.body2,
                                color = MiuixTheme.colorScheme.primary,
                                textDecoration = TextDecoration.Underline,
                                modifier = Modifier
                                    .clickable { uriHandler.openUri(url) }
                                    .padding(vertical = 2.dp),
                            )
                        }
                    }
                }
            }
        }
    }
}

/** 只认 `**加粗**` 的极简富文本。 */
private fun richText(text: String) = buildAnnotatedString {
    var index = 0
    while (index < text.length) {
        val start = text.indexOf("**", index)
        if (start < 0) {
            append(text.substring(index))
            break
        }
        append(text.substring(index, start))
        val end = text.indexOf("**", start + 2)
        if (end < 0) {
            append(text.substring(start))
            break
        }
        withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) {
            append(text.substring(start + 2, end))
        }
        index = end + 2
    }
}
