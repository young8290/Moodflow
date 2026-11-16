package com.moodflow.ime.logic

/** Editable model for the version 1 desktop/mobile interchange format. */
data class ChordDraft(
    val name: String,
    val left: List<String>,
    val right: List<String>,
    val intervalMs: Int,
    val mappings: List<Row>,
) {
    data class Row(val keys: List<String>, val output: String, val fragment: Boolean)
    data class Issue(val message: String, val row: Int? = null)

    /** Android touch constraints apply to drafts too, except that an empty draft can be saved. */
    fun issue(apply: Boolean): Issue? {
        if (name.isBlank() || name.length > 80 || name.any { it.isISOControl() })
            return Issue("方案名称须为 1–80 个字符，且不能包含控制字符。")
        if (intervalMs !in 50..2000) return Issue("物理键盘的两手间隔须为 50–2000 毫秒。")
        if (left.isEmpty() || right.isEmpty()) return Issue("左右手至少各分配一个键。")
        val hands = left + right
        if (hands.any { it !in SUPPORTED }) return Issue("触屏键位只能使用 a–z、逗号和句号。")
        if (hands.distinct().size != hands.size) return Issue("同一个键不能重复分配。")
        for ((side, keys) in listOf(left, right).withIndex()) {
            for ((i, grid) in GRID.withIndex()) {
                val max = if (side == 1 && i > 0) 4 else 5
                if (keys.count { ChordKeys.char(it).single() in grid } > max)
                    return Issue("${if (side == 0) "左" else "右"}手第 ${i + 1} 行最多放 $max 个键。")
            }
        }
        if (mappings.size > MAX_ROWS) return Issue("最多支持 $MAX_ROWS 条映射。")
        val seen = HashSet<Set<String>>()
        for ((i, row) in mappings.withIndex()) {
            val keys = row.keys.toSet()
            if (keys.size != row.keys.size || keys.size < 2 || keys.any { it !in hands } ||
                keys.count { it in left } > 2 || keys.count { it in right } > 2
            ) return Issue("组合须使用已分配且不同的键，共至少两个，每手最多两个。", i)
            if (!OUTPUT.matches(row.output)) return Issue("拼音只能包含 1–16 个小写字母或隔音符。", i)
            if (!seen.add(keys)) return Issue("这个组合已经有映射（按键顺序不影响组合）。", i)
        }
        if (apply && mappings.isEmpty()) return Issue("至少添加一条映射才能启用。")
        return null
    }

    fun matching(query: String): List<IndexedValue<Row>> {
        val needle = query.trim().lowercase()
        return mappings.withIndex().filter { (_, row) ->
            val keys = row.keys.joinToString("") { ChordKeys.char(it) }
            needle.isEmpty() || keys.contains(needle) || row.output.contains(needle) ||
                row.keys.joinToString(" ").contains(needle)
        }
    }

    companion object {
        const val MAX_ROWS = 1024
        const val MAX_BYTES = 256 * 1024
        private val GRID = listOf("qwertyuiop", "asdfghjkl", "zxcvbnm,.")
        private val SUPPORTED = ('a'..'z').map { it.toString() }.toSet() + setOf("comma", "period")
        private val OUTPUT = Regex("[a-z']{1,16}")

        @JvmStatic
        fun keys(text: String): List<String> =
            text.trim().split(Regex("\\s+")).filter(String::isNotEmpty).map {
                when (it.lowercase()) {
                    "," -> "comma"
                    "." -> "period"
                    else -> it.lowercase()
                }
            }
    }
}
