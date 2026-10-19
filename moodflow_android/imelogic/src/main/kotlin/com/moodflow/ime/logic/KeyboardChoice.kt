package com.moodflow.ime.logic

/**
 * 键盘布局与配色这两项宿主偏好的取值规则（XSIME `KeyboardSettings` 的取值部分）。
 *
 * 输入方案不在这里：它是核心配置 `schema.active`（经核心切换、核心落盘），宿主不另存一份。
 * 九宫格只配拼音——当前方案不是拼音时，九宫格偏好保留，但界面按全键盘显示（[visibleLetters]）。
 */
object KeyboardChoice {
    const val QWERTY = "qwerty"
    const val NINE_KEY = "nineKey"
    const val ORTHOGONAL = "orthogonal"
    const val SPLIT_ORTHOGONAL = "splitOrthogonal"
    val LAYOUTS = listOf(QWERTY, NINE_KEY, ORTHOGONAL, SPLIT_ORTHOGONAL)

    /** 九宫格与并击都只能接拼音方案。 */
    const val PINYIN_SCHEMA = "pinyin"

    fun layout(stored: String?): String = if (stored in LAYOUTS) stored!! else QWERTY

    fun isChord(layout: String): Boolean = layout == ORTHOGONAL || layout == SPLIT_ORTHOGONAL

    /**
     * 这一刻字母区该显示什么。九宫格只在拼音方案的中文态出现；并击面板在拼音方案下一直在，
     * 英文态在同一块面板上按普通单键打字母（XSIME 同款）。
     */
    fun visibleLetters(layout: String, schema: String, chinese: Boolean): String = when {
        layout == NINE_KEY && (schema != PINYIN_SCHEMA || !chinese) -> QWERTY
        isChord(layout) && schema != PINYIN_SCHEMA -> QWERTY
        else -> layout
    }
}
