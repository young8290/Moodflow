// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime.keyboard;

/**
 * Keyboard palettes. Values mirror XSIME's KeyboardTheme (itself mirroring iOS
 * Support/KeyboardTheme.swift); no image assets or animation runtime.
 *
 * <p>Moodflow: moved out of the Android app so the palette math is JVM-testable — the caller
 * passes "dark" instead of a Context, and HSV conversion is done here instead of
 * android.graphics.Color.
 */
public final class KeyboardTheme {
    public static final KeyboardTheme[] ALL={
        new KeyboardTheme("apple","原生","◐",0xFFD1D3D9,0xFFFFFFFF,0xFFABB0BA,0xFF007AFF,0xFF1F1F1F,0xFF6E6E6E,0xFF404040,0xFF0A84FF),
        new KeyboardTheme("rhino","犀牛","🦏",0xFFD8D8D1,0xFFFFF9EB,0xFFB8BBB9,0xFFC65316,0xFF292A28,0xFF46463F,0xFF5C5E59,0xFFF49A53),
        new KeyboardTheme("crab","寄居蟹","🦀",0xFFEADDD1,0xFFFFF7EB,0xFFDEC1A7,0xFFBE4C3D,0xFF312925,0xFF504039,0xFF655045,0xFFF58E72),
        new KeyboardTheme("kitten","小猫","🐱",0xFFF1DFC9,0xFFFFFAF1,0xFFEBC797,0xFFA45B1E,0xFF30271F,0xFF514131,0xFF66503A,0xFFF5B968),
        new KeyboardTheme("puppy","小狗","🐶",0xFFE6D9CC,0xFFFFF9F1,0xFFD8BB99,0xFF825734,0xFF2E2722,0xFF4F4136,0xFF64503F,0xFFDCB384),
        new KeyboardTheme("piglet","小猪","🐷",0xFFF0DCE3,0xFFFFF8FA,0xFFE6B6C5,0xFFAC4469,0xFF32252B,0xFF533B45,0xFF684552,0xFFF0A0BC),
        new KeyboardTheme("noto-1f415","狗狗","🐕",0xFFE9DED0,0xFFFFF9EF,0xFFD9C29F,0xFF93602D,0xFF2F2820,0xFF514335,0xFF67533D,0xFFEABC7A),
        new KeyboardTheme("noto-1f429","贵宾犬","🐩",0xFFDCDDDD,0xFFFAFBFC,0xFFBEC3C7,0xFF626A72,0xFF272A2D,0xFF42474C,0xFF555D64,0xFFC8D1D8),
        new KeyboardTheme("noto-1f416","猪","🐖",0xFFF2DECF,0xFFFFF9F1,0xFFECC0A7,0xFFBF5A62,0xFF322824,0xFF544039,0xFF695147,0xFFF8BDA4),
        new KeyboardTheme("noto-1f407","兔子","🐇",0xFFE2DFE4,0xFFFFFAFD,0xFFD8C7D2,0xFF9D557E,0xFF2B272E,0xFF48414D,0xFF5E4F60,0xFFEDB4D5),
        new KeyboardTheme("noto-1f980","螃蟹","🦀",0xFFF1DFC8,0xFFFFFAEE,0xFFF0C482,0xFFC45F0A,0xFF32291F,0xFF55422E,0xFF6B5437,0xFFFFA32B),
        new KeyboardTheme("noto-1f427","企鹅","🐧",0xFFDEDFDC,0xFFFFFFF8,0xFFBFC2BF,0xFFEBAE32,0xFF262827,0xFF424643,0xFF555B56,0xFFF4C458),
        new KeyboardTheme("noto-1f98a","狐狸","🦊",0xFFF0DAC8,0xFFFFFAF1,0xFFE9B98F,0xFFBD531A,0xFF32251D,0xFF543E2E,0xFF6B4D36,0xFFFFA159),
        new KeyboardTheme("noto-1f43c","熊猫","🐼",0xFFDEDEDC,0xFFFCFCFA,0xFFC1C2C0,0xFF414442,0xFF242625,0xFF414543,0xFF535956,0xFFD0D3CF),
        new KeyboardTheme("noto-1f422","乌龟","🐢",0xFFDFE6CE,0xFFFCFDEC,0xFFC4CF9D,0xFF667628,0xFF282D1F,0xFF434B31,0xFF57613D,0xFFC4D379),
        new KeyboardTheme("noto-1f419","章鱼","🐙",0xFFF0DAD7,0xFFFFF8F4,0xFFEEB4AB,0xFFD6504F,0xFF332524,0xFF553B38,0xFF6B4B45,0xFFFF9490),
        new KeyboardTheme("noto-1f438","青蛙","🐸",0xFFE1E8C8,0xFFFBFFE9,0xFFCAD995,0xFFAAC921,0xFF282F1E,0xFF444E30,0xFF58623B,0xFFC1DE3A),
        new KeyboardTheme("noto-1f423","小鸡","🐣",0xFFEFE6CD,0xFFFFFCED,0xFFE7D49A,0xFFF1C232,0xFF302B1D,0xFF514832,0xFF665939,0xFFF7D368)
    };
    public final String id,title,glyph;
    private final Palette lightPalette,darkPalette;
    KeyboardTheme(String id,String title,String glyph,int... colors) {
        this.id=id; this.title=title; this.glyph=glyph;
        lightPalette=new Palette(colors[0],colors[1],colors[2],colors[3],0xFF000000,id.equals("apple"),false);
        darkPalette=new Palette(colors[4],colors[5],colors[6],colors[7],0xFFFFFFFF,id.equals("apple"),true);
    }
    public static KeyboardTheme named(String id) { for(KeyboardTheme theme:ALL) if(theme.id.equals(id)) return theme; return ALL[0]; }
    /** The pet and its keyboard palette are one choice, in the chooser's order. */
    public static KeyboardTheme next(String id) {
        KeyboardTheme current=named(id);
        for(int i=0;i<ALL.length;i++) if(ALL[i]==current) return ALL[(i+1)%ALL.length];
        return ALL[0];
    }
    public Palette palette(boolean dark) { return dark?darkPalette:lightPalette; }
    public static final class Palette {
        public final int background,key,functional,accent,ink,accentInk,accentText,pressedSelected,pressedSelectedInk;
        public final boolean system,dark;
        Palette(int background,int key,int functional,int accent,int ink,boolean system,boolean dark) {
            this.background=background; this.key=key; this.functional=functional; this.accent=accent; this.ink=ink; this.system=system; this.dark=dark;
            accentInk=system?0xFFFFFFFF:contrastingInk(accent);
            float[] hsv=new float[3]; colorToHsv(accent,hsv); hsv[2]*=0.78f;
            pressedSelected=hsvToColor(hsv); pressedSelectedInk=contrastingInk(pressedSelected);
            accentText=contrast(accent,background)>=4.5?accent:ink;
        }
        private static int contrastingInk(int color) { return luminance(color)>0.179?0xFF000000:0xFFFFFFFF; }
        private static double luminance(int color) {
            double value=0; double[] weights={0.2126,0.7152,0.0722};
            for(int i=0;i<3;i++) { double channel=((color>>(16-i*8))&255)/255.0; value+=weights[i]*(channel<=0.04045?channel/12.92:Math.pow((channel+0.055)/1.055,2.4)); }
            return value;
        }
        static double contrast(int a,int b) { double x=luminance(a),y=luminance(b); return (Math.max(x,y)+0.05)/(Math.min(x,y)+0.05); }
    }
    /** Same convention as android.graphics.Color.colorToHSV: h in [0,360), s and v in [0,1]. */
    static void colorToHsv(int color,float[] hsv) {
        float r=((color>>16)&255)/255f,g=((color>>8)&255)/255f,b=(color&255)/255f;
        float max=Math.max(r,Math.max(g,b)),min=Math.min(r,Math.min(g,b)),delta=max-min;
        float h;
        if(delta==0) h=0;
        else if(max==r) h=60*(((g-b)/delta)%6);
        else if(max==g) h=60*((b-r)/delta+2);
        else h=60*((r-g)/delta+4);
        if(h<0) h+=360;
        hsv[0]=h; hsv[1]=max==0?0:delta/max; hsv[2]=max;
    }
    /** Opaque color from HSV (inverse of {@link #colorToHsv}). */
    static int hsvToColor(float[] hsv) {
        float h=hsv[0],s=hsv[1],v=hsv[2],c=v*s,x=c*(1-Math.abs((h/60)%2-1)),m=v-c;
        float r,g,b;
        if(h<60) { r=c; g=x; b=0; } else if(h<120) { r=x; g=c; b=0; } else if(h<180) { r=0; g=c; b=x; }
        else if(h<240) { r=0; g=x; b=c; } else if(h<300) { r=x; g=0; b=c; } else { r=c; g=0; b=x; }
        return 0xFF000000|(Math.round((r+m)*255)<<16)|(Math.round((g+m)*255)<<8)|Math.round((b+m)*255);
    }
}
