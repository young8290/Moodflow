package com.moodflow.ime.logic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivatePinyinTest {
    private val lookup: (String) -> List<String> = {
        when (it) {
            "ni" -> listOf("你", "尼", "呢")
            "ni'hao" -> listOf("你好")
            else -> emptyList()
        }
    }

    @Test
    fun pagesUseAbsolutePositionsAndCommitOnlyChosenCandidate() {
        val editor = PrivatePinyin()
        "ni".forEach { assertTrue(editor.append(it, lookup)) }
        assertEquals(CandidateWindow(1, 3, 0, listOf("尼", "呢")), editor.window(1, 2))
        assertNull(editor.choose(30))
        assertEquals("ni", editor.spelling)
        assertEquals("呢", editor.choose(2))
        assertEquals("", editor.spelling)
        assertEquals(CandidateWindow.EMPTY, editor.window(0, 2))
    }

    @Test
    fun rollbackAndLiteralSettlementNeverCarryStateToAnotherField() {
        val editor = PrivatePinyin()
        "ni'hao".forEach { editor.append(it, lookup) }
        assertEquals(listOf("你好"), editor.window(0, 1).items)
        assertTrue(editor.backspace(lookup))
        assertEquals("ni'ha", editor.finish(0, raw = true))
        assertFalse(editor.backspace(lookup))
        "ni".forEach { editor.append(it, lookup) }
        editor.clear()
        assertNull(editor.finish(0, raw = false))
        assertEquals(emptyList<String>(), editor.window(0, 10).items)
    }

    @Test
    fun limitsComplexityAndRejectsUnexpectedKeysBeforeLookup() {
        val editor = PrivatePinyin()
        for (ch in listOf('N', ' ', '/', '\'', '3')) {
            assertFalse(editor.append(ch) { error("Invalid keys must not be queried") })
        }
        repeat(48) { assertTrue(editor.append('a', lookup)) }
        assertFalse(editor.append('a') { error("Overlong input must not be queried") })
        assertEquals(48, editor.spelling.length)
    }
}
