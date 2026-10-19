// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.ui;

import android.content.Context;
import android.content.SharedPreferences;
import com.moodflow.ime.keyboard.KeyboardTheme;
import com.moodflow.ime.logic.KeyboardChoice;

/**
 * Validated keyboard preferences shared by the keyboard and the settings app.
 *
 * <p>Moodflow: only the host-side choices live here — key layout and palette. The Chinese scheme
 * is the core's {@code schema.active} (switched and persisted by the core), clipboard and plugins
 * are core config keys too; XSIME's translation direction, AI mock and learning switches are not
 * ported (see docs/design/xsime-feature-port-plan.md).
 */
public final class KeyboardSettings {
    public static final String PREFERENCES_NAME="keyboard";
    public static final String KEY_LAYOUT="layout";
    public static final String KEY_THEME="theme";
    public static final String DEFAULT_THEME="apple";
    private final SharedPreferences preferences;

    public KeyboardSettings(Context context) {
        this(context.getSharedPreferences(PREFERENCES_NAME,Context.MODE_PRIVATE));
    }
    public KeyboardSettings(SharedPreferences preferences) {
        if(preferences==null) throw new IllegalArgumentException("Keyboard preferences are required");
        this.preferences=preferences;
    }
    public SharedPreferences preferences() { return preferences; }
    public String getLayout() { return KeyboardChoice.INSTANCE.layout(preferences.getString(KEY_LAYOUT,null)); }
    public String getTheme() { return KeyboardTheme.named(preferences.getString(KEY_THEME,DEFAULT_THEME)).id; }
    public void setLayout(String value) { preferences.edit().putString(KEY_LAYOUT,KeyboardChoice.INSTANCE.layout(value)).apply(); }
    public void setTheme(String value) { preferences.edit().putString(KEY_THEME,KeyboardTheme.named(value).id).apply(); }
}
