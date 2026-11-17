package com.moodflow.ime.logic

/** Field-local full-pinyin composition. Candidate lookup receives only the current spelling. */
class PrivatePinyin {
    var spelling: String = ""
        private set
    private var candidates: List<String> = emptyList()

    fun append(character: Char, lookup: (String) -> List<String>): Boolean {
        val allowed = character in 'a'..'z' ||
            (character == '\'' && spelling.isNotEmpty() && !spelling.endsWith('\''))
        if (!allowed || spelling.length >= MAX_SPELLING) return false
        spelling += character
        refresh(lookup)
        return true
    }

    fun backspace(lookup: (String) -> List<String>): Boolean {
        if (spelling.isEmpty()) return false
        spelling = spelling.dropLast(1)
        refresh(lookup)
        return true
    }

    private fun refresh(lookup: (String) -> List<String>) {
        candidates = if (spelling.isEmpty()) emptyList() else lookup(spelling.trimEnd('\''))
    }

    fun window(offset: Int, limit: Int): CandidateWindow {
        val start = offset.coerceIn(0, candidates.size)
        return CandidateWindow(start, candidates.size, 0, candidates.drop(start).take(limit.coerceAtLeast(0)))
    }

    fun choose(index: Int): String? = candidates.getOrNull(index)?.also { clear() }

    fun finish(index: Int, raw: Boolean): String? {
        if (spelling.isEmpty()) return null
        val text = if (raw) spelling else candidates.getOrNull(index) ?: spelling
        clear()
        return text
    }

    fun clear() {
        spelling = ""
        candidates = emptyList()
    }

    companion object {
        private const val MAX_SPELLING = 48
    }
}
