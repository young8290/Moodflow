// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import com.moodflow.ime.R;

import android.content.Context;
import android.graphics.drawable.Drawable;

/** Native vectors converted by XSIME from a pinned official Lucide (ISC) subset; see assets/licenses/Lucide-LICENSE.txt. */
public enum KeyboardIcon {
    SETTINGS(R.drawable.kb_icon_settings),
    SLIDERS(R.drawable.kb_icon_sliders),
    STACK_LAYERS(R.drawable.kb_icon_stack_layers),
    GRID_9(R.drawable.kb_icon_grid_9),
    PAPER_PLANE(R.drawable.kb_icon_paper_plane),
    SEND_ALL(R.drawable.kb_icon_send_all),
    SHIFT(R.drawable.kb_icon_shift),
    SHIFT_FILL(R.drawable.kb_icon_shift_fill),
    DELETE(R.drawable.kb_icon_delete),
    SMILE(R.drawable.kb_icon_smile),
    GLOBE(R.drawable.kb_icon_globe),
    TRANSLATE(R.drawable.kb_icon_translate),
    CHAT_QUESTION(R.drawable.kb_icon_chat_question),
    MAGIC_WAND(R.drawable.kb_icon_magic_wand),
    BOOK(R.drawable.kb_icon_book),
    PLAY(R.drawable.kb_icon_play),
    PAUSE(R.drawable.kb_icon_pause),
    STOP(R.drawable.kb_icon_stop),
    CHEVRON_LEFT(R.drawable.kb_icon_chevron_left),
    CHEVRON_RIGHT(R.drawable.kb_icon_chevron_right),
    CHECK(R.drawable.kb_icon_check),
    APPEARANCE(R.drawable.kb_icon_appearance),
    SPACE(R.drawable.kb_icon_space),
    KEYBOARD(R.drawable.kb_icon_keyboard),
    CLEAR(R.drawable.kb_icon_clear),
    WRITE(R.drawable.kb_icon_write);

    private final int resource;
    KeyboardIcon(int resource) { this.resource=resource; }
    public Drawable drawable(Context context) { return context.getDrawable(resource).mutate(); }
}
