// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.List;
import org.junit.Test;
import static org.junit.Assert.*;

public final class KeyboardGeometryTest {
    @Test public void constrainedFourRowsRemainFullyContainedInEveryMode() {
        for(boolean landscape:new boolean[]{false,true}) for(KeyboardLayout.Mode mode:KeyboardLayout.Mode.values()) {
            float preferred=KeyboardLayout.height(landscape),actual=preferred*0.72f;
            List<KeyboardLayout.Key> original=KeyboardLayout.keys(392,landscape,mode);
            List<KeyboardLayout.Key> fitted=KeyboardGeometry.fitHeight(original,preferred,actual);
            assertEquals(original.size(),fitted.size());
            float bottom=0;
            for(int i=0;i<fitted.size();i++) {
                KeyboardLayout.Key key=fitted.get(i),before=original.get(i);
                assertSame(before.action,key.action); assertEquals(before.text,key.text);
                assertEquals(before.x,key.x,0); assertEquals(before.width,key.width,0);
                assertEquals(before.visualX,key.visualX,0); assertEquals(before.visualWidth,key.visualWidth,0);
                assertTrue(key.height>0 && key.visualHeight>0);
                assertTrue(key.y>=0 && key.y+key.height<=actual+0.001f);
                assertTrue(key.visualY>=key.y-0.001f && key.visualY+key.visualHeight<=key.y+key.height+0.001f);
                bottom=Math.max(bottom,key.y+key.height);
            }
            assertEquals("the final touch row reaches the fitted surface",actual,bottom,0.001f);
        }
    }
    @Test public void footerSpaceAndReturnStayPresentUnderPortraitAndLandscapeLimits() {
        for(boolean landscape:new boolean[]{false,true}) for(KeyboardLayout.Mode mode:new KeyboardLayout.Mode[]{KeyboardLayout.Mode.QWERTY,KeyboardLayout.Mode.NINE_KEY}) {
            float actual=landscape?95:154;
            List<KeyboardLayout.Key> fitted=KeyboardGeometry.fitHeight(KeyboardLayout.keys(400,landscape,mode),KeyboardLayout.height(landscape),actual);
            for(KeyboardLayout.Action action:new KeyboardLayout.Action[]{KeyboardLayout.Action.SPACE,KeyboardLayout.Action.RETURN}) {
                KeyboardLayout.Key key=fitted.stream().filter(k -> k.action==action).findFirst().orElseThrow();
                assertTrue(key.visualHeight>0); assertTrue(key.visualY+key.visualHeight<=actual+0.001f);
                assertEquals(actual,key.y+key.height,0.001f);
            }
        }
    }
    @Test public void ordinaryHeightAndLargerParentKeepOriginalGeometryAndIdentity() {
        List<KeyboardLayout.Key> original=KeyboardLayout.keys(400,false,KeyboardLayout.Mode.QWERTY);
        assertSame(original,KeyboardGeometry.fitHeight(original,206,206));
        assertSame(original,KeyboardGeometry.fitHeight(original,206,300));
        assertEquals(206,original.stream().map(k -> k.y+k.height).max(Float::compare).orElseThrow(),0.001f);
    }
    @Test public void exhaustedBudgetHasNoNegativeOrOvershootingFrames() {
        List<KeyboardLayout.Key> fitted=KeyboardGeometry.fitHeight(KeyboardLayout.keys(400,false,KeyboardLayout.Mode.NINE_KEY),206,0);
        for(KeyboardLayout.Key key:fitted) {
            assertEquals(0,key.y,0); assertEquals(0,key.height,0);
            assertEquals(0,key.visualY,0); assertEquals(0,key.visualHeight,0);
        }
    }
    @Test public void constrainedResultDoesNotMutatePreferredReference() {
        List<KeyboardLayout.Key> original=KeyboardLayout.keys(400,false,KeyboardLayout.Mode.QWERTY);
        KeyboardGeometry.fitHeight(original,206,150);
        assertEquals(154.5,original.stream().filter(k -> k.action==KeyboardLayout.Action.SPACE).findFirst().orElseThrow().y,0.001);
    }
    @Test public void invalidBudgetsFailBeforeCreatingInvalidTouchFrames() {
        List<KeyboardLayout.Key> keys=KeyboardLayout.keys(400,false,KeyboardLayout.Mode.QWERTY);
        for(float[] pair:new float[][]{{0,10},{-1,10},{206,-1},{Float.NaN,10},{206,Float.POSITIVE_INFINITY}}) {
            try { KeyboardGeometry.fitHeight(keys,pair[0],pair[1]); fail("Invalid keyboard height accepted"); }
            catch(IllegalArgumentException expected) { }
        }
    }
}
