package com.moodflow.ime.core

import android.content.Context
import android.util.Log
import com.moodflow.ime.keyboard.ChordProfile
import com.moodflow.ime.logic.ChordKeys

/** 核心并击方案 → 触屏手势用的 [ChordProfile]（键面字符：`comma` → `,`）。 */
object ChordProfiles {
    fun of(chord: NativeChord): ChordProfile? = try {
        val (left, right) = chord.hands()
        ChordProfile(
            left.joinToString("") { ChordKeys.char(it) },
            right.joinToString("") { ChordKeys.char(it) },
            chord.mappings().map { ChordProfile.Entry(it.chars, it.output, it.fragment) },
        )
    } catch (e: IllegalArgumentException) {
        Log.w("Moodflow", "并击方案不能用于触屏手势，并击布局停用")
        null
    }

    internal class Loaded(
        val chord: NativeChord,
        val surface: ChordProfile,
        val intervalMs: Long,
    ) : AutoCloseable {
        override fun close() = chord.close()
    }

    // Validate and construct the native settler and its preview together. A
    // corrupt or inaccessible custom snapshot falls back to the builtin.
    internal fun load(context: Context): Loaded? {
        val custom = runCatching {
            val json = ChordProfileStore(context).read().active ?: return@runCatching null
            val draft = ChordDraftCodec.decode(json)
            check(draft.issue(apply = true) == null)
            val chord = NativeChord.fromJson(json) ?: return@runCatching null
            val view = of(chord)
            if (view == null) {
                chord.close()
                null
            } else Loaded(chord, view, draft.intervalMs.toLong())
        }.getOrNull()
        if (custom != null) return custom
        val chord = NativeChord.builtin() ?: return null
        val view = of(chord)
        if (view != null) return Loaded(chord, view, 300L)
        chord.close()
        return null
    }

    fun preview(context: Context): ChordProfile? = load(context)?.use { it.surface }

    /** 内置方案的手势模型；原生库不在时为 null。 */
    fun builtin(): ChordProfile? = NativeChord.builtin()?.use { of(it) }
}
