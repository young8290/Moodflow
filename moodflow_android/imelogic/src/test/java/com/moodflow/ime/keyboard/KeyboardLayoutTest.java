// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import org.junit.Test;
import java.util.List;
import static org.junit.Assert.*;

public class KeyboardLayoutTest {
    @Test public void everyModeFitsWithoutOverlappingAtPhoneAndTabletWidths() {
        for(int width:new int[]{280,320,360,411,600,840,1200}) for(boolean landscape:new boolean[]{false,true})
            for(KeyboardLayout.Mode mode:KeyboardLayout.Mode.values()) for(boolean chinese:new boolean[]{false,true}) {
                List<KeyboardLayout.Key> keys=KeyboardLayout.keys(width,landscape,mode,chinese);
                for(int i=0;i<keys.size();i++) {
                    KeyboardLayout.Key a=keys.get(i);
                    assertTrue(a.x>=0 && a.y>=0 && a.width>0 && a.height>=KeyboardLayout.height(landscape)/4f);
                    assertTrue(a.x+a.width<=width+0.01 && a.y+a.height<=KeyboardLayout.height(landscape));
                    assertTrue("cap left/top in touch cell",a.visualX>=a.x-0.01 && a.visualY>=a.y-0.01);
                    assertTrue("cap right/bottom in touch cell",a.visualX+a.visualWidth<=a.x+a.width+0.01
                            && a.visualY+a.visualHeight<=a.y+a.height+0.01);
                    assertTrue(a.visualWidth>0 && a.visualHeight>0);
                    for(int j=i+1;j<keys.size();j++) {
                        KeyboardLayout.Key b=keys.get(j);
                        assertFalse(mode+" overlap",Math.min(a.x+a.width,b.x+b.width)-Math.max(a.x,b.x)>0.01
                                && Math.min(a.y+a.height,b.y+b.height)-Math.max(a.y,b.y)>0.01);
                    }
                }
            }
    }
    @Test public void qwertyLettersHaveEqualWidthAndSpaceIsReachable() {
        List<KeyboardLayout.Key> keys=KeyboardLayout.keys(390,false,KeyboardLayout.Mode.QWERTY);
        int letters=0;
        for(KeyboardLayout.Key key:keys) {
            if(key.action==KeyboardLayout.Action.TEXT) { letters++; assertEquals(39,key.width,0.01); }
            if(key.action==KeyboardLayout.Action.SPACE) {
                assertTrue(key.width>=390*0.33);
                assertEquals(195,key.x+key.width/2,0.01);
            }
        }
        assertEquals(26,letters);
        assertEquals(19.5,keys.get(10).x,0.01);
        assertEquals(58.5,keys.get(19).x,0.01);
    }
    @Test public void nineKeysHaveTelephoneOrderAndEqualTouchArea() {
        List<KeyboardLayout.Key> keys=KeyboardLayout.keys(400,false,KeyboardLayout.Mode.NINE_KEY);
        int digits=0;
        for(KeyboardLayout.Key key:keys) if(key.action==KeyboardLayout.Action.TEXT) {
            digits++; assertEquals(80,key.width,0.01); assertEquals(51.5,key.height,0.01);
            if(key.text.equals("2")) { assertEquals(160,key.x,0.01); assertEquals(0,key.y,0.01); }
            if(key.text.equals("9")) { assertEquals(240,key.x,0.01); assertEquals(103,key.y,0.01); }
        }
        assertEquals(8,digits);
    }
    @Test public void letterCapsKeepTheIosReferenceWhileFrequentControlsRemainReachable() {
        assertEquals(206,KeyboardLayout.height(false)); assertEquals(143,KeyboardLayout.height(true));
        List<KeyboardLayout.Key> qwerty=KeyboardLayout.keys(392,false,KeyboardLayout.Mode.QWERTY);
        KeyboardLayout.Key q=qwerty.get(0),a=qwerty.get(10),z=qwerty.get(19);
        assertEquals(33.8,q.visualWidth,0.01); assertEquals(44,q.visualHeight,0.01);
        assertEquals(19.9,a.visualX,0.01); assertEquals(54,a.visualY,0.01);
        assertEquals(59.7,z.visualX,0.01); assertEquals(108,z.visualY,0.01);
        KeyboardLayout.Key shift=find(qwerty,KeyboardLayout.Action.SHIFT),space=find(qwerty,KeyboardLayout.Action.SPACE);
        assertEquals(45.08,shift.visualWidth,0.01);
        assertEquals(131.92,space.visualX,0.01); assertEquals(128.16,space.visualWidth,0.01);
        assertEquals(162,space.visualY,0.01);
        List<KeyboardLayout.Key> nine=KeyboardLayout.keys(392,false,KeyboardLayout.Mode.NINE_KEY);
        KeyboardLayout.Key enter=find(nine,KeyboardLayout.Action.RETURN),nineSpace=find(nine,KeyboardLayout.Action.SPACE);
        assertEquals(318.4,enter.visualX,0.01); assertEquals(108,enter.visualY,0.01);
        assertEquals(73.6,enter.visualWidth,0.01); assertEquals(98,enter.visualHeight,0.01);
        assertEquals(79.6,nineSpace.visualX,0.01); assertEquals(232.8,nineSpace.visualWidth,0.01);
        List<KeyboardLayout.Key> wide=KeyboardLayout.keys(840,true,KeyboardLayout.Mode.NINE_KEY);
        assertEquals(32,wide.get(0).visualHeight,0.01);
        assertEquals(69,find(wide,KeyboardLayout.Action.RETURN).visualHeight,0.01);
    }
    @Test public void dailyControlsHaveDirectMarksCenteredSpaceAndNoBackspaceDeadZone() {
        for(int width:new int[]{280,392,840}) for(boolean landscape:new boolean[]{false,true}) {
            List<KeyboardLayout.Key> q=KeyboardLayout.keys(width,landscape,KeyboardLayout.Mode.QWERTY);
            assertTrue(q.stream().anyMatch(key -> key.action==KeyboardLayout.Action.PUNCTUATION && key.text.equals(",")));
            assertTrue(q.stream().anyMatch(key -> key.action==KeyboardLayout.Action.PUNCTUATION && key.text.equals(".")));
            KeyboardLayout.Key delete=find(q,KeyboardLayout.Action.DELETE);
            KeyboardLayout.Key m=q.stream().filter(key -> key.text.equals("m")).findFirst().orElseThrow();
            assertEquals(m.x+m.width,delete.x,0.01);
            assertTrue(delete.visualWidth>=width*0.14f-0.01);
            assertEquals(width/2f,find(q,KeyboardLayout.Action.SPACE).visualX+find(q,KeyboardLayout.Action.SPACE).visualWidth/2,0.01);
            List<KeyboardLayout.Key> nine=KeyboardLayout.keys(width,landscape,KeyboardLayout.Mode.NINE_KEY);
            assertEquals(width/2f,find(nine,KeyboardLayout.Action.SPACE).x+find(nine,KeyboardLayout.Action.SPACE).width/2,0.01);
            assertEquals(KeyboardLayout.height(landscape)/2f,find(nine,KeyboardLayout.Action.DELETE).height,0.01);
            assertEquals(KeyboardLayout.height(landscape)/4f,find(nine,KeyboardLayout.Action.NUMBERS).y,0.01);
        }
    }
    private static String text(List<KeyboardLayout.Key> keys) {
        StringBuilder text=new StringBuilder();
        for(KeyboardLayout.Key key:keys) if(key.action==KeyboardLayout.Action.TEXT) text.append(key.text);
        return text.toString();
    }
    @Test public void chineseInputShowsTheMarksItTypesOnNumberAndSymbolPages() {
        assertEquals("1234567890-/:;()$&@\".,?!'",text(KeyboardLayout.keys(392,false,KeyboardLayout.Mode.NUMERIC)));
        assertEquals("1234567890-/：；（）￥@“”。，、？！.",text(KeyboardLayout.keys(392,false,KeyboardLayout.Mode.NUMERIC,true)));
        assertEquals("【】｛｝#%^*+=_—\\｜～《》$&·…‘’「」〈〉",text(KeyboardLayout.keys(392,false,KeyboardLayout.Mode.SYMBOLS,true)));
        // Letters, nine-key and emoji do not depend on the language.
        for(KeyboardLayout.Mode mode:new KeyboardLayout.Mode[]{KeyboardLayout.Mode.QWERTY,KeyboardLayout.Mode.NINE_KEY,KeyboardLayout.Mode.EMOJI})
            assertEquals(text(KeyboardLayout.keys(392,false,mode)),text(KeyboardLayout.keys(392,false,mode,true)));
        // Longer third rows stay centred between the side keys at the letter-key width.
        List<KeyboardLayout.Key> symbols=KeyboardLayout.keys(392,false,KeyboardLayout.Mode.SYMBOLS,true);
        KeyboardLayout.Key first=symbols.get(20),last=symbols.get(26),page=find(symbols,KeyboardLayout.Action.SYMBOLS),delete=find(symbols,KeyboardLayout.Action.DELETE);
        assertEquals("…",first.text); assertEquals("〉",last.text);
        assertEquals(39.2,first.width,0.01); assertEquals(58.8,first.x,0.01);
        assertTrue(first.visualX>page.visualX+page.visualWidth && last.visualX+last.visualWidth<delete.visualX);
    }
    @Test public void punctuationTablesMatchTheXsimeFixture() {
        // Moodflow: XSIME checks these against Shared/Tests/.../punctuation-layout.tsv, which is not
        // vendored here; the expected rows below are that fixture's values.
        assertArrayEquals(new String[]{"1234567890","-/:;()$&@\"",".,?!'"},PunctuationLayout.rows(false,false));
        assertArrayEquals(new String[]{"[]{}#%^*+=","_\\|~<>€£¥•",".,?!'"},PunctuationLayout.rows(true,false));
        assertEquals("，。？！、：；…“”（）",String.join("",PunctuationLayout.STRIP));
        StringBuilder sentence=new StringBuilder();
        for(String mark:PunctuationLayout.STRIP) if(PunctuationLayout.returnsToLetters(mark)) sentence.append(mark);
        assertEquals("，。？！、：；",sentence.toString());
        for(String other:new String[]{"1",".",",","“","（","…","-","","，。"}) assertFalse(other,PunctuationLayout.returnsToLetters(other));
    }
    private static KeyboardLayout.Key find(List<KeyboardLayout.Key> keys,KeyboardLayout.Action action) {
        return keys.stream().filter(key -> key.action==action).findFirst().orElseThrow();
    }
    @Test public void nineKeyBackspaceUnpinsTheLastSyllable() {
        // Moodflow: spelling choices and pinning moved to the core (moodflow_mobile::t9); the
        // digit and backspace rules stay on the keyboard side.
        assertEquals("64426",NineKeyPinyin.digits("nihao"));
        assertEquals("ni'426",NineKeyPinyin.backspace("ni'hao'"));
        assertEquals("6442",NineKeyPinyin.backspace("64426"));
        assertEquals("ni'42",NineKeyPinyin.backspace("ni'426"));
        assertEquals("",NineKeyPinyin.backspace(""));
    }
}
