// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Fits the whole key surface, including its touch cells, into a reduced vertical budget. */
public final class KeyboardGeometry {
    private KeyboardGeometry() {}
    public static List<KeyboardLayout.Key> fitHeight(List<KeyboardLayout.Key> keys,float preferred,float actual) {
        if(!Float.isFinite(preferred) || preferred<=0 || !Float.isFinite(actual) || actual<0)
            throw new IllegalArgumentException("Keyboard heights must be finite, preferred positive and actual nonnegative");
        if(actual>=preferred) return keys;
        float scale=actual/preferred; List<KeyboardLayout.Key> fitted=new ArrayList<>(keys.size());
        for(KeyboardLayout.Key key:keys) fitted.add(new KeyboardLayout.Key(key.action,key.text,
                key.x,key.y*scale,key.width,key.height*scale,
                key.visualX,key.visualY*scale,key.visualWidth,key.visualHeight*scale));
        return Collections.unmodifiableList(fitted);
    }
}
