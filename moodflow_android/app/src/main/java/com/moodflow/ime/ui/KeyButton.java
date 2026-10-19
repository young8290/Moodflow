// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.os.Build;
import android.view.HapticFeedbackConstants;
import android.view.MotionEvent;
import android.widget.Button;

/** A native accessible button with a full touch cell and an inset, visibly pressed keycap. */
public final class KeyButton extends Button {
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private Drawable iconDrawable;
    private int iconTint;
    private final RectF cap=new RectF();
    private KeyboardTheme.Palette palette;
    private KeyboardTheme currentTheme;
    private int currentUiMode=-1;
    private boolean functional,compact,accent,plain,classic,shortcut;
    private KeyboardIcon icon;
    private float iconSize=20;
    private boolean iconWithText;
    private float frameX,frameY,frameWidth=-1,frameHeight=-1;
    private boolean nativeTouchActive;
    private long nativeDownTime=-1;
    private String candidateLabel="",candidateComment="";
    private static final float COMMENT_SCALE=0.65f;
    /** TextView measures the small annotation; painting uses the same two font sizes. */
    public void candidate(String text,String comment) {
        if(candidateLabel.equals(text) && candidateComment.equals(comment) && getText().length()>0) return;
        candidateLabel=text; candidateComment=comment;
        if(comment.isEmpty()) { setText(text); return; }
        android.text.SpannableString label=new android.text.SpannableString(text+" "+comment);
        label.setSpan(new android.text.style.RelativeSizeSpan(COMMENT_SCALE),text.length(),label.length(),android.text.Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
        setText(label);
    }
    public KeyButton(Context context) {
        super(context);
        setAllCaps(false); setMinWidth(0); setMinimumWidth(0); setMinHeight(0); setMinimumHeight(0);
        int vertical=Math.round(4*getResources().getDisplayMetrics().density);
        setPadding(1,vertical,1,vertical); setBackgroundColor(android.graphics.Color.TRANSPARENT);
        setStateListAnimator(null); setElevation(0); setIncludeFontPadding(false);
        setSingleLine(true); setGravity(android.view.Gravity.CENTER);
        setAutoSizeTextTypeUniformWithConfiguration(10,24,1,android.util.TypedValue.COMPLEX_UNIT_SP);
        theme(KeyboardTheme.ALL[0]);
    }
    public void appearance(boolean functional,boolean compact,boolean accent) {
        if(this.functional==functional && this.compact==compact && this.accent==accent) return;
        this.functional=functional; this.compact=compact; this.accent=accent; updateTextColors(); invalidate();
    }
    public void classic(boolean value) { if(classic!=value) { classic=value; updateTextColors(); invalidate(); } }
    private boolean systemCaps() { return palette.system && !classic; }
    /** Visible iOS frame inside this button's larger touch cell, in dp. */
    public void capFrame(float x,float y,float width,float height) {
        if(frameX==x && frameY==y && frameWidth==width && frameHeight==height) return;
        frameX=x; frameY=y; frameWidth=width; frameHeight=height; invalidate();
    }
    /** Candidates are plain text with transient press feedback, without a persistent cap. */
    public void plain(boolean value) { if(plain!=value) { plain=value; updateTextColors(); invalidate(); } }
    /** Flat plugin entry: rounded system fill, with neither a keycap shadow nor a border. */
    public void shortcut(boolean value) { if(shortcut!=value) { shortcut=value; updateTextColors(); invalidate(); } }
    public void icon(KeyboardIcon value) { icon(value,20,false); }
    public void icon(KeyboardIcon value,float sizeDp) { icon(value,sizeDp,false); }
    /** Preserve native text/AX labels while painting either a glyph or a glyph-and-label tile. */
    public void icon(KeyboardIcon value,float sizeDp,boolean withText) {
        if(!Float.isFinite(sizeDp) || sizeDp<=0) throw new IllegalArgumentException("Icon size must be positive");
        if(icon==value && iconSize==sizeDp && iconWithText==withText) return;
        if(icon!=value) { iconDrawable=value==null?null:value.drawable(getContext()); iconTint=0; }
        icon=value; iconSize=sizeDp; iconWithText=withText; requestLayout(); invalidate();
    }
    public void theme(KeyboardTheme theme) {
        int uiMode=getResources().getConfiguration().uiMode;
        if(currentTheme==theme && currentUiMode==uiMode) return;
        currentTheme=theme; currentUiMode=uiMode;
        palette=Themes.palette(getContext(),theme);
        updateTextColors();
        invalidate();
    }
    private void updateTextColors() {
        if(palette==null) return;
        int selectedInk=systemCaps() && !accent && !shortcut?palette.ink:palette.accentInk;
        setTextColor(new ColorStateList(new int[][]{new int[]{-android.R.attr.state_enabled},
                new int[]{android.R.attr.state_pressed,android.R.attr.state_selected},new int[]{android.R.attr.state_pressed},
                new int[]{android.R.attr.state_selected},new int[]{}},
                new int[]{plain?palette.ink:(palette.ink&0xFFFFFF)|0x66000000,(!systemCaps() || accent || shortcut)?palette.pressedSelectedInk:palette.accentInk,
                    palette.accentInk,selectedInk,palette.ink}));
    }
    public void font(int size) { setAutoSizeTextTypeUniformWithConfiguration(10,size,1,android.util.TypedValue.COMPLEX_UNIT_SP); }
    public void fontStyle(boolean monospaced,int size) {
        fontStyle(monospaced,size,monospaced);
    }
    public void fontStyle(boolean monospaced,int size,boolean medium) {
        int style=(monospaced?1:0)|(medium?2:0);
        if(fontStyle==style && fontLimit==size) return;
        fontStyle=style;
        Typeface face=monospaced?Typeface.MONOSPACE:Typeface.create("sans-serif",Typeface.NORMAL);
        if(medium) face=Build.VERSION.SDK_INT>=28?Typeface.create(face,500,false)
                :monospaced?Typeface.MONOSPACE:Typeface.create("sans-serif-medium",Typeface.NORMAL);
        if(!face.equals(getTypeface())) setTypeface(face);
        if(fontLimit!=size) { fontLimit=size; font(size); }
    }
    private int fontLimit=-1,fontStyle=-1;
    @Override protected void onMeasure(int widthSpec,int heightSpec) {
        super.onMeasure(widthSpec,heightSpec);
        if(icon!=null && iconWithText && MeasureSpec.getMode(widthSpec)!=MeasureSpec.EXACTLY) {
            float density=getResources().getDisplayMetrics().density;
            float textWidth=getPaint().measureText(getText().toString());
            int desired=(int)Math.ceil(iconSize*density+(getText().length()>0?3*density:0)+textWidth+getPaddingLeft()+getPaddingRight());
            setMeasuredDimension(resolveSize(Math.max(getSuggestedMinimumWidth(),desired),widthSpec),getMeasuredHeight());
        }
    }
    /** A target loss cancels queued native clicks and retires the lower-level press still held. */
    @Override public void onCancelPendingInputEvents() {
        super.onCancelPendingInputEvents();
        nativeTouchActive=false; nativeDownTime=-1; setPressed(false);
    }
    @Override public boolean onTouchEvent(MotionEvent event) {
        int action=event.getActionMasked();
        if(action==MotionEvent.ACTION_DOWN) {
            nativeTouchActive=true; nativeDownTime=event.getDownTime();
        } else if(!nativeTouchActive || event.getDownTime()!=nativeDownTime) return true;
        try { return super.onTouchEvent(event); }
        finally {
            if(action==MotionEvent.ACTION_UP || action==MotionEvent.ACTION_CANCEL) {
                nativeTouchActive=false; nativeDownTime=-1;
            }
        }
    }
    // Accessibility clicks have no touch stream and remain a native, independent action.
    @Override public boolean performClick() { performHapticFeedback(HapticFeedbackConstants.KEYBOARD_TAP); return super.performClick(); }
    @Override protected void drawableStateChanged() { super.drawableStateChanged(); invalidate(); }
    @Override protected void onDraw(Canvas canvas) {
        // Single-line TextView centers its text in a horizontally scrolling layout.
        // Caps belong to the view viewport, not to that text layout's scroll offset.
        int saved=canvas.save(); canvas.translate(getScrollX(),getScrollY());
        float density=getResources().getDisplayMetrics().density;
        float x=frameX*density,y=frameY*density;
        float width=frameWidth<0?getWidth():frameWidth*density,height=frameHeight<0?getHeight():frameHeight*density;
        boolean pressed=isPressed(),selected=isSelected(); int alpha=isEnabled()?255:102;
        float radius=(systemCaps()?5:compact?3:6)*density;
        if(shortcut) {
            cap.set(x,y,x+width,y+height);
            paint.setColor(pressed || selected?palette.accent:(palette.ink&0xffffff)|(palette.dark?0x1a000000:0x10000000));
            if(!isEnabled()) paint.setAlpha(Math.round(paint.getAlpha()*0.4f));
            canvas.drawRoundRect(cap,7*density,7*density,paint);
        } else if(plain) {
            if(pressed) {
                cap.set(x,y,x+width,y+height); paint.setColor(palette.accent); paint.setAlpha(alpha);
                canvas.drawRoundRect(cap,4*density,4*density,paint);
            }
        } else if(systemCaps()) {
            cap.set(x,y+0.5f*density,x+width,y+height-0.5f*density);
            cap.offset(0,density); paint.setColor(palette.dark?0x80000000:0x40000000);
            if(!isEnabled()) paint.setAlpha(Math.round(paint.getAlpha()*0.4f));
            canvas.drawRoundRect(cap,radius,radius,paint); cap.offset(0,-density);
            int fill=pressed?(accent && selected?palette.pressedSelected:palette.accent)
                    :accent && selected?palette.accent:selected?palette.key:functional?palette.functional:palette.key;
            paint.setColor(fill); paint.setAlpha(alpha); canvas.drawRoundRect(cap,radius,radius,paint);
        } else {
            float dx=(compact?0:0.5f)*density,dy=(compact?0.75f:1.5f)*density;
            cap.set(x+dx,y+dy,x+width-dx,y+height-dy);
            cap.offset(0,(compact?0.75f:1.5f)*density); paint.setColor(palette.dark?0xFF545458:0xFFC6C6C8); paint.setAlpha(alpha);
            canvas.drawRoundRect(cap,radius,radius,paint);
            cap.set(x+dx,y+dy,x+width-dx,y+height-dy);
            cap.offset(0,(pressed?(compact?0.75f:1.5f):(compact?-0.25f:-0.5f))*density);
            paint.setColor(pressed && selected?palette.pressedSelected:pressed || selected?palette.accent:functional?palette.functional:palette.key);
            paint.setAlpha(alpha); canvas.drawRoundRect(cap,radius,radius,paint);
            paint.setColor((pressed || selected?0x26000000:0x1A000000)|(palette.dark?0x00FFFFFF:0));
            if(!isEnabled()) paint.setAlpha(Math.round(paint.getAlpha()*0.4f));
            paint.setStyle(Paint.Style.STROKE); paint.setStrokeWidth(density*0.7f);
            canvas.drawRoundRect(cap,radius,radius,paint); paint.setStyle(Paint.Style.FILL);
        }
        // Draw the native button's label at the cap centre; the touch cell may include a gap.
        // Button text/content descriptions still provide native accessibility and keyboard focus.
        String text=getText().toString(); Paint textPaint=getPaint(); float originalSize=textPaint.getTextSize();
        float dx=(compact?0:0.5f)*density,dy=(compact?0.75f:1.5f)*density;
        float centreY=y+height/2+(plain || shortcut?0:pressed?(compact?0.75f:1.5f):(compact?-0.25f:-0.5f))*density;
        float textWidth=textPaint.measureText(text),availableWidth=Math.max(1,plain || shortcut?width-getPaddingLeft()-getPaddingRight():width-2*dx-2*density);
        Paint.FontMetrics metrics=textPaint.getFontMetrics(); float availableHeight=Math.max(1,height-2*dy);
        if(plain && !candidateComment.isEmpty()) {
            float mainWidth=textPaint.measureText(candidateLabel);
            textPaint.setTextSize(originalSize*COMMENT_SCALE);
            float commentWidth=textPaint.measureText(" "+candidateComment);
            float scale=Math.min(1,Math.min(availableWidth/Math.max(1,mainWidth+commentWidth),availableHeight/(metrics.descent-metrics.ascent)));
            float left=x+(width-(mainWidth+commentWidth)*scale)/2;
            textPaint.setTextSize(originalSize*scale); metrics=textPaint.getFontMetrics();
            textPaint.setColor(getCurrentTextColor());
            canvas.drawText(candidateLabel,left,centreY-(metrics.ascent+metrics.descent)/2,textPaint);
            textPaint.setTextSize(originalSize*scale*COMMENT_SCALE); metrics=textPaint.getFontMetrics();
            if(!pressed) textPaint.setAlpha(Math.round(android.graphics.Color.alpha(getCurrentTextColor())*0.72f));
            canvas.drawText(" "+candidateComment,left+mainWidth*scale,centreY-(metrics.ascent+metrics.descent)/2,textPaint);
            textPaint.setTextSize(originalSize); textPaint.setColor(getCurrentTextColor());
            canvas.restoreToCount(saved); return;
        }
        if(icon!=null) {
            float glyphSize=Math.max(1,Math.min(iconSize*density,Math.min(availableWidth,availableHeight-2*density)));
            float gap=iconWithText && !text.isEmpty()?3*density:0;
            if(iconWithText && !text.isEmpty()) {
                float scale=Math.min(1,Math.min(Math.max(1,availableWidth-glyphSize-gap)/Math.max(1,textWidth),availableHeight/(metrics.descent-metrics.ascent)));
                if(scale<1) textPaint.setTextSize(originalSize*scale);
                textWidth=textPaint.measureText(text); metrics=textPaint.getFontMetrics();
                float left=x+(width-glyphSize-gap-textWidth)/2;
                drawIcon(canvas,left,centreY-glyphSize/2,glyphSize);
                textPaint.setColor(getCurrentTextColor());
                canvas.drawText(text,left+glyphSize+gap,centreY-(metrics.ascent+metrics.descent)/2,textPaint);
            } else {
                drawIcon(canvas,x+(width-glyphSize)/2,centreY-glyphSize/2,glyphSize);
            }
            textPaint.setTextSize(originalSize); canvas.restoreToCount(saved); return;
        }
        float scale=Math.min(1,Math.min(availableWidth/Math.max(1,textWidth),availableHeight/(metrics.descent-metrics.ascent)));
        if(scale<1) textPaint.setTextSize(originalSize*scale);
        metrics=textPaint.getFontMetrics(); textPaint.setColor(getCurrentTextColor());
        canvas.drawText(text,x+(width-textPaint.measureText(text))/2,centreY-(metrics.ascent+metrics.descent)/2,textPaint);
        textPaint.setTextSize(originalSize); canvas.restoreToCount(saved);
    }
    private void drawIcon(Canvas canvas,float left,float top,float size) {
        int color=getCurrentTextColor();
        if(iconTint!=color) { iconTint=color; iconDrawable.setTint(color); }
        iconDrawable.setBounds(Math.round(left),Math.round(top),Math.round(left+size),Math.round(top+size));
        iconDrawable.draw(canvas);
    }
}
