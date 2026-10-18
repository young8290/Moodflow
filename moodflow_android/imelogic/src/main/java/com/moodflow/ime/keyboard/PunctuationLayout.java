// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.Arrays;
import java.util.Collections;
import java.util.List;

/**
 * Number and symbol pages plus the nine-key punctuation strip. iOS keeps the same table in
 * PunctuationLayout.swift; both are checked against Shared's punctuation-layout.tsv fixture.
 */
public final class PunctuationLayout {
    private static final String[] NUMERIC={"1234567890","-/:;()$&@\"",".,?!'"};
    private static final String[] SYMBOLS={"[]{}#%^*+=","_\\|~<>€£¥•",".,?!'"};
    /** Chinese input shows the marks it types. The half-width '.' stays for decimals. */
    private static final String[] CHINESE_NUMERIC={"1234567890","-/：；（）￥@“”","。，、？！."};
    private static final String[] CHINESE_SYMBOLS={"【】｛｝#%^*+=","_—\\｜～《》$&·","…‘’「」〈〉"};
    /** Offered in the candidate row by the nine-key punctuation key. */
    public static final List<String> STRIP=Collections.unmodifiableList(
            Arrays.asList("，","。","？","！","、","：","；","…","“","”","（","）"));
    private static final String SENTENCE_MARKS="，。？！、：；";
    private PunctuationLayout() {}
    public static String[] rows(boolean symbols,boolean chinese) {
        return (symbols?(chinese?CHINESE_SYMBOLS:SYMBOLS):(chinese?CHINESE_NUMERIC:NUMERIC)).clone();
    }
    /** A sentence mark typed first on a number page was the reason for opening it. */
    public static boolean returnsToLetters(String text) {
        return text.length()==1 && SENTENCE_MARKS.contains(text);
    }
}
