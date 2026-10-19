// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.Locale;

/**
 * Nine-key spelling helpers.
 *
 * <p>Moodflow: XSIME also offered spelling choices from a librime digit prism. Moodflow's choices
 * and pinning come from the core ({@code moodflow_mobile::t9}); only the pure string rules that
 * the keyboard applies to its own input string remain here.
 */
public final class NineKeyPinyin {
    private static final String[] GROUPS={"abc","def","ghi","jkl","mno","pqrs","tuv","wxyz"};
    private NineKeyPinyin() {}
    public static String digits(String spelling) {
        StringBuilder out=new StringBuilder();
        for(char c:spelling.toLowerCase(Locale.ROOT).toCharArray()) {
            boolean found=false;
            for(int i=0;i<GROUPS.length;i++) if(GROUPS[i].indexOf(c)>=0) { out.append(i+2); found=true; break; }
            if(!found) out.append(c);
        }
        return out.toString();
    }
    /**
     * Deletes the last character. Deleting the separator after a pinned syllable returns that
     * syllable to its digits, so the user can choose a different spelling.
     */
    public static String backspace(String raw) {
        if(raw.isEmpty()) return raw;
        String rest=raw.substring(0,raw.length()-1);
        if(!raw.endsWith("'")) return rest;
        int boundary=rest.lastIndexOf('\'')+1; String last=rest.substring(boundary);
        return last.matches("[a-z]+")?rest.substring(0,boundary)+digits(last):rest;
    }
}
