// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import android.content.Context;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import com.moodflow.ime.R;
import com.moodflow.ime.keyboard.KeyboardTheme;
import com.moodflow.ime.logic.ClipEntry;
import java.util.List;
import java.util.function.Consumer;

/**
 * Clipboard history panel drawn inside the key surface.
 *
 * <p>Moodflow: the entries are the core's clipboard history (the same {@code clipboard_history.json}
 * Windows uses, gated by {@code clipboard.enabled} and the privacy rules); XSIME's explicit-collect
 * store is replaced by foreground capture, so the panel adds a pin toggle and an "enable" action
 * when history recording is off.
 */
public final class TextClipboardPanel extends LinearLayout {
    public interface Listener {
        void collect();
        void select(ClipEntry entry);
        void pin(ClipEntry entry);
        void remove(ClipEntry entry);
        void clear();
        void enable();
        void close();
    }
    private final LinearLayout entries;
    private final TextView heading,note;
    private final KeyButton collect,clear,close;
    private List<ClipEntry> shown=java.util.Collections.emptyList();
    private KeyboardTheme shownTheme;
    private boolean wasEnabled;
    private long presentation;
    private final Listener listener;
    public TextClipboardPanel(Context context,Listener listener) {
        super(context); setOrientation(VERTICAL); this.listener=listener;
        heading=new TextView(context); heading.setText(R.string.clipboard_history); heading.setTextSize(16); heading.setPadding(dp(8),dp(6),dp(8),dp(2)); addView(heading);
        LinearLayout actions=new LinearLayout(context); addView(actions,new LayoutParams(-1,dp(40)));
        collect=button(actions,"",() -> { if(wasEnabled) listener.collect(); else listener.enable(); },2);
        clear=button(actions,getResources().getString(R.string.clipboard_clear),listener::clear,1);
        close=button(actions,getResources().getString(R.string.clipboard_close),listener::close,1);
        note=new TextView(context); note.setTextSize(12); note.setPadding(dp(8),dp(4),dp(8),dp(4)); addView(note);
        ScrollView scroll=new ScrollView(context); scroll.setFillViewport(true); addView(scroll,new LayoutParams(-1,0,1));
        entries=new LinearLayout(context); entries.setOrientation(VERTICAL); scroll.addView(entries);
    }
    /** @param enabled whether the core records clipboard history ({@code clipboard.enabled}) */
    public void render(List<ClipEntry> values,KeyboardTheme theme,boolean enabled) {
        KeyboardTheme.Palette palette=Themes.palette(getContext(),theme);
        setBackgroundColor(palette.background); heading.setTextColor(palette.ink); note.setTextColor(palette.ink);
        note.setText(!enabled?getResources().getString(R.string.clipboard_history_off)
                :values.isEmpty()?getResources().getString(R.string.clipboard_history_empty):getResources().getString(R.string.clipboard_history_hint));
        collect.setText(enabled?R.string.clipboard_collect:R.string.clipboard_enable);
        collect.setContentDescription(collect.getText());
        collect.theme(theme); clear.theme(theme); close.theme(theme); clear.setEnabled(!values.isEmpty());
        boolean unchanged=theme==shownTheme && enabled==wasEnabled && values.equals(shown);
        if(unchanged) return;
        shown=values; shownTheme=theme; wasEnabled=enabled; entries.removeAllViews();
        final long token=++presentation;
        for(ClipEntry entry:values) {
            LinearLayout row=new LinearLayout(getContext()); entries.addView(row,new LayoutParams(-1,dp(58)));
            String text=entry.getText();
            String preview=text.length()>120?text.substring(0,120)+"…":text;
            KeyButton item=button(row,preview,() -> { if(token==presentation) listener.select(entry); },1); item.setMaxLines(2); item.setSingleLine(false);
            item.setContentDescription(getResources().getString(R.string.clipboard_insert)+": "+preview); item.theme(theme);
            KeyButton pin=button(row,entry.getPinned()?"★":"☆",() -> { if(token==presentation) listener.pin(entry); },0);
            pin.setContentDescription(getResources().getString(entry.getPinned()?R.string.clipboard_unpin:R.string.clipboard_pin)); pin.theme(theme);
            KeyButton delete=button(row,"×",() -> { if(token==presentation) listener.remove(entry); },0);
            delete.setContentDescription(getResources().getString(R.string.clipboard_delete)); delete.theme(theme);
        }
    }
    /** Scrub old-target text when the panel closes or the field changes. */
    public void redact() { presentation++; wasEnabled=false; entries.removeAllViews(); shown=java.util.Collections.emptyList(); note.setText(""); }
    private KeyButton button(LinearLayout row,String text,Runnable action,float weight) {
        KeyButton button=new KeyButton(getContext()); button.setText(text); button.font(13); button.setContentDescription(text);
        button.setOnClickListener(v -> action.run()); row.addView(button,new LayoutParams(weight==0?dp(44):0,-1,weight)); return button;
    }
    private int dp(int value) { return Math.round(value*getResources().getDisplayMetrics().density); }
}
