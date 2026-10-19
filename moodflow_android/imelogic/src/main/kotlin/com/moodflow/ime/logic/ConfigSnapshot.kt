package com.moodflow.ime.logic

/**
 * 从核心的配置快照（`MobileCore::config_snapshot`，`toml::to_string` 的输出）里读几个标量。
 *
 * 只认宿主要显示的那几种形状：`[a.b]` 表头下的 `key = true|false|"串"|["x", "y"]`（数组可跨行）。
 * 不是通用 TOML 解析器——设置页只读 `clipboard.enabled`、`plugins.enabled` 这类简单键，
 * 为它们引一个 TOML 库不值得；读不到时调用方用默认值。
 */
class ConfigSnapshot(text: String) {
    private val values = HashMap<String, String>()

    init {
        var section = ""
        val lines = text.lines()
        var i = 0
        while (i < lines.size) {
            val line = lines[i].trim()
            i++
            if (line.isEmpty() || line.startsWith("#")) continue
            if (line.startsWith("[") && !line.startsWith("[[")) {
                section = line.removePrefix("[").removeSuffix("]").trim()
                continue
            }
            val eq = line.indexOf('=')
            if (eq <= 0) continue
            val key = line.substring(0, eq).trim().trim('"')
            var value = line.substring(eq + 1).trim()
            if (value.startsWith("[") && !value.endsWith("]")) {
                val sb = StringBuilder(value)
                while (i < lines.size) {
                    val more = lines[i].trim()
                    i++
                    sb.append(more)
                    if (more.endsWith("]")) break
                }
                value = sb.toString()
            }
            values[if (section.isEmpty()) key else "$section.$key"] = value
        }
    }

    fun bool(key: String): Boolean? = when (values[key]) {
        "true" -> true
        "false" -> false
        else -> null
    }

    fun string(key: String): String? = values[key]?.let { v ->
        if (v.length >= 2 && v.startsWith('"') && v.endsWith('"')) unquote(v) else null
    }

    fun stringList(key: String): List<String>? {
        val v = values[key] ?: return null
        if (!v.startsWith("[") || !v.endsWith("]")) return null
        val body = v.substring(1, v.length - 1)
        val out = ArrayList<String>()
        Regex("\"((?:[^\"\\\\]|\\\\.)*)\"").findAll(body).forEach { out += unquote("\"${it.groupValues[1]}\"") }
        return out
    }

    private fun unquote(v: String): String =
        v.substring(1, v.length - 1).replace("\\\"", "\"").replace("\\\\", "\\")
}
