// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;

import android.content.Context;
import android.view.View;
import android.view.ViewGroup;
import android.widget.ScrollView;
import java.util.ArrayList;
import java.util.List;

/** Stable content coordinates: scrolling never measures or repacks candidates. */
public final class CandidateMatrixView extends ScrollView {
    /** Moodflow: candidates by absolute index, pulled from the core's candidate window. */
    public static final class Item {
        public final int index;
        public final String text, comment;
        public Item(int index,String text,String comment) { this.index=index; this.text=text; this.comment=comment; }
        @Override public boolean equals(Object value) {
            if(!(value instanceof Item)) return false;
            Item other=(Item)value;
            return index==other.index && text.equals(other.text) && comment.equals(other.comment);
        }
        @Override public int hashCode() { return java.util.Objects.hash(index,text,comment); }
    }
    public interface Listener { void select(Item item); void more(); }
    private final Flow content;
    private final Listener listener;
    private List<Item> items=java.util.Collections.emptyList();
    private KeyboardTheme theme=KeyboardTheme.ALL[0];
    public CandidateMatrixView(Context context,Listener listener) {
        super(context); this.listener=listener;
        setFillViewport(false); setVerticalScrollBarEnabled(false);
        content=new Flow(context); addView(content,new ScrollView.LayoutParams(-1,-2));
        setContentDescription("矩阵候选"); setTag("keyboard.candidates.matrix");
    }
    public void update(List<Item> next,KeyboardTheme theme,boolean enabled) {
        this.theme=theme;
        if(!items.equals(next)) {
            boolean appending=next.size()>=items.size() && next.subList(0,items.size()).equals(items);
            if(!appending) content.removeAllViews();
            items=new ArrayList<>(next);
            while(content.getChildCount()>items.size()) content.removeViewAt(content.getChildCount()-1);
            for(int i=0;i<items.size();i++) {
                final Item item=items.get(i);
                KeyButton button;
                if(i<content.getChildCount()) button=(KeyButton)content.getChildAt(i);
                else {
                    button=new KeyButton(getContext()); button.plain(true); button.setTextSize(20);
                    button.setAutoSizeTextTypeWithDefaults(android.widget.TextView.AUTO_SIZE_TEXT_TYPE_NONE);
                    button.setPadding(dp(6),0,dp(6),0); button.setMinWidth(dp(32));
                    button.setEllipsize(android.text.TextUtils.TruncateAt.END);
                    content.addView(button,new ViewGroup.LayoutParams(-2,dp(32)));
                }
                button.candidate(item.text,item.comment);
                button.setContentDescription("候选"+(item.index+1)+" "+item.text);
                button.setOnClickListener(v -> { if(v.getParent()==content && items.contains(item)) listener.select(item); });
            }
            if(!appending) scrollTo(0,0);
            content.requestLayout();
        }
        for(int i=0;i<content.getChildCount();i++) {
            KeyButton button=(KeyButton)content.getChildAt(i); button.theme(theme); button.setEnabled(enabled);
        }
    }
    @Override protected void onScrollChanged(int x,int y,int oldX,int oldY) {
        super.onScrollChanged(x,y,oldX,oldY);
        if(y+getHeight()>=content.getHeight()-dp(64)) listener.more();
    }
    public ViewGroup candidateContent() { return content; }
    private int dp(int value) { return Math.round(value*getResources().getDisplayMetrics().density); }
    private final class Flow extends ViewGroup {
        Flow(Context context) { super(context); }
        @Override protected void onMeasure(int widthSpec,int heightSpec) {
            int width=Math.max(1,MeasureSpec.getSize(widthSpec)),x=0,y=0,gap=dp(4),height=dp(32);
            for(int i=0;i<getChildCount();i++) {
                View child=getChildAt(i);
                child.measure(MeasureSpec.makeMeasureSpec(width,MeasureSpec.AT_MOST),MeasureSpec.makeMeasureSpec(height,MeasureSpec.EXACTLY));
                int w=child.getMeasuredWidth();
                if(x>0 && x+w>width) { x=0; y+=height+gap; }
                x+=w+gap;
            }
            setMeasuredDimension(width,getChildCount()==0?0:y+height);
        }
        @Override protected void onLayout(boolean changed,int left,int top,int right,int bottom) {
            int width=right-left,x=0,y=0,gap=dp(4),height=dp(32);
            for(int i=0;i<getChildCount();i++) {
                View child=getChildAt(i); int w=child.getMeasuredWidth();
                if(x>0 && x+w>width) { x=0; y+=height+gap; }
                child.layout(x,y,x+w,y+height); x+=w+gap;
            }
        }
    }
}
