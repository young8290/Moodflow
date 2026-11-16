package com.moodflow.ime.logic

import org.junit.Assert.*
import org.junit.Test

class ChordDraftTest {
    private val base = ChordDraft(
        "跨端方案", listOf("a", "s", "d"), listOf("j", "k", "comma", "period"), 275,
        listOf(ChordDraft.Row(listOf("a", "j"), "ni", false), ChordDraft.Row(listOf("s", "d"), "sh", true)),
    )

    @Test fun aliasesAreNormalizedButCannotAssignOneKeyTwice() {
        assertEquals(listOf("a", "comma", "period"), ChordDraft.keys(" A  ,\tPeriod "))
        assertNotNull(base.copy(right = ChordDraft.keys("j , comma")).issue(apply = true))
        assertNotNull(base.copy(right = listOf("semicolon", "j")).issue(apply = true))
        assertNotNull(base.copy(right = listOf("a", "j")).issue(apply = true))
        assertNull(base.issue(apply = true))
    }

    @Test fun draftRequiresMappingsOnlyOnActivation() {
        val blank = base.copy(mappings = emptyList())
        assertNull(blank.issue(apply = false))
        assertNotNull(blank.issue(apply = true))
        assertNotNull(base.copy(intervalMs = 2001).issue(apply = true))
        assertNotNull(base.copy(name = "   ").issue(apply = false))
    }

    @Test fun rejectsCombinationsThatCannotBePerformedOnTheTouchSurface() {
        for (keys in listOf(listOf("a"), listOf("a", "a"), listOf("a", "q"), listOf("a", "s", "d", "j"))) {
            assertEquals(0, base.copy(mappings = listOf(ChordDraft.Row(keys, "ni", false))).issue(true)?.row)
        }
        val cramped = base.copy(right = listOf("n", "m", "comma", "period", "b"))
        assertTrue(cramped.issue(true)?.message?.contains("右手第 3 行") == true)
        val duplicate = base.copy(mappings = base.mappings + base.mappings[0].copy(keys = listOf("j", "a")))
        assertEquals(2, duplicate.issue(true)?.row)
    }

    @Test fun searchPreservesSourceRowForEditingFilteredResults() {
        assertEquals(listOf(1), base.matching(" SH ").map { it.index })
        assertEquals(listOf(0), base.matching("a j").map { it.index })
        assertEquals(listOf(0, 1), base.matching(" ").map { it.index })
        assertTrue(base.matching("she").isEmpty())
    }
}
