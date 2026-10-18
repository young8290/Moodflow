// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.view.View;
import android.widget.HorizontalScrollView;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/** Ephemeral block projection. Visible chips never change the text delivered to the host. */
public final class BufferRail extends HorizontalScrollView {
    private final Content content;
    private List<String> lastBlocks=Collections.emptyList();
    private List<String> lastSource;
    private String lastComposition="";
    private KeyboardTheme lastTheme;
    private int lastMode;
    private boolean lastLandscape;
    private boolean followPending;
    private String placeholder="输入内容暂存于此";
    public void placeholder(String value) { if(!placeholder.equals(value)) { placeholder=value; content.invalidate(); } }
    private float lastScaledDensity;
    private final Runnable followCaret=() -> { if(followPending && !isLayoutRequested()) { followPending=false; fullScroll(FOCUS_RIGHT); } };
    public BufferRail(Context context) {
        super(context); setHorizontalScrollBarEnabled(false); setFillViewport(true); setClipToOutline(true); setSmoothScrollingEnabled(false);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        content=new Content(context); content.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        addView(content,new LayoutParams(LayoutParams.WRAP_CONTENT,LayoutParams.MATCH_PARENT));
    }
    public void render(List<String> blocks,String composing,KeyboardTheme theme,boolean landscape) {
        int mode=getResources().getConfiguration().uiMode;
        float scaledDensity=getResources().getDisplayMetrics().scaledDensity;
        // BufferSession supplies an immutable projection, stable until its revision changes.
        boolean blocksChanged=blocks!=lastSource && !blocks.equals(lastBlocks);
        boolean textChanged=blocksChanged || !composing.equals(lastComposition);
        boolean fontChanged=lastTheme==null || lastLandscape!=landscape || lastScaledDensity!=scaledDensity;
        if(!textChanged && lastTheme==theme && mode==lastMode && !fontChanged) return;
        if(lastTheme!=theme || lastMode!=mode) {
            KeyboardTheme.Palette palette=Themes.palette(getContext(),theme);
            GradientDrawable rail=new GradientDrawable(); rail.setColor(palette.dark?0xff000000:0xffffffff);
            rail.setCornerRadius(dp(7)); rail.setStroke(Math.max(1,Math.round(dp(1))),(palette.accent&0xffffff)|0xb3000000);
            setBackground(rail); content.palette=palette;
        }
        if(fontChanged) content.textPaint.setTextSize((landscape?14:15)*scaledDensity);
        if(fontChanged || blocksChanged) content.measureConfirmed(blocks,fontChanged);
        if(fontChanged || !composing.equals(lastComposition)) content.measureComposition(composing);
        if(blocksChanged) lastBlocks=new ArrayList<>(blocks);
        lastSource=blocks;
        lastComposition=composing; lastTheme=theme; lastMode=mode;
        lastLandscape=landscape; lastScaledDensity=scaledDensity;
        setContentDescription("Buffer "+content.confirmedText+composing);
        content.requestLayout(); content.invalidate();
        if(textChanged) { followPending=true; removeCallbacks(followCaret); post(followCaret); }
    }
    @Override protected void onLayout(boolean changed,int left,int top,int right,int bottom) {
        super.onLayout(changed,left,top,right,bottom);
        if(followPending) { followPending=false; fullScroll(FOCUS_RIGHT); }
    }
    @Override protected void onScrollChanged(int left,int top,int oldLeft,int oldTop) {
        super.onScrollChanged(left,top,oldLeft,oldTop);
        // The content records only the current viewport, so invalidate its display list on a swipe.
        if(content!=null) content.invalidate();
    }
    /** Scrub old-target text, drawing caches and accessibility together. */
    public void clearProjection() {
        removeCallbacks(followCaret); followPending=false; lastBlocks=Collections.emptyList(); lastSource=null; lastComposition=""; lastTheme=null;
        placeholder="输入内容暂存于此"; content.clear(); setContentDescription(null); scrollTo(0,0); content.requestLayout(); content.invalidate();
    }
    public int visibleChipCount() { return content.visibleChips; }
    public String drawingState() {
        return "scrollX="+getScrollX()+" rail="+getWidth()+"x"+getHeight()+" child="+content.getWidth()+"x"+content.getHeight()
                +" measured="+content.getMeasuredWidth()+" clip="+content.clip.toShortString()+" visible="+content.visibleChips
                +" window="+content.windowLeft+".."+content.windowRight+" clipAccepted="+content.clipAccepted
                +" blocks="+content.starts.length+" end="+content.confirmedEnd;
    }
    public long confirmedMeasurementCount() { return content.confirmedMeasurements; }
    private float dp(float value) { return value*getResources().getDisplayMetrics().density; }
    private static String oneLine(String value) {
        return value.indexOf('\n')<0 && value.indexOf('\r')<0?value:value.replace('\n','↵').replace('\r','\u200b');
    }
    private final class Content extends View {
        private final Paint textPaint=new Paint(Paint.ANTI_ALIAS_FLAG|Paint.SUBPIXEL_TEXT_FLAG);
        private final Paint decoration=new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF chip=new RectF();
        private final Rect clip=new Rect();
        private List<String> displayBlocks=Collections.emptyList();
        private Map<String,Float> measuredWidths=Collections.emptyMap();
        private float[] starts=new float[0],widths=new float[0];
        private String confirmedText="",composition="";
        private float confirmedEnd,compositionWidth;
        private KeyboardTheme.Palette palette;
        private int visibleChips;
        private float windowLeft,windowRight;
        private boolean clipAccepted;
        private long confirmedMeasurements;
        Content(Context context) { super(context); textPaint.setTypeface(Typeface.create("sans-serif",Typeface.NORMAL)); }
        void measureConfirmed(List<String> blocks,boolean fontChanged) {
            List<String> next=new ArrayList<>(blocks.size()); Map<String,Float> nextWidths=new HashMap<>();
            starts=new float[blocks.size()]; widths=new float[blocks.size()]; float x=dp(8);
            for(int i=0;i<blocks.size();i++) {
                String text=oneLine(blocks.get(i)); next.add(text);
                Float width=nextWidths.get(text);
                if(width==null && !fontChanged) width=measuredWidths.get(text);
                if(width==null) { width=textPaint.measureText(text); confirmedMeasurements++; }
                nextWidths.put(text,width); starts[i]=x; widths[i]=width; x+=width;
                if(i+1<blocks.size()) x+=dp(10);
            }
            displayBlocks=next; measuredWidths=nextWidths; confirmedEnd=x; confirmedText=String.join("",blocks);
        }
        void measureComposition(String value) { composition=oneLine(value); compositionWidth=textPaint.measureText(composition); }
        void clear() {
            displayBlocks=Collections.emptyList(); measuredWidths=Collections.emptyMap(); starts=new float[0]; widths=new float[0];
            confirmedText=""; composition=""; confirmedEnd=dp(8); compositionWidth=0; visibleChips=0;
        }
        @Override protected void onMeasure(int widthSpec,int heightSpec) {
            int desiredWidth=(int)Math.ceil(Math.max(dp(16),confirmedEnd+compositionWidth+dp(8)));
            setMeasuredDimension(resolveSize(desiredWidth,widthSpec),resolveSize(Math.round(dp(lastLandscape?28:36)),heightSpec));
        }
        private int firstVisible(float left) {
            int low=0,high=starts.length;
            while(low<high) {
                int mid=(low+high)/2;
                float end=starts[mid]+widths[mid]+(mid==starts.length-1?compositionWidth:0);
                if(end+dp(4)<left) low=mid+1; else high=mid;
            }
            return low;
        }
        private void background(Canvas canvas,float left,float right,boolean active) {
            chip.set(left-dp(4),dp(5),right+dp(4),getHeight()-dp(5));
            decoration.setColor(active?(palette.accent&0xffffff)|0x1f000000:(palette.ink&0xffffff)|0x10000000);
            decoration.setStyle(Paint.Style.FILL); canvas.drawRoundRect(chip,dp(5),dp(5),decoration);
            if(active) {
                decoration.setColor(palette.accent); decoration.setStyle(Paint.Style.STROKE); decoration.setStrokeWidth(dp(1));
                canvas.drawRoundRect(chip,dp(5),dp(5),decoration); decoration.setStyle(Paint.Style.FILL);
            }
        }
        @Override protected void onDraw(Canvas canvas) {
            super.onDraw(canvas); visibleChips=0; if(palette==null) return;
            int saved=canvas.save(); float scroll=BufferRail.this.getScrollX();
            // Cancel the huge child offset before issuing drawing commands. Bitmap and hardware
            // canvases then see bounded viewport coordinates even at the 16K-character limit.
            canvas.translate(scroll,0); canvas.getClipBounds(clip);
            float left=Math.max(clip.left,dp(2.5f));
            float right=Math.min(clip.right,BufferRail.this.getWidth()-dp(2.5f));
            windowLeft=left+scroll; windowRight=right+scroll;
            clipAccepted=canvas.clipRect(left,dp(2.5f),right,getHeight()-dp(2.5f));
            if(!clipAccepted) { canvas.restoreToCount(saved); return; }
            Paint.FontMetrics fm=textPaint.getFontMetrics(); float baseline=getHeight()/2f-(fm.ascent+fm.descent)/2;
            int first=firstVisible(windowLeft);
            for(int i=first;i<starts.length && starts[i]-dp(4)<=windowRight;i++) {
                boolean active=i==starts.length-1;
                float end=starts[i]+widths[i]+(active?compositionWidth:0);
                background(canvas,starts[i]-scroll,end-scroll,active); visibleChips++;
                textPaint.setColor(palette.ink); canvas.drawText(displayBlocks.get(i),starts[i]-scroll,baseline,textPaint);
            }
            float end=confirmedEnd+compositionWidth;
            if(!composition.isEmpty() && confirmedEnd<=windowRight && end>=windowLeft) {
                if(starts.length==0) { background(canvas,dp(8)-scroll,end-scroll,true); visibleChips++; }
                decoration.setColor((palette.accent&0xffffff)|0x1a000000);
                canvas.drawRect(confirmedEnd-scroll,baseline+fm.ascent,end-scroll,baseline+fm.descent,decoration);
                textPaint.setColor(palette.accentText); canvas.drawText(composition,confirmedEnd-scroll,baseline,textPaint);
                canvas.drawRect(confirmedEnd-scroll,baseline+dp(2),end-scroll,baseline+dp(3),textPaint);
            }
            if(displayBlocks.isEmpty() && composition.isEmpty()) {
                textPaint.setColor((palette.ink&0xffffff)|0x66000000); canvas.drawText(placeholder,dp(12)-scroll,baseline,textPaint);
            }
            float caretHeight=Math.min(getHeight()-dp(8),fm.descent-fm.ascent+dp(2));
            decoration.setColor(palette.accent);
            canvas.drawRect(end-scroll-dp(1),(getHeight()-caretHeight)/2,end-scroll+dp(1),(getHeight()+caretHeight)/2,decoration);
            canvas.restoreToCount(saved);
        }
    }
}
