// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;


import android.content.Context;
import android.os.SystemClock;
import android.view.InputDevice;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.LinearLayout;

/** Retires the raw keyboard touch stream before a ViewGroup can split it onto a new key. */
public final class KeyboardRoot extends LinearLayout {
    private boolean activeStream;
    private long streamDownTime=-1,retiredDownTime=-1;
    private View inputSurface;
    private int naturalHeight,availableHeight,surfaceBudget;
    private int warnedAvailable=-1,warnedChrome=-1;
    private boolean chromeTooTall;

    public KeyboardRoot(Context context) { super(context); }

    /** The flexible keys/panel region; candidate, Buffer and safe padding keep their sizes. */
    public void setInputSurface(View surface) {
        if(inputSurface==surface) return;
        inputSurface=surface; requestLayout();
    }
    public int naturalHeight() { return naturalHeight; }
    public int availableHeight() { return availableHeight; }
    public int surfaceBudget() { return surfaceBudget; }
    public boolean chromeTooTall() { return chromeTooTall; }

    @Override protected void onMeasure(int widthSpec,int heightSpec) {
        int mode=MeasureSpec.getMode(heightSpec),maximum=MeasureSpec.getSize(heightSpec);
        if(getOrientation()!=VERTICAL || inputSurface==null || inputSurface.getParent()!=this
                || inputSurface.getVisibility()==GONE) {
            super.onMeasure(widthSpec,heightSpec);
            naturalHeight=getMeasuredHeight(); availableHeight=getMeasuredHeight(); surfaceBudget=0; chromeTooTall=false;
            return;
        }
        // Find the complete preferred stack before an OEM/window constraint clamps the root.
        super.onMeasure(widthSpec,MeasureSpec.makeMeasureSpec(0,MeasureSpec.UNSPECIFIED));
        naturalHeight=getMeasuredHeight();
        availableHeight=mode==MeasureSpec.UNSPECIFIED?naturalHeight:maximum;
        int preferredSurface=inputSurface.getMeasuredHeight();
        int chrome=naturalHeight-preferredSurface;
        surfaceBudget=preferredSurface; chromeTooTall=false;
        if(mode!=MeasureSpec.UNSPECIFIED && maximum<naturalHeight) {
            surfaceBudget=Math.max(0,maximum-chrome);
            chromeTooTall=chrome>maximum;
            ViewGroup.LayoutParams params=inputSurface.getLayoutParams(); int preferred=params.height;
            params.height=surfaceBudget;
            try { super.onMeasure(widthSpec,heightSpec); }
            finally { params.height=preferred; }
            // No input contents are logged. A zero key budget cannot make oversized chrome fit.
            if(chromeTooTall && (warnedAvailable!=maximum || warnedChrome!=chrome)) {
                android.util.Log.w("Moodflow","Keyboard chrome exceeds available height: available="+maximum+", chrome="+chrome);
                warnedAvailable=maximum; warnedChrome=chrome;
            }
        } else if(mode==MeasureSpec.EXACTLY) {
            // Honor a larger exact parent without stretching the preferred key caps.
            super.onMeasure(widthSpec,heightSpec);
        }
    }

    @Override public boolean dispatchTouchEvent(MotionEvent event) {
        int action=event.getActionMasked();
        if(action==MotionEvent.ACTION_DOWN) {
            if(event.getDownTime()==retiredDownTime) return true;
            if(activeStream) cancelPendingInputEvents();
            activeStream=true; streamDownTime=event.getDownTime();
        } else if(!activeStream || event.getDownTime()!=streamDownTime) return true;
        try { return super.dispatchTouchEvent(event); }
        finally {
            if(action==MotionEvent.ACTION_UP || action==MotionEvent.ACTION_CANCEL) {
                activeStream=false; streamDownTime=-1;
            }
        }
    }

    @Override public void onCancelPendingInputEvents() {
        super.onCancelPendingInputEvents();
        long down=streamDownTime;
        if(activeStream) retiredDownTime=down;
        activeStream=false; streamDownTime=-1;
        // Also release native child touch targets. Clearing pressed flags alone would leave
        // ViewGroup's old target list able to translate a sibling POINTER_DOWN into DOWN.
        long now=SystemClock.uptimeMillis();
        MotionEvent cancel=MotionEvent.obtain(down>=0?down:now,now,MotionEvent.ACTION_CANCEL,0,0,0);
        cancel.setSource(InputDevice.SOURCE_TOUCHSCREEN);
        try { super.dispatchTouchEvent(cancel); }
        finally { cancel.recycle(); }
    }
}
