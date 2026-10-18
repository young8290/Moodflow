// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

/** Built-in hold/up hints match iOS Keyboard/OrdinaryKeyGesture.swift. */
public final class OrdinaryKeyAlternates {
    private static final String LETTERS="qwertyuiopasdfghjklzxcvbnm";
    private static final String[] DIGITS={"1","2","3","4","5","6","7","8","9","0"};
    private static final String[] CHINESE={"，","。","？","！","：","；","、","（","）","“","”","《","》","—","…","·"};
    private static final String[] ENGLISH={",",".","?","!",":",";","/","(",")","\"","'","<",">","-","…","@"};
    private static final String[] NINE_CHINESE={"，","。","？","！","：","；","、","…"};
    private static final String[] NINE_ENGLISH={",",".","?","!",":",";","/","…"};
    private OrdinaryKeyAlternates() {}

    public static String text(KeyboardLayout.Mode mode,String key,boolean chinese) {
        if(key==null || key.length()!=1) return null;
        char value=key.charAt(0);
        if(mode==KeyboardLayout.Mode.NINE_KEY) {
            return value>='2' && value<='9'?(chinese?NINE_CHINESE:NINE_ENGLISH)[value-'2']:null;
        }
        if(mode!=KeyboardLayout.Mode.QWERTY) return null;
        int index=LETTERS.indexOf(Character.toLowerCase(value));
        return index<0?null:index<10?DIGITS[index]:(chinese?CHINESE:ENGLISH)[index-10];
    }
}
