// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.MotionEvent;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;

/** iOS-style hold/up literal digit or mark; ordinary taps remain native Button clicks. */
public final class UpwardNumberTouch {
    static final long HOLD_MILLIS=320;
    private final KeyButton button;
    private final Runnable insert,repaint;
    private final Handler main=new Handler(Looper.getMainLooper());
    private final float density;
    private boolean active,eligible,armed,selected;
    private int pointer=-1;
    private long downTime=-1,beganAt,generation,excludedDownTime=-1;
    private float originX,originY;
    private Runnable pending;
    final String alternate;

    public UpwardNumberTouch(KeyButton button,String alternate,Runnable insert,Runnable repaint) {
        this.button=button; this.alternate=alternate; this.insert=insert; this.repaint=repaint;
        density=button.getResources().getDisplayMetrics().density;
        String kind=alternate.length()==1 && alternate.charAt(0)>='0' && alternate.charAt(0)<='9'?"数字 ":"符号 ";
        button.setOnTouchListener((view,event) -> observe(event));
        int accessibilityAction=R.id.key_alternate_action;
        button.setAccessibilityDelegate(new View.AccessibilityDelegate() {
            @Override public void onInitializeAccessibilityNodeInfo(View host,AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host,info);
                info.addAction(new AccessibilityNodeInfo.AccessibilityAction(accessibilityAction,"输入"+kind+alternate));
                info.setHintText("按住向上滑动输入"+kind+alternate);
            }
            @Override public boolean performAccessibilityAction(View host,int action,android.os.Bundle arguments) {
                if(action!=accessibilityAction) return super.performAccessibilityAction(host,action,arguments);
                if(!button.isEnabled() || !button.isShown() || !button.isAttachedToWindow()) return false;
                if(active) cancel(); else stop();
                insertAlternate(); return true;
            }
        });
    }
    public boolean selected() { return active && armed && selected; }
    /** Multitouch cancels only hold eligibility; overlapping native taps remain independent. */
    public boolean suppressForStream(long stream) {
        excludedDownTime=stream;
        boolean consume=active && armed;
        if(consume) cancel(); else stop();
        return consume;
    }
    private void insertAlternate() { button.performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP); insert.run(); }
    public void cancel() { stop(); button.cancelPendingInputEvents(); }
    private void stop() {
        boolean visible=armed || selected;
        generation++; active=false; eligible=false; armed=false; selected=false; pointer=-1; downTime=-1;
        if(pending!=null) { main.removeCallbacks(pending); pending=null; }
        if(visible) repaint.run();
    }
    private boolean usable() { return button.isEnabled() && button.isShown() && button.isAttachedToWindow(); }
    private boolean arm(long at) {
        if(!active || armed || !eligible || at-beganAt<HOLD_MILLIS || !usable() || !button.isPressed()) return false;
        armed=true; repaint.run(); return true;
    }
    private void move(float x,float y,long at) {
        arm(at);
        if(armed) {
            boolean next=originY-y>=18*density && Math.abs(x-originX)<=Math.max(24*density,button.getWidth()*0.8f);
            if(next!=selected) { selected=next; repaint.run(); }
        } else if(Math.hypot(x-originX,y-originY)>12*density) {
            eligible=false;
            if(pending!=null) { main.removeCallbacks(pending); pending=null; }
        }
    }
    private boolean inside(float x,float y) { return x>=0 && y>=0 && x<button.getWidth() && y<button.getHeight(); }
    private boolean observe(MotionEvent event) {
        int action=event.getActionMasked();
        if(action==MotionEvent.ACTION_DOWN) {
            // Preserve the legitimate pending native click of a previously completed tap.
            if(active) cancel(); else stop();
            if(event.getDownTime()==excludedDownTime) return false;
            active=true; eligible=true; pointer=event.getPointerId(0); downTime=event.getDownTime();
            beganAt=event.getEventTime(); originX=event.getX(); originY=event.getY(); long ticket=generation;
            pending=() -> { if(active && generation==ticket) { pending=null; arm(SystemClock.uptimeMillis()); } };
            main.postAtTime(pending,beganAt+HOLD_MILLIS); return false;
        }
        if(!active || event.getDownTime()!=downTime) return false;
        if(!usable() || armed && !button.isPressed()) { cancel(); return true; }
        if(action==MotionEvent.ACTION_CANCEL || action==MotionEvent.ACTION_POINTER_DOWN
                || action==MotionEvent.ACTION_POINTER_UP && event.getPointerId(event.getActionIndex())==pointer) {
            cancel(); return false;
        }
        int index=event.findPointerIndex(pointer);
        if(index<0) { cancel(); return true; }
        float x=event.getX(index),y=event.getY(index);
        if(action==MotionEvent.ACTION_MOVE) {
            move(x,y,event.getEventTime());
            // Once armed, preserve the native cap's feedback while the finger moves above it.
            return armed;
        }
        if(action==MotionEvent.ACTION_UP) {
            move(x,y,event.getEventTime());
            if(armed && selected) { cancel(); insertAlternate(); return true; }
            if(armed && !inside(x,y)) { cancel(); return true; }
            stop(); return false;
        }
        return false;
    }
}
