package com.moodflow.ime.core

import com.moodflow.ime.logic.ChordDraft
import org.json.JSONArray
import org.json.JSONObject
import java.io.InputStream
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

internal object ChordDraftCodec {
    fun decode(json: String): ChordDraft {
        require(json.toByteArray(Charsets.UTF_8).size <= ChordDraft.MAX_BYTES) { "方案文件超过 256 KiB。" }
        val root = JSONObject(json)
        require(root.getInt("version") == 1) { "只支持版本 1 的并击方案。" }
        fun keys(obj: JSONObject, name: String): List<String> {
            val values = obj.getJSONArray(name)
            require(values.length() <= 28) { "方案键位过多。" }
            return (0 until values.length()).flatMap {
                val value = values.get(it) as? String ?: error("键名须为文本。")
                ChordDraft.keys(value).also { parsed -> require(parsed.size == 1) { "键名无效。" } }
            }
        }
        val rows = root.optJSONArray("mappings") ?: JSONArray()
        require(rows.length() <= ChordDraft.MAX_ROWS) { "映射数量超过上限。" }
        val mappings = (0 until rows.length()).map { index ->
            val row = rows.getJSONObject(index)
            val kind = row.getString("kind")
            require(kind == "syllable" || kind == "fragment") { "第 ${index + 1} 条映射的类型无效。" }
            ChordDraft.Row(keys(row, "keys"), row.getString("output"), kind == "fragment")
        }
        return ChordDraft(root.getString("name"), keys(root, "left_keys"), keys(root, "right_keys"),
            root.optInt("interval_ms", 300), mappings)
    }

    fun encode(draft: ChordDraft): String = JSONObject()
        .put("version", 1)
        .put("name", draft.name)
        .put("left_keys", JSONArray(draft.left))
        .put("right_keys", JSONArray(draft.right))
        .put("interval_ms", draft.intervalMs)
        .put("mappings", JSONArray(draft.mappings.map { row ->
            JSONObject().put("keys", JSONArray(row.keys)).put("output", row.output)
                .put("kind", if (row.fragment) "fragment" else "syllable")
        })).toString()

    /** Read at most limit + 1 bytes even if a document provider doesn't report a size. */
    fun boundedText(input: InputStream, limit: Int): String {
        val buffer = ByteArray(limit + 1)
        var size = 0
        while (size < buffer.size) {
            val n = input.read(buffer, size, buffer.size - size)
            if (n == -1) break
            if (n == 0) throw IOException("读取方案时连接中断。")
            size += n
        }
        require(size <= limit) { "方案文件过大。" }
        val decoder = Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
        return decoder.decode(ByteBuffer.wrap(buffer, 0, size)).toString().removePrefix("\uFEFF")
    }
}
