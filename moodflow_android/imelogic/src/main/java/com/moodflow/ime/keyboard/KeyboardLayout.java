// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** iOS cap geometry with separate, contiguous touch cells. Dimensions are in dp. */
public final class KeyboardLayout {
    public enum Mode { QWERTY, NINE_KEY, NUMERIC, SYMBOLS, EMOJI }
    public enum Action { TEXT, SHIFT, DELETE, RETURN, NUMBERS, SYMBOLS, LANGUAGE, EMOJI, SPACE, SPELLING, SEPARATOR, PUNCTUATION }
    public static final String[] EMOJIS={"😀","😄","😂","🥹","😊","😍","😘","😎","🤔","😅",
            "😭","🥰","👍","🙏","❤️","🎉","🔥","✨","🌹","💪","👌","🤝","👏","💯","☀️","🌙","🍀","☕","🐱","🐶"};
    public static final class Key {
        public final Action action;
        public final String text;
        public final float x,y,width,height;
        public final float visualX,visualY,visualWidth,visualHeight;
        Key(Action action,String text,float x,float y,float width,float height,
                float visualX,float visualY,float visualWidth,float visualHeight) {
            this.action=action; this.text=text; this.x=x; this.y=y; this.width=width; this.height=height;
            this.visualX=visualX; this.visualY=visualY; this.visualWidth=visualWidth; this.visualHeight=visualHeight;
        }
    }
    private KeyboardLayout() {}
    public static int height(boolean landscape) { return landscape?143:206; }
    public static List<Key> keys(float width,boolean landscape,Mode mode) { return keys(width,landscape,mode,false); }
    /** Chinese input swaps the number and symbol rows for the marks they type; other modes ignore it. */
    public static List<Key> keys(float width,boolean landscape,Mode mode,boolean chinese) {
        if(width<=0) throw new IllegalArgumentException("Keyboard width must be positive");
        List<Key> result=new ArrayList<>(); float row=height(landscape)/4f;
        float gap=landscape?5:6,rowGap=landscape?5:10,capHeight=(height(landscape)-3*rowGap)/4f;
        if(mode==Mode.NINE_KEY) {
            float unit=width/5,capUnit=(width-4*gap)/5;
            nine(result,Action.PUNCTUATION,"",0,0,1,1,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.SEPARATOR,"",1,0,1,1,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.DELETE,"",4,0,1,2,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.NUMBERS,"",0,1,1,1,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.SPELLING,"",0,2,1,1,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.RETURN,"",4,2,1,2,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.LANGUAGE,"",0,3,1,1,unit,row,capUnit,capHeight,gap,rowGap);
            nine(result,Action.SPACE,"",1,3,3,1,unit,row,capUnit,capHeight,gap,rowGap);
            for(int digit=2;digit<=9;digit++) nine(result,Action.TEXT,String.valueOf(digit),
                    (digit-1)%3+1,(digit-1)/3,1,1,unit,row,capUnit,capHeight,gap,rowGap);
        } else {
            float unit=width/10,capUnit=(width-9*gap)/10;
            if(mode==Mode.EMOJI) {
                for(int i=0;i<EMOJIS.length;i++) letter(result,EMOJIS[i],i%10,i/10,unit,row,capUnit,capHeight,gap,rowGap);
            } else {
                String[] rows=mode==Mode.QWERTY?new String[]{"qwertyuiop","asdfghjkl","zxcvbnm"}
                        :PunctuationLayout.rows(mode==Mode.SYMBOLS,chinese);
                for(int r=0;r<3;r++) {
                    String letters=rows[r]; float start=(10-letters.length())/2f;
                    for(int i=0;i<letters.length();i++) letter(result,letters.substring(i,i+1),start+i,r,unit,row,capUnit,capHeight,gap,rowGap);
                }
                float sideWidth=Math.max(capUnit,width*0.115f);
                add(result,mode==Mode.QWERTY?Action.SHIFT:Action.SYMBOLS,"",0,2*row,1.3f*unit,row,
                        0,2*(capHeight+rowGap),sideWidth,capHeight);
                float deleteWidth=Math.max(sideWidth,width*0.14f);
                add(result,Action.DELETE,"",8.5f*unit,2*row,1.5f*unit,row,
                        width-deleteWidth,2*(capHeight+rowGap),deleteWidth,capHeight);
            }
            Action[] footer=mode==Mode.EMOJI
                    ?new Action[]{Action.NUMBERS,Action.EMOJI,Action.LANGUAGE,Action.SPACE,Action.DELETE}
                    :mode==Mode.QWERTY?new Action[]{Action.NUMBERS,Action.EMOJI,Action.PUNCTUATION,Action.SPACE,Action.PUNCTUATION,Action.LANGUAGE,Action.RETURN}
                    :new Action[]{Action.NUMBERS,Action.EMOJI,Action.LANGUAGE,Action.SPACE,Action.RETURN};
            float[] weights=mode==Mode.QWERTY?new float[]{1.2f,1,1,3.6f,1,1,1.2f}:new float[]{1,1,1,4.8f,2.2f};
            float x=0,footerUnit=(width-(footer.length-1)*gap)/10;
            for(int i=0;i<footer.length;i++) {
                float w=footerUnit*weights[i];
                float left=i==0?0:x-gap/2,right=i==footer.length-1?width:x+w+gap/2;
                String text=footer[i]==Action.PUNCTUATION?(i==2?",":"."):"";
                add(result,footer[i],text,left,3*row,right-left,row,x,3*(capHeight+rowGap),w,capHeight);
                x+=w+gap;
            }
        }
        return Collections.unmodifiableList(result);
    }
    private static void letter(List<Key> keys,String text,float column,int r,float unit,float row,
            float capUnit,float capHeight,float gap,float rowGap) {
        add(keys,Action.TEXT,text,column*unit,r*row,unit,row,
                column*(capUnit+gap),r*(capHeight+rowGap),capUnit,capHeight);
    }
    private static void nine(List<Key> keys,Action action,String text,int c,int r,int columns,int rows,
            float unit,float row,float capUnit,float capHeight,float gap,float rowGap) {
        add(keys,action,text,c*unit,r*row,columns*unit,rows*row,
                c*(capUnit+gap),r*(capHeight+rowGap),columns*capUnit+(columns-1)*gap,
                rows*capHeight+(rows-1)*rowGap);
    }
    private static void add(List<Key> keys,Action action,String text,float x,float y,float w,float h,
            float capX,float capY,float capW,float capH) {
        keys.add(new Key(action,text,x,y,w,h,capX,capY,capW,capH));
    }
}
