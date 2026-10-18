// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.view.View;
import com.moodflow.ime.keyboard.KeyboardLayout;
import com.moodflow.ime.keyboard.ChordLayout;
import com.moodflow.ime.keyboard.ChordProfile;

/** A non-interactive preview drawn from the same key frames and palettes as the IME. */
@android.annotation.SuppressLint("ViewConstructor") // Constructed in Java with a fixed, non-interactive preview model.
public final class SettingsKeyboardPreview extends View {
    private static final float WIDTH=393, CANDIDATES=32;
    private static final String[] NINE_LABELS={"","","ABC","DEF","GHI","JKL","MNO","PQRS","TUV","WXYZ"};
    private final String layout;
    private final KeyboardTheme theme;
    private final Paint paint=new Paint(Paint.ANTI_ALIAS_FLAG);
    private final boolean chord,split;
    private final java.util.List<KeyboardLayout.Key> keys;
    private final java.util.List<ChordLayout.Key> chordKeys;
    private final RectF frame=new RectF();
    private final java.util.EnumMap<KeyboardIcon,Drawable> icons=new java.util.EnumMap<>(KeyboardIcon.class);
    /** Moodflow: chord layouts need the core's chord profile; without it they preview as QWERTY. */
    public SettingsKeyboardPreview(Context context,String layout,KeyboardTheme theme,ChordProfile profile) {
        super(context); this.layout=layout; this.theme=theme;
        split=layout.equals("splitOrthogonal"); chord=profile!=null && (split || layout.equals("orthogonal"));
        keys=chord?java.util.Collections.emptyList():KeyboardLayout.keys(WIDTH-8,false,layout.equals("nineKey")?KeyboardLayout.Mode.NINE_KEY:KeyboardLayout.Mode.QWERTY);
        chordKeys=chord?ChordLayout.keys(WIDTH-8,split,profile):java.util.Collections.emptyList();
        for(KeyboardIcon icon:new KeyboardIcon[]{KeyboardIcon.CHEVRON_RIGHT,KeyboardIcon.DELETE,KeyboardIcon.SHIFT,KeyboardIcon.SMILE}) icons.put(icon,icon.drawable(context));
        paint.setTypeface(Typeface.create("sans-serif",Typeface.NORMAL));
        setContentDescription(context.getString(R.string.app_name)+" "+layout+" · "+theme.title); setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        setLayoutDirection(LAYOUT_DIRECTION_LTR);
    }
    private float logicalHeight() { return CANDIDATES+(chord?ChordLayout.height(WIDTH,split):KeyboardLayout.height(false))+8; }
    @Override protected void onMeasure(int widthSpec,int heightSpec) {
        int width=MeasureSpec.getSize(widthSpec);
        setMeasuredDimension(width,resolveSize(Math.round(width*logicalHeight()/WIDTH),heightSpec));
    }
    @Override protected void onDraw(Canvas canvas) {
        super.onDraw(canvas); float scale=getWidth()/WIDTH; canvas.save(); canvas.scale(scale,scale);
        KeyboardTheme.Palette palette=Themes.palette(getContext(),theme);
        paint.setColor(palette.background); frame.set(0,0,WIDTH,logicalHeight()); canvas.drawRoundRect(frame,10,10,paint);
        paint.setTextSize(15); paint.setColor(palette.ink);
        drawText(canvas,"你好",44,18); drawText(canvas,"世界",124,18); drawText(canvas,"输入",204,18);
        drawIcon(canvas,KeyboardIcon.CHEVRON_RIGHT,WIDTH-25,16,14,palette.ink);
        canvas.translate(4,CANDIDATES);
        if(chord) {
            for(ChordLayout.Key key:chordKeys) {
                frame.set(key.x,key.y,key.x+key.width,key.y+key.height);
                cap(canvas,frame,palette.key);
                if(key.action==ChordLayout.Action.DELETE) drawIcon(canvas,KeyboardIcon.DELETE,frame.centerX(),frame.centerY(),18,palette.ink);
                else if(key.action==ChordLayout.Action.EMOJI) drawIcon(canvas,KeyboardIcon.SMILE,frame.centerX(),frame.centerY(),18,palette.ink);
                else { paint.setTextSize(16); paint.setColor(palette.ink); drawText(canvas,key.text,frame.centerX(),frame.centerY()); }
            }
        } else {
            boolean nine=layout.equals("nineKey");
            for(KeyboardLayout.Key key:keys) {
                frame.set(key.visualX,key.visualY,key.visualX+key.visualWidth,key.visualY+key.visualHeight);
                boolean functional=!nine && key.action!=KeyboardLayout.Action.TEXT && key.action!=KeyboardLayout.Action.SPACE;
                cap(canvas,frame,key.action==KeyboardLayout.Action.RETURN?palette.accent:functional?palette.functional:palette.key);
                int color=key.action==KeyboardLayout.Action.RETURN?palette.accentInk:palette.ink;
                KeyboardIcon icon=key.action==KeyboardLayout.Action.DELETE?KeyboardIcon.DELETE:key.action==KeyboardLayout.Action.SHIFT?KeyboardIcon.SHIFT:key.action==KeyboardLayout.Action.EMOJI?KeyboardIcon.SMILE:null;
                if(icon!=null) drawIcon(canvas,icon,frame.centerX(),frame.centerY(),18,color);
                else {
                    String label=label(key,nine); paint.setTextSize(nine?14:16); paint.setColor(color);
                    drawText(canvas,label,frame.centerX(),frame.centerY());
                }
            }
        }
        canvas.restore();
    }
    private void cap(Canvas canvas,RectF frame,int color) { paint.setColor(color); canvas.drawRoundRect(frame,5,5,paint); }
    private void drawText(Canvas canvas,String text,float x,float y) {
        canvas.drawText(text,x-paint.measureText(text)/2,y-(paint.ascent()+paint.descent())/2,paint);
    }
    private void drawIcon(Canvas canvas,KeyboardIcon icon,float x,float y,int size,int color) {
        Drawable drawable=icons.get(icon); drawable.setTint(color);
        drawable.setBounds(Math.round(x-size/2f),Math.round(y-size/2f),Math.round(x+size/2f),Math.round(y+size/2f)); drawable.draw(canvas);
    }
    private String label(KeyboardLayout.Key key,boolean nine) {
        boolean chinese=getResources().getConfiguration().getLocales().get(0).getLanguage().equals("zh");
        switch(key.action) {
            case TEXT: if(nine) return NINE_LABELS[Integer.parseInt(key.text)]; return key.text;
            case NUMBERS:return "123";case SYMBOLS:return "#+=";case LANGUAGE:return "中/En";
            case RETURN:return chinese?"换行":"return";case SPACE:return chinese?"空格":"space";
            case PUNCTUATION:return "，。";case SEPARATOR:return "分词";case SPELLING:return "拼音";default:return "";
        }
    }
}
