package com.moodflow.ime.core

import android.content.Context
import android.util.AtomicFile
import com.moodflow.ime.logic.ChordDraft
import com.moodflow.ime.ui.KeyboardSettings
import org.json.JSONObject
import java.io.File
import java.io.FileNotFoundException
import java.util.UUID

/** One atomic snapshot keeps the editable draft and activated copy independent. */
internal class ChordProfileStore(context: Context) {
    private val app = context.applicationContext
    private val file = AtomicFile(File(app.filesDir, "android-chord-profiles.json"))

    data class Snapshot(val draft: String? = null, val active: String? = null)

    fun read(): Snapshot = synchronized(LOCK) { readLocked() }

    private fun readLocked(): Snapshot {
        val contents = try {
            file.openRead().use { ChordDraftCodec.boundedText(it, STATE_LIMIT) }
        } catch (e: FileNotFoundException) {
            if (file.baseFile.exists()) throw e
            return Snapshot()
        }
        val state = JSONObject(contents)
        require(state.getInt("version") == 1) { "并击存储版本不受支持。" }
        fun value(key: String) = if (state.isNull(key)) null else state.getString(key)
        return Snapshot(value("draft"), value("active"))
    }

    /** No mutation on error or if another editor saved after this one was opened. */
    fun write(expected: Snapshot, next: Snapshot): Snapshot = synchronized(LOCK) {
        check(readLocked() == expected) { "方案已在其他窗口修改，请返回并重新打开编辑器。" }
        if (next == expected) return@synchronized next
        val bytes = JSONObject().put("version", 1)
            .put("draft", next.draft ?: JSONObject.NULL)
            .put("active", next.active ?: JSONObject.NULL)
            .toString().toByteArray(Charsets.UTF_8)
        require(bytes.size <= STATE_LIMIT) { "并击方案存储过大。" }
        val stream = file.startWrite()
        try {
            stream.write(bytes)
            stream.fd.sync()
            file.finishWrite(stream)
        } catch (e: Exception) {
            file.failWrite(stream)
            throw e
        }
        check(readLocked() == next) { "无法确认方案已完整保存。" }
        if (next.active != expected.active) {
            // Data is durable first; this token only invalidates the in-process keyboard view.
            KeyboardSettings(app).preferences().edit()
                .putString(KeyboardSettings.KEY_CHORD_PROFILE, UUID.randomUUID().toString()).apply()
        }
        next
    }

    companion object {
        private val LOCK = Any()
        private const val STATE_LIMIT = 4 * ChordDraft.MAX_BYTES
    }
}
