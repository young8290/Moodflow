package com.moodflow.ime.core

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

    /** 内置方案的手势模型；原生库不在时为 null。 */
    fun builtin(): ChordProfile? = NativeChord.builtin()?.use { of(it) }
}
