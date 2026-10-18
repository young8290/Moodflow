// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Touch-chord key sets for {@link ChordGesture}: which keys each hand owns, which combinations
 * exist, and the pinyin each combination previews.
 *
 * <p>Moodflow: XSIME embedded its mappings (ChordData) and encoded them to Natural Code for
 * librime. Here the mappings come from the core's built-in keymap
 * ({@code moodflow_mobile::chord::MobileChord::builtin}, passed over JNI), and the text that is
 * actually typed is settled by the core ({@code settle_batch}); this class only drives the
 * gesture model (eligible keys, endpoint constraints, preview). XSIME's "left fragment + right
 * fragment" rule is already expanded into explicit mappings in that keymap, so resolution here is
 * a plain lookup.
 */
public final class ChordProfile {
    private static final String ALPHABET="abcdefghijklmnopqrstuvwxyz,.";
    public static final class Entry {
        public final String keys,output;
        public final boolean fragment;
        final int mask;
        public Entry(String keys,String output,boolean fragment) {
            this.keys=keys; this.output=output; this.fragment=fragment; mask=mask(keys);
            if(mask==0 || output==null || output.isEmpty()) throw new IllegalArgumentException("Invalid chord "+keys);
        }
    }
    public static final class Resolution {
        public final String keys,preview;
        Resolution(String keys,String preview) { this.keys=keys; this.preview=preview; }
        /** Exactly one key: typed as that key, not looked up. */
        public boolean single() { return keys.length()==1; }
        @Override public boolean equals(Object other) {
            if(!(other instanceof Resolution)) return false;
            Resolution r=(Resolution)other;
            return keys.equals(r.keys) && preview.equals(r.preview);
        }
        @Override public int hashCode() { return java.util.Objects.hash(keys,preview); }
        @Override public String toString() { return keys+"→"+preview; }
    }
    public final String leftKeys,rightKeys;
    public final List<Entry> entries;
    public final int leftMask,rightMask,allMask;
    final int[] combinations;
    private final Map<Integer,Entry> mappings;

    /**
     * @param leftKeys  left-hand keys as characters ({@code ,} and {@code .} for comma/period)
     * @param rightKeys right-hand keys
     */
    public ChordProfile(String leftKeys,String rightKeys,List<Entry> source) {
        this.leftKeys=leftKeys; this.rightKeys=rightKeys;
        leftMask=mask(leftKeys); rightMask=mask(rightKeys); allMask=leftMask|rightMask;
        if(leftMask==0 || rightMask==0 || (leftMask&rightMask)!=0) throw new IllegalArgumentException("Invalid chord hands");
        List<Entry> values=new ArrayList<>(); Map<Integer,Entry> indexed=new HashMap<>();
        for(Entry entry:source) {
            if(indexed.put(entry.mask,entry)!=null || (entry.mask&~allMask)!=0
                    || Integer.bitCount(entry.mask&leftMask)>2 || Integer.bitCount(entry.mask&rightMask)>2)
                throw new IllegalArgumentException("Invalid chord "+entry.keys);
            values.add(entry);
        }
        entries=Collections.unmodifiableList(values); mappings=Collections.unmodifiableMap(indexed);
        Set<Integer> reachable=new LinkedHashSet<>();
        for(int i=0;i<ALPHABET.length();i++) if((allMask&(1<<i))!=0) reachable.add(1<<i);
        reachable.addAll(indexed.keySet());
        combinations=new int[reachable.size()]; int i=0;
        for(int keys:reachable) combinations[i++]=keys;
    }
    public static int mask(String keys) {
        if(keys==null) return 0; int result=0;
        for(int i=0;i<keys.length();i++) { int index=ALPHABET.indexOf(keys.charAt(i)); if(index<0) return 0; result|=1<<index; }
        return result;
    }
    public static int mask(char key) { int index=ALPHABET.indexOf(key); return index<0?0:1<<index; }
    public int hand(char key) { int bit=mask(key); return (bit&leftMask)!=0?0:(bit&rightMask)!=0?1:-1; }
    public int handMask(int hand) { return hand==0?leftMask:rightMask; }
    public String canonical(int keys) {
        StringBuilder result=new StringBuilder();
        for(char key:(leftKeys+rightKeys).toCharArray()) if((mask(key)&keys)!=0) result.append(key);
        return result.toString();
    }
    public Resolution resolve(String keys) { return resolve(mask(keys)); }
    public Resolution resolve(int keys) {
        if(keys==0 || (keys&~allMask)!=0) return null;
        String canonical=canonical(keys);
        if(Integer.bitCount(keys)==1) return new Resolution(canonical,canonical);
        Entry entry=mappings.get(keys);
        return entry==null?null:new Resolution(canonical,entry.output);
    }
    public String handOutput(int keys) {
        if(keys==0) return null;
        if(Integer.bitCount(keys)==1) return canonical(keys);
        Entry entry=mappings.get(keys); return entry==null?null:entry.output;
    }
}
