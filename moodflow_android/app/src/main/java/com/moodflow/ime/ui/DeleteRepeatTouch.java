// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.os.Handler;
import android.os.Looper;
import android.view.MotionEvent;
import java.util.Objects;
import java.util.function.BooleanSupplier;

/** Native short click and long-press recognition, with cancellable held backspace repeats. */
public final class DeleteRepeatTouch {
    static final long REPEAT_MILLIS=75;
    private final KeyButton button;
    private final BooleanSupplier canRepeat;
    private final Handler main=new Handler(Looper.getMainLooper());
    private boolean active,repeating;
    private int pointer=-1;
    private long downTime=-1,excludedDownTime=-1,generation;
    private Runnable pending;

    public DeleteRepeatTouch(KeyButton button) {
        this(button,() -> true);
    }
    public DeleteRepeatTouch(KeyButton button,BooleanSupplier canRepeat) {
        this.button=Objects.requireNonNull(button);
        this.canRepeat=Objects.requireNonNull(canRepeat);
        button.setOnTouchListener((view,event) -> observe(event));
        button.setOnLongClickListener(view -> beginRepeating());
    }
    /** Retire both scheduled repeats and the native click belonging to this held press. */
    public void cancel() {
        stop();
        button.cancelPendingInputEvents();
    }
    /** Deny long-press ownership to every split child of this raw multi-pointer stream. */
    public boolean suppressForStream(long streamDownTime) {
        excludedDownTime=streamDownTime;
        // An already consumed long-click must not become a release click after cancellation.
        boolean consumed=active && repeating;
        if(consumed) cancel(); else stop();
        return consumed;
    }
    private void stop() {
        generation++; active=false; repeating=false; pointer=-1; downTime=-1;
        if(pending!=null) { main.removeCallbacks(pending); pending=null; }
    }
    private boolean observe(MotionEvent event) {
        int action=event.getActionMasked();
        if(action==MotionEvent.ACTION_DOWN) {
            // A completed prior tap can still have a legitimate native click in the queue.
            // Only an unfinished press needs native cancellation before a new stream starts.
            if(active) cancel(); else stop();
            // ViewGroup can translate a sibling POINTER_DOWN to DOWN with the same raw time.
            // Leave its ordinary Button press intact, but never reacquire repeat eligibility.
            if(event.getDownTime()==excludedDownTime) return false;
            active=true; pointer=event.getPointerId(0); downTime=event.getDownTime();
        } else if(active && downTime==event.getDownTime()) {
            if(action==MotionEvent.ACTION_MOVE) {
                int index=event.findPointerIndex(pointer);
                if(index<0 || !inside(event.getX(index),event.getY(index))) cancel();
            } else if(action==MotionEvent.ACTION_POINTER_DOWN || action==MotionEvent.ACTION_CANCEL
                    || action==MotionEvent.ACTION_POINTER_UP && event.getPointerId(event.getActionIndex())==pointer) {
                cancel();
            } else if(action==MotionEvent.ACTION_UP) {
                // Keep native Button's long-click flag intact: it suppresses the release click.
                stop();
            }
        }
        // Button still owns press feedback, tap clicks, accessibility and its long-press delay.
        return false;
    }
    private boolean inside(float x,float y) {
        return x>=0 && y>=0 && x<button.getWidth() && y<button.getHeight();
    }
    private boolean valid(long ticket) {
        return active && repeating && generation==ticket && button.isPressed() && button.isEnabled()
                && button.isShown() && button.isAttachedToWindow();
    }
    private boolean beginRepeating() {
        // An accessibility long-click without a held touch must not start a repeating task.
        if(!active || !button.isPressed() || !button.isEnabled() || !button.isShown()
                || !button.isAttachedToWindow()) return false;
        repeating=true; long ticket=generation;
        // A busy serial engine must not accumulate held deletes behind its current work.
        // This gate applies only to repeats; Button still delivers every ordinary short tap.
        if(canRepeat.getAsBoolean()) button.performClick();
        if(valid(ticket)) {
            pending=new Runnable() {
                @Override public void run() {
                    if(!valid(ticket)) { if(generation==ticket) stop(); return; }
                    // Skip this tick while busy, with no debt to replay when work completes.
                    if(canRepeat.getAsBoolean()) button.performClick();
                    // The action can synchronously retire a target or replace this surface.
                    if(valid(ticket)) main.postDelayed(this,REPEAT_MILLIS);
                }
            };
            main.postDelayed(pending,REPEAT_MILLIS);
        }
        return true;
    }
}
