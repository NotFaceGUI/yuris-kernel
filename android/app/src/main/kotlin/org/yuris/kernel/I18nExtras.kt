package org.yuris.kernel

/** Kotlin 侧短名入口;Java 侧直接用 [I18n.t]。 */
fun t(key: String, vararg args: Any?): String = I18n.t(key, *args)

/** 弹窗里显示的语言名(一律用母语)。 */
fun languageDisplayName(languageTag: String): String = I18n.displayName(languageTag)
