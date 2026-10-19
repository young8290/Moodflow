package com.moodflow.ime.ui;

import android.content.Context;
import android.content.res.Configuration;
import com.moodflow.ime.keyboard.KeyboardTheme;

/** Picks the light or dark palette of a {@link KeyboardTheme} from the system night mode. */
public final class Themes {
    private Themes() {}
    public static boolean dark(Context context) {
        return (context.getResources().getConfiguration().uiMode&Configuration.UI_MODE_NIGHT_MASK)==Configuration.UI_MODE_NIGHT_YES;
    }
    public static KeyboardTheme.Palette palette(Context context,KeyboardTheme theme) { return theme.palette(dark(context)); }
}
