package com.moodflow.ime.keyboard;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Loads the core's built-in keymap ({@code moodflow-mobile/data/chord-builtin.json}) the same way
 * the app does over JNI, so the gesture model is tested against exactly the data the core settles.
 */
final class BuiltinChordProfile {
    private BuiltinChordProfile() {}
    private static final Pattern LIST=Pattern.compile("\"(left_keys|right_keys)\":\\s*\\[([^\\]]*)\\]");
    private static final Pattern MAPPING=Pattern.compile("\\{\"keys\":\\s*\\[([^\\]]*)\\],\\s*\"output\":\\s*\"([a-z']+)\",\\s*\"kind\":\\s*\"(syllable|fragment)\"\\}");
    static File file() {
        for(String path:new String[]{"../../moodflow_input/crates/moodflow-mobile/data/chord-builtin.json",
                "moodflow_input/crates/moodflow-mobile/data/chord-builtin.json"}) {
            File f=new File(path); if(f.isFile()) return f;
        }
        throw new IllegalStateException("chord-builtin.json not found from "+new File(".").getAbsolutePath());
    }
    private static String keys(String list) {
        StringBuilder out=new StringBuilder();
        for(String item:list.split(",")) {
            String name=item.trim().replace("\"","");
            if(name.isEmpty()) continue;
            out.append(name.equals("comma")?",":name.equals("period")?".":name);
        }
        return out.toString();
    }
    static ChordProfile load() {
        try {
            String json=new String(Files.readAllBytes(file().toPath()),StandardCharsets.UTF_8);
            String left=null,right=null;
            Matcher hands=LIST.matcher(json);
            while(hands.find()) { if(hands.group(1).equals("left_keys")) left=keys(hands.group(2)); else right=keys(hands.group(2)); }
            List<ChordProfile.Entry> entries=new ArrayList<>();
            Matcher m=MAPPING.matcher(json);
            while(m.find()) entries.add(new ChordProfile.Entry(keys(m.group(1)),m.group(2),m.group(3).equals("fragment")));
            return new ChordProfile(left,right,entries);
        } catch(java.io.IOException e) { throw new IllegalStateException(e); }
    }
}
