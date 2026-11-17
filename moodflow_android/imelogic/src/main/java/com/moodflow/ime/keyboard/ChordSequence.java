package com.moodflow.ime.keyboard;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Joins two released touch strokes when the active profile has that exact cross-hand mapping. */
public final class ChordSequence {
    private final ChordProfile profile;
    private long intervalMs;
    private ChordGesture.Stroke pending;
    private long deadline;
    private boolean following;
    private long revision;

    public ChordSequence(ChordProfile profile, long intervalMs) {
        this.profile = profile;
        this.intervalMs = intervalMs;
    }

    public void setIntervalMs(long intervalMs) { this.intervalMs = intervalMs; }

    public long revision() { return revision; }
    public boolean hasPending() { return pending != null; }
    public long deadline() { return pending != null && !following ? deadline : -1; }

    /** The second touch has to start in time. Its user may hold or slide before releasing. */
    public ChordProfile.Resolution begin(char key, long now) {
        if (pending == null) return null;
        if (now >= deadline || profile.hand(key) == hand(pending.mask)) return flush();
        following = true;
        revision++;
        return null;
    }

    public List<ChordProfile.Resolution> finish(ChordGesture.Stroke stroke, long now) {
        List<ChordProfile.Resolution> output = new ArrayList<>(2);
        if (pending != null) {
            if (following && hand(stroke.mask) >= 0 && hand(stroke.mask) != hand(pending.mask)) {
                ChordProfile.Resolution joined = profile.resolve(pending.mask | stroke.mask);
                if (joined != null) {
                    reset();
                    return Collections.singletonList(joined);
                }
            }
            ChordProfile.Resolution fallback = flush();
            if (fallback != null) output.add(fallback);
        }
        if (canContinue(stroke.mask)) {
            pending = stroke;
            deadline = now + intervalMs;
            revision++;
        } else if (stroke.resolution != null) {
            output.add(stroke.resolution);
        }
        return output;
    }

    public ChordProfile.Resolution expire(long now) {
        return pending != null && !following && now >= deadline ? flush() : null;
    }

    public ChordProfile.Resolution flush() {
        ChordProfile.Resolution result = pending == null ? null : pending.resolution;
        reset();
        return result;
    }

    public void reset() {
        if (pending == null) return;
        pending = null;
        following = false;
        deadline = 0;
        revision++;
    }

    public ChordGesture.Preview preview(ChordGesture.Preview live) {
        if (pending == null) return live;
        ChordGesture.Side saved = new ChordGesture.Side(
                profile.canonical(pending.mask), profile.handOutput(pending.mask));
        int hand = hand(pending.mask);
        if (live != null && live.left != null && live.right != null) return live;
        ChordGesture.Side left = hand == 0 ? saved : live == null ? null : live.left;
        ChordGesture.Side right = hand == 1 ? saved : live == null ? null : live.right;
        ChordProfile.Resolution joined = left == null || right == null ? null
                : profile.resolve(ChordProfile.mask(left.keys) | ChordProfile.mask(right.keys));
        return new ChordGesture.Preview(left, right, joined == null ? null : joined.preview, true);
    }

    private int hand(int keys) {
        if (keys == 0) return -1;
        if ((keys & profile.rightMask) == 0) return 0;
        return (keys & profile.leftMask) == 0 ? 1 : -1;
    }

    private boolean canContinue(int keys) {
        int hand = hand(keys);
        if (intervalMs <= 0 || hand < 0) return false;
        int same = profile.handMask(hand);
        for (int combo : profile.combinations) {
            if ((combo & same) == keys && (combo & ~same) != 0) return true;
        }
        return false;
    }
}
