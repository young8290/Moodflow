// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.content.Context;
import android.content.res.Configuration;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import java.util.ArrayList;
import java.util.List;
import com.moodflow.ime.keyboard.KeyboardLayout;
import com.moodflow.ime.keyboard.KeyboardGeometry;
import com.moodflow.ime.keyboard.OrdinaryKeyAlternates;

/** Measured geometry owns every key width. Candidate refreshes never rebuild the touch surface. */
public final class KeyboardSurface extends ViewGroup {
    public interface Handler {
        String label(KeyboardLayout.Key key);
        String description(KeyboardLayout.Key key);
        boolean enabled(KeyboardLayout.Key key);
        boolean selected(KeyboardLayout.Key key);
        void press(KeyboardLayout.Key key);
        /** Literal alternate text must bypass Rime's candidate-number shortcuts. */
        default void onAlternate(String text) {}
        /** Held repeats skip a busy interval; ordinary taps keep their serial ordering. */
        default boolean canRepeatDelete() { return true; }
    }
    private KeyboardLayout.Mode mode;
    private boolean chinese;
    private List<KeyboardLayout.Key> frames;
    private final Handler handler;
    private KeyboardTheme theme=KeyboardTheme.ALL[0];
    private int themeUiMode=-1;
    private final List<DeleteRepeatTouch> deleteRepeats=new ArrayList<>();
    private final List<UpwardNumberTouch> alternates=new ArrayList<>();
    private final Paint hintPaint=new Paint(Paint.ANTI_ALIAS_FLAG);
    public KeyboardSurface(Context context,Handler handler) { super(context); this.handler=handler; setLayoutDirection(LAYOUT_DIRECTION_LTR); }
    public void render(KeyboardLayout.Mode mode,KeyboardTheme theme) { render(mode,false,theme); }
    public void render(KeyboardLayout.Mode mode,boolean chinese,KeyboardTheme theme) {
        // Letter-key alternates also depend on language; retire them when it changes.
        int uiMode=getResources().getConfiguration().uiMode;
        boolean rebuild=this.mode!=mode || this.chinese!=chinese;
        if(rebuild || this.theme!=theme || themeUiMode!=uiMode) cancelTouches();
        this.theme=theme;
        themeUiMode=uiMode;
        if(rebuild) {
            this.mode=mode; this.chinese=chinese; removeAllViews(); deleteRepeats.clear(); alternates.clear();
            frames=KeyboardLayout.keys(400,landscape(),mode,chinese);
            for(KeyboardLayout.Key key:frames) {
                KeyButton button=new KeyButton(getContext());
                button.setOnClickListener(v -> handler.press(key));
                if(key.action==KeyboardLayout.Action.DELETE) deleteRepeats.add(new DeleteRepeatTouch(button,handler::canRepeatDelete));
                String alternate=alternateFor(key);
                if(alternate!=null) alternates.add(new UpwardNumberTouch(button,alternate,() -> handler.onAlternate(alternate),this::invalidate));
                addView(button);
            }
            requestLayout();
        }
        for(int i=0;i<getChildCount();i++) {
            KeyboardLayout.Key key=frames.get(i); KeyButton button=(KeyButton)getChildAt(i);
            boolean system=theme.id.equals("apple"),letter=key.action==KeyboardLayout.Action.TEXT;
            boolean glyph=letter || key.action==KeyboardLayout.Action.PUNCTUATION && !key.text.isEmpty();
            boolean functional=mode!=KeyboardLayout.Mode.NINE_KEY && !letter && key.action!=KeyboardLayout.Action.SPACE;
            button.appearance(functional,false,key.action==KeyboardLayout.Action.RETURN);
            int font=glyph?(mode==KeyboardLayout.Mode.NINE_KEY?20:system?24:21)
                    :key.action==KeyboardLayout.Action.LANGUAGE?18:system?18:14;
            button.fontStyle(glyph && !system && mode!=KeyboardLayout.Mode.EMOJI,font,!system || key.action==KeyboardLayout.Action.LANGUAGE);
            button.icon(key.action==KeyboardLayout.Action.SHIFT?(handler.selected(key)?KeyboardIcon.SHIFT_FILL:KeyboardIcon.SHIFT)
                    :key.action==KeyboardLayout.Action.DELETE?KeyboardIcon.DELETE:key.action==KeyboardLayout.Action.EMOJI && mode!=KeyboardLayout.Mode.EMOJI?KeyboardIcon.SMILE:null);
            String label=handler.label(key); if(!android.text.TextUtils.equals(button.getText(),label)) button.setText(label);
            String description=handler.description(key);
            if(!android.text.TextUtils.equals(button.getContentDescription(),description)) button.setContentDescription(description);
            boolean enabled=handler.enabled(key);
            if(button.isEnabled() && !enabled && key.action==KeyboardLayout.Action.DELETE) cancelDeleteRepeats();
            if(button.isEnabled() && !enabled && alternateFor(key)!=null) cancelAlternates();
            button.setEnabled(enabled);
            button.setSelected(handler.selected(key)); button.theme(theme);
        }
    }
    private void cancelDeleteRepeats() {
        // Visibility callbacks can occur inside View's constructor, before field initialization.
        if(deleteRepeats!=null) for(DeleteRepeatTouch repeat:deleteRepeats) repeat.cancel();
    }
    private void cancelAlternates() {
        if(alternates!=null) for(UpwardNumberTouch swipe:alternates) swipe.cancel();
    }
    private void cancelTouches() { cancelDeleteRepeats(); cancelAlternates(); }
    @Override public boolean onInterceptTouchEvent(MotionEvent event) {
        if(event.getActionMasked()==MotionEvent.ACTION_POINTER_DOWN) {
            // Preserve overlapping ordinary native taps. Retire hold eligibility on
            // every sibling before ViewGroup splits this pointer into a new DOWN.
            // A hold which already won consumes the remainder without a release click.
            boolean consume=false;long stream=event.getDownTime();
            for(UpwardNumberTouch swipe:alternates) consume|=swipe.suppressForStream(stream);
            for(DeleteRepeatTouch repeat:deleteRepeats) consume|=repeat.suppressForStream(stream);
            if(consume) return true;
        }
        return super.onInterceptTouchEvent(event);
    }
    @Override public boolean onTouchEvent(MotionEvent event) { return true; }
    @Override public void onCancelPendingInputEvents() {
        super.onCancelPendingInputEvents(); cancelTouches();
    }
    @Override protected void onVisibilityChanged(View changedView,int visibility) {
        super.onVisibilityChanged(changedView,visibility);
        if(visibility!=VISIBLE) cancelTouches();
    }
    @Override protected void onWindowVisibilityChanged(int visibility) {
        super.onWindowVisibilityChanged(visibility);
        if(visibility!=VISIBLE) cancelTouches();
    }
    @Override protected void onDetachedFromWindow() {
        cancelTouches(); super.onDetachedFromWindow();
    }
    private String alternateFor(KeyboardLayout.Key key) {
        return key.action==KeyboardLayout.Action.TEXT?OrdinaryKeyAlternates.text(mode,key.text,chinese):null;
    }
    @Override protected void dispatchDraw(Canvas canvas) {
        super.dispatchDraw(canvas);
        if(frames==null) return;
        float density=getResources().getDisplayMetrics().density;
        KeyboardTheme.Palette palette=Themes.palette(getContext(),theme); int swipeIndex=0;
        for(int i=0;i<frames.size();i++) {
            KeyboardLayout.Key key=frames.get(i); String alternate=alternateFor(key);
            if(alternate==null) continue;
            View button=getChildAt(i); UpwardNumberTouch swipe=alternates.get(swipeIndex++);
            hintPaint.setTextSize(Math.min(11,key.visualHeight*0.24f)*density);
            hintPaint.setColor(button.isPressed()?palette.accentInk:palette.ink);
            hintPaint.setAlpha(swipe.selected()?255:button.isEnabled()?140:56);
            float x=(key.visualX+key.visualWidth-4)*density-hintPaint.measureText(alternate);
            float baseline=(key.visualY+2)*density-hintPaint.ascent();
            canvas.drawText(alternate,x,baseline,hintPaint);
        }
    }
    private boolean landscape() { return getResources().getConfiguration().orientation==Configuration.ORIENTATION_LANDSCAPE; }
    @Override protected void onMeasure(int widthSpec,int heightSpec) {
        int width=MeasureSpec.getSize(widthSpec); float density=getResources().getDisplayMetrics().density;
        int height=Math.round(KeyboardLayout.height(landscape())*density);
        setMeasuredDimension(width,resolveSize(height,heightSpec));
        if(mode==null) return;
        frames=KeyboardGeometry.fitHeight(KeyboardLayout.keys(Math.max(1,width/density),landscape(),mode,chinese),
                KeyboardLayout.height(landscape()),getMeasuredHeight()/density);
        for(int i=0;i<frames.size();i++) {
            KeyboardLayout.Key key=frames.get(i);
            int w=Math.round((key.x+key.width)*density)-Math.round(key.x*density);
            int h=Math.round((key.y+key.height)*density)-Math.round(key.y*density);
            getChildAt(i).measure(MeasureSpec.makeMeasureSpec(w,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(h,MeasureSpec.EXACTLY));
            int x=Math.round(key.x*density),y=Math.round(key.y*density);
            int capX=Math.round(key.visualX*density),capY=Math.round(key.visualY*density);
            int capW=Math.round((key.visualX+key.visualWidth)*density)-capX;
            int capH=Math.round((key.visualY+key.visualHeight)*density)-capY;
            ((KeyButton)getChildAt(i)).capFrame((capX-x)/density,(capY-y)/density,capW/density,capH/density);
        }
    }
    @Override protected void onLayout(boolean changed,int l,int t,int r,int b) {
        if(changed) cancelTouches();
        float density=getResources().getDisplayMetrics().density;
        if(frames==null) return;
        for(int i=0;i<frames.size();i++) {
            KeyboardLayout.Key key=frames.get(i); View child=getChildAt(i);
            int x=Math.round(key.x*density),y=Math.round(key.y*density);
            child.layout(x,y,x+child.getMeasuredWidth(),y+child.getMeasuredHeight());
        }
    }
}
