package com.moodflow.ime.keyboard;

import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import org.junit.Test;
import static org.junit.Assert.*;

public class ChordSequenceTest {
    private final ChordProfile profile = new ChordProfile("ads", "jkl", Arrays.asList(
            new ChordProfile.Entry("adj", "zhang", false),
            new ChordProfile.Entry("ak", "ai", false),
            new ChordProfile.Entry("sl", "an", false)));

    private ChordGesture.Stroke stroke(String keys) {
        ChordGesture gesture = new ChordGesture(profile);
        gesture.begin(1, keys.charAt(0));
        return gesture.endStroke(1, keys.charAt(keys.length() - 1));
    }

    @Test public void unmappedHalfCanJoinInEitherOrderAndMayBeHeldAfterTheDeadline() {
        for (boolean reverse : new boolean[]{false, true}) {
            ChordSequence seq = new ChordSequence(profile, 300);
            String first = reverse ? "j" : "ad";
            String second = reverse ? "ad" : "j";
            assertTrue(seq.finish(stroke(first), 100).isEmpty());
            assertTrue(seq.preview(null).pending);
            assertNull(seq.begin(second.charAt(0), 399));
            assertEquals(-1, seq.deadline());
            assertNull(seq.expire(500));
            assertEquals(Collections.singletonList(profile.resolve("adj")), seq.finish(stroke(second), 900));
            assertFalse(seq.hasPending());
        }
    }

    @Test public void timeoutAndSameHandRetainOrdinarySingleKeyBehavior() {
        ChordSequence seq = new ChordSequence(profile, 300);
        assertTrue(seq.finish(stroke("a"), 100).isEmpty());
        assertNull(seq.expire(399));
        assertEquals(profile.resolve("a"), seq.expire(400));
        assertNull(seq.expire(450));
        assertTrue(seq.finish(stroke("a"), 500).isEmpty());
        assertEquals(profile.resolve("a"), seq.begin('s', 501));
        assertTrue(seq.finish(stroke("s"), 502).isEmpty());
        assertEquals(profile.resolve("s"), seq.flush());
    }

    @Test public void mismatchedPairDoesNotEatEitherValidStroke() {
        ChordSequence seq = new ChordSequence(profile, 300);
        seq.finish(stroke("a"), 100);
        assertNull(seq.begin('l', 101));
        assertEquals(Collections.singletonList(profile.resolve("a")), seq.finish(stroke("l"), 102));
        assertEquals(profile.resolve("l"), seq.expire(402));
    }

    @Test public void retiredTargetCannotReplayAWaitingStroke() {
        ChordSequence seq = new ChordSequence(profile, 300);
        seq.finish(stroke("a"), 200);
        seq.reset();
        assertNull(seq.expire(900));
        assertNull(seq.preview(null));
        assertNull(seq.begin('k', 201));
        assertTrue(seq.finish(stroke("k"), 202).isEmpty());
        assertEquals(profile.resolve("k"), seq.flush());
    }

    @Test public void simultaneousStrokesSettleWithoutWaiting() {
        ChordSequence seq = new ChordSequence(profile, 300);
        int mask = ChordProfile.mask("ak");
        List<ChordProfile.Resolution> output = seq.finish(new ChordGesture.Stroke(mask, profile.resolve(mask)), 100);
        assertEquals(Collections.singletonList(profile.resolve("ak")), output);
        assertFalse(seq.hasPending());
    }
}
