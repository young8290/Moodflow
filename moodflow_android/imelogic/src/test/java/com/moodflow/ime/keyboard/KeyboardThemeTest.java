package com.moodflow.ime.keyboard;

import org.junit.Test;
import static org.junit.Assert.*;

public class KeyboardThemeTest {
    @Test public void eighteenPalettesWithUniqueIdsAndCycling() {
        assertEquals(18,KeyboardTheme.ALL.length);
        java.util.Set<String> ids=new java.util.HashSet<>();
        for(KeyboardTheme t:KeyboardTheme.ALL) assertTrue(t.id,ids.add(t.id));
        assertSame(KeyboardTheme.ALL[0],KeyboardTheme.named("missing"));
        assertSame(KeyboardTheme.ALL[1],KeyboardTheme.next("apple"));
        assertSame(KeyboardTheme.ALL[0],KeyboardTheme.next(KeyboardTheme.ALL[17].id));
    }
    @Test public void accentTextAndInkStayReadable() {
        for(KeyboardTheme t:KeyboardTheme.ALL) for(boolean dark:new boolean[]{false,true}) {
            KeyboardTheme.Palette p=t.palette(dark);
            assertEquals(dark,p.dark);
            assertTrue(t.id,KeyboardTheme.Palette.contrast(p.accentText,p.background)>=4.5
                    || p.accentText==p.ink);
            assertTrue(p.accentInk==0xFF000000 || p.accentInk==0xFFFFFFFF);
            assertEquals(0xFF000000,p.pressedSelected&0xFF000000);
        }
        assertTrue(KeyboardTheme.ALL[0].palette(false).system);
        assertFalse(KeyboardTheme.ALL[1].palette(false).system);
    }
    @Test public void hsvMatchesAndroidConventionAndRoundTrips() {
        float[] hsv=new float[3];
        KeyboardTheme.colorToHsv(0xFFFF0000,hsv); assertEquals(0,hsv[0],0.01); assertEquals(1,hsv[1],0.01); assertEquals(1,hsv[2],0.01);
        KeyboardTheme.colorToHsv(0xFF0000FF,hsv); assertEquals(240,hsv[0],0.01);
        for(int c:new int[]{0xFF007AFF,0xFFC65316,0xFFAAC921,0xFF808080,0xFF000000,0xFFFFFFFF}) {
            KeyboardTheme.colorToHsv(c,hsv); assertEquals(Integer.toHexString(c),c,KeyboardTheme.hsvToColor(hsv));
        }
        // pressed = value × 0.78
        KeyboardTheme.colorToHsv(0xFF0A84FF,hsv); hsv[2]*=0.78f;
        assertEquals(KeyboardTheme.hsvToColor(hsv),KeyboardTheme.ALL[0].palette(true).pressedSelected);
    }
}
