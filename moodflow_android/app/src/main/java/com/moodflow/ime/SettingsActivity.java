// Portions copyright XSIME authors (https://github.com/scholay/xsime), Apache-2.0. Modified for Moodflow.
package com.moodflow.ime;

import android.app.Activity;
import android.content.Intent;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.os.Build;
import android.os.Bundle;
import android.os.SystemClock;
import android.provider.Settings;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.Gravity;
import android.view.View;
import android.view.WindowInsets;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;
import android.widget.Toast;
import com.moodflow.ime.core.ChordProfiles;
import com.moodflow.ime.core.CoreHolder;
import com.moodflow.ime.core.MoodflowCore;
import com.moodflow.ime.core.NativePractice;
import com.moodflow.ime.keyboard.ChordProfile;
import com.moodflow.ime.keyboard.KeyboardTheme;
import com.moodflow.ime.logic.ConfigSnapshot;
import com.moodflow.ime.logic.KeyboardChoice;
import com.moodflow.ime.logic.PluginInfo;
import com.moodflow.ime.logic.PracticeScore;
import com.moodflow.ime.logic.SchemaEntry;
import com.moodflow.ime.logic.Wire;
import com.moodflow.ime.ui.KeyboardIcon;
import com.moodflow.ime.ui.KeyboardSettings;
import com.moodflow.ime.ui.SettingsKeyboardPreview;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * The launcher entry and Android's IME settings entry: grouped navigation ported from XSIME's
 * SetupActivity (welcome / getting started / try typing / typing settings / data &amp; privacy /
 * licenses).
 *
 * <p>Moodflow: settings map to the core's config keys through the shared core
 * ({@link CoreHolder}) — scheme = {@code schema.active} (selected via the core), clipboard =
 * {@code clipboard.enabled}, plugins = {@code plugins.enabled} / {@code plugins.safe_mode}; layout
 * and palette are host preferences ({@link KeyboardSettings}). The typing speed test scores with the
 * core's {@code moodflow-typing-practice} rules (same as the Hub). XSIME's AI / translation / plugin
 * store / community pages are not ported.
 */
public final class SettingsActivity extends Activity {
    private KeyboardSettings settings;
    private MoodflowCore core;
    private boolean coreReady;
    private ChordProfile chordProfile;
    private LinearLayout content;
    private String page="home", license;
    private String renderedPage;
    private ScrollView currentScroll;
    private final ArrayList<String> navigation=new ArrayList<>();
    private final java.util.HashMap<String,Integer> scrollPositions=new java.util.HashMap<>();
    private int background,card,ink,secondary,accent,separator;
    private final ExecutorService documents=Executors.newSingleThreadExecutor();
    private long documentGeneration;
    private boolean destroyed,resumed;
    // Typing speed test (in memory only; nothing is saved or logged).
    private NativePractice practice;
    private long practiceStart=-1;
    private int passage;
    private PracticeScore lastScore;

    /** Same passages as the Hub's typing speed test (moodflow_hub/src/windows/dashboard/practice.ts). */
    private static final String[] PASSAGES={
        "早上推开窗，风里带着一点桂花的味道。楼下的早点铺已经排起了队，蒸笼里冒出的白气把半条街都染得暖暖的。",
        "周末去了一趟城郊的湖边，水面很静，偶尔有几只白鹭贴着水飞过去。坐在长椅上发了一会儿呆，心里慢慢松了下来。",
        "下班路上下起了小雨，便利店门口挤满了躲雨的人。有人递给我一把旧伞，说反正他家就在前面，明天放回店里就好。",
        "学一样新东西，开头总是最难的。把它拆成每天十分钟的小事，坚持一个月再回头看，往往会发现自己已经走了很远。",
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        settings=new KeyboardSettings(this);
        if(state!=null) { page=state.getString("settings.page","home"); license=state.getString("settings.license");
            ArrayList<String> saved=state.getStringArrayList("settings.navigation"); if(saved!=null) navigation.addAll(saved); }
        getWindow().setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
        if(Build.VERSION.SDK_INT>=33) getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,this::goBack);
        chordProfile=ChordProfiles.INSTANCE.builtin();
        CoreHolder.INSTANCE.acquire(this,c -> {
            if(destroyed) return kotlin.Unit.INSTANCE;
            core=c; coreReady=true;
            if(!page.equals("playground") && !page.equals("practice")) render();
            return kotlin.Unit.INSTANCE;
        });
        render();
    }
    @Override protected void onResume() {
        super.onResume();
        // Returning from the system IME picker must not discard a playground draft.
        if(resumed && content!=null && !page.equals("playground") && !page.equals("practice")) render();
        resumed=true;
    }
    @Override protected void onSaveInstanceState(Bundle state) {
        super.onSaveInstanceState(state);
        state.putString("settings.page",page); state.putString("settings.license",license);
        state.putStringArrayList("settings.navigation",new ArrayList<>(navigation));
        // Typing fields deliberately have no saved state or preference entry.
    }
    // API 33+ uses the native dispatcher registered above; retain this hook for API 26–32.
    @SuppressWarnings("deprecation")
    @Override public void onBackPressed() { goBack(); }
    @Override protected void onDestroy() {
        destroyed=true; documentGeneration++; documents.shutdownNow(); closePractice();
        core=null; CoreHolder.INSTANCE.release(); super.onDestroy();
    }
    private void goBack() {
        if(page.equals("home")) { finish(); return; }
        String destination=navigation.isEmpty()?"home":navigation.remove(navigation.size()-1);
        show(destination);
    }
    private void navigate(String destination) {
        navigation.add(page); show(destination);
    }
    private void show(String destination) {
        InputMethodManager imm=getSystemService(InputMethodManager.class);
        if(imm!=null) imm.hideSoftInputFromWindow(getWindow().getDecorView().getWindowToken(),0);
        if(currentScroll!=null) scrollPositions.put(page,currentScroll.getScrollY());
        if(page.equals("practice")) closePractice();
        page=destination; render();
    }
    private int dp(float value) { return Math.round(value*getResources().getDisplayMetrics().density); }
    private LinearLayout column() {
        LinearLayout view=new LinearLayout(this); view.setOrientation(LinearLayout.VERTICAL); return view;
    }
    private GradientDrawable shape(int color,float radius) {
        GradientDrawable value=new GradientDrawable(); value.setColor(color); value.setCornerRadius(dp(radius)); return value;
    }
    private TextView text(String value,float size,int color,boolean bold) {
        TextView view=new TextView(this); view.setText(value); view.setTextSize(size); view.setTextColor(color);
        if(bold) view.setTypeface(Typeface.create("sans-serif",Typeface.BOLD)); return view;
    }
    private void render() {
        if(destroyed) return;
        documentGeneration++;
        Integer savedScroll=scrollPositions.get(page);
        int previousScroll=page.equals(renderedPage) && currentScroll!=null?currentScroll.getScrollY():savedScroll==null?0:savedScroll;
        renderedPage=page;
        boolean dark=(getResources().getConfiguration().uiMode&Configuration.UI_MODE_NIGHT_MASK)==Configuration.UI_MODE_NIGHT_YES;
        background=dark?0xFF000000:0xFFF2F2F7; card=dark?0xFF1C1C1E:0xFFFFFFFF;
        ink=dark?0xFFF5F5F7:0xFF111113; secondary=dark?0xFF9C9CA3:0xFF76767D;
        accent=dark?0xFF7EB6FF:0xFF2F6FD0; separator=dark?0xFF38383A:0xFFE5E5EA;
        content=column(); content.setTag("settings.page."+page);
        content.setPadding(dp(16),dp(8),dp(16),dp(24)); content.setFocusableInTouchMode(true);
        if(!page.equals("home")) {
            LinearLayout back=new LinearLayout(this); back.setOrientation(LinearLayout.HORIZONTAL); back.setGravity(Gravity.CENTER_VERTICAL);
            back.addView(icon(KeyboardIcon.CHEVRON_LEFT,accent,20));
            TextView name=text(titleFor(navigation.isEmpty()?"home":navigation.get(navigation.size()-1)),17,accent,false); name.setPadding(dp(4),0,0,0); back.addView(name);
            back.setContentDescription("返回："+name.getText()); back.setMinimumHeight(dp(44));
            clickable(back,this::goBack); content.addView(back);
        }
        TextView title=text(titleFor(page),page.equals("home")?34:22,ink,true); title.setPadding(0,dp(12),0,dp(16)); content.addView(title);
        switch(page) {
            case "home":home();break;case "setup":setup();break;case "playground":playground();break;
            case "schema":schemas();break;case "chords":chords();break;case "appearance":appearance();break;
            case "mappings":mappings();break;case "plugins":plugins();break;case "practice":practice();break;
            case "data":data();break;case "privacy":privacy();break;case "licenses":licenses();break;
            case "license":document();break;
            default:page="home";render();return;
        }
        ScrollView scroll=new ScrollView(this); currentScroll=scroll; scroll.setFillViewport(true); scroll.setVerticalScrollBarEnabled(false);
        scroll.addView(content,new ScrollView.LayoutParams(-1,-2));
        FrameLayout frame=new FrameLayout(this); frame.setBackgroundColor(background); frame.addView(scroll,new FrameLayout.LayoutParams(-1,-1));
        frame.setOnApplyWindowInsetsListener((view,insets) -> {
            if(Build.VERSION.SDK_INT>=30) {
                android.graphics.Insets safe=insets.getInsets(WindowInsets.Type.systemBars()|WindowInsets.Type.displayCutout()|WindowInsets.Type.ime());
                view.setPadding(safe.left,safe.top,safe.right,safe.bottom);
            } else view.setPadding(insets.getSystemWindowInsetLeft(),insets.getSystemWindowInsetTop(),insets.getSystemWindowInsetRight(),insets.getSystemWindowInsetBottom());
            return insets;
        });
        setContentView(frame); content.requestFocus();
        if(previousScroll>0) scroll.post(() -> scroll.scrollTo(0,previousScroll));
    }
    private String titleFor(String destination) {
        switch(destination) {
            case "setup":return "启用 Moodflow 键盘";case "playground":return "输入体验";
            case "schema":return "输入方案";case "chords":return "滑动并击与键位";
            case "mappings":return "内置并击映射";case "plugins":return "插件";
            case "appearance":return "键盘布局与配色";case "practice":return "打字测速";
            case "data":return "数据管理";case "privacy":return "隐私";
            case "licenses":return "第三方许可";case "license":return license==null?"许可":license;
            default:return "欢迎";
        }
    }
    private LinearLayout group(String heading) {
        TextView label=text(heading,13,secondary,false); label.setPadding(dp(16),dp(24),dp(16),dp(8)); content.addView(label);
        LinearLayout group=column(); group.setBackground(shape(card,24)); group.setClipToOutline(true);
        content.addView(group,new LinearLayout.LayoutParams(-1,-2)); return group;
    }
    private void divider(LinearLayout group) {
        if(group.getChildCount()==0) return;
        View line=new View(this); line.setBackgroundColor(separator);
        LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(-1,Math.max(1,dp(0.5f)));
        frame.setMarginStart(dp(56)); frame.setMarginEnd(dp(16)); group.addView(line,frame);
    }
    private void clickable(View view,Runnable action) {
        view.setFocusable(true); view.setClickable(true);
        view.setForeground(new RippleDrawable(ColorStateList.valueOf(0x22888888),null,shape(0xFFFFFFFF,0)));
        view.setOnClickListener(v -> action.run());
        view.setAccessibilityDelegate(new View.AccessibilityDelegate() {
            @Override public void onInitializeAccessibilityNodeInfo(View host,android.view.accessibility.AccessibilityNodeInfo info) {
                super.onInitializeAccessibilityNodeInfo(host,info); info.setClassName(android.widget.Button.class.getName());
            }
        });
    }
    private ImageView icon(KeyboardIcon value,int tint,int size) {
        ImageView view=new ImageView(this); view.setImageDrawable(value.drawable(this)); view.setImageTintList(ColorStateList.valueOf(tint));
        view.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO); view.setLayoutParams(new LinearLayout.LayoutParams(dp(size),dp(size))); return view;
    }
    private LinearLayout row(LinearLayout group,KeyboardIcon image,String label,String detail,String value,Runnable action) {
        divider(group); LinearLayout row=new LinearLayout(this); row.setOrientation(LinearLayout.HORIZONTAL); row.setGravity(Gravity.CENTER_VERTICAL);
        row.setMinimumHeight(dp(52)); row.setPadding(dp(16),dp(12),dp(16),dp(12));
        if(image!=null) { ImageView symbol=icon(image,accent,24); ((LinearLayout.LayoutParams)symbol.getLayoutParams()).setMarginEnd(dp(16)); row.addView(symbol); }
        LinearLayout labels=column(); labels.addView(text(label,17,ink,false));
        if(detail!=null) { TextView caption=text(detail,13,secondary,false); caption.setPadding(0,dp(4),0,0); labels.addView(caption); }
        row.addView(labels,new LinearLayout.LayoutParams(0,-2,1));
        if(value!=null) { TextView trailing=text(value,15,accent,false); trailing.setPadding(dp(8),0,dp(6),0); trailing.setMaxWidth(dp(130)); row.addView(trailing); }
        if(action!=null) { row.addView(icon(KeyboardIcon.CHEVRON_RIGHT,secondary,16)); clickable(row,action); }
        row.setContentDescription(label+(detail==null?"":", "+detail)+(value==null?"":", "+value));
        group.addView(row,new LinearLayout.LayoutParams(-1,-2)); return row;
    }
    private void note(String value) {
        TextView view=text(value,13,secondary,false); view.setLineSpacing(dp(3),1); view.setPadding(dp(16),dp(8),dp(16),0); content.addView(view);
    }
    private void paragraph(LinearLayout group,String value) {
        TextView view=text(value,16,ink,false); view.setLineSpacing(dp(4),1); view.setPadding(dp(16),dp(16),dp(16),dp(16)); group.addView(view);
    }
    private void toggle(LinearLayout group,String label,boolean checked,java.util.function.Consumer<Boolean> save) {
        divider(group); Switch toggle=new Switch(this); toggle.setText(label); toggle.setTextSize(17); toggle.setTextColor(ink); toggle.setPadding(dp(16),dp(12),dp(16),dp(12));
        toggle.setMinHeight(dp(52)); toggle.setChecked(checked);
        toggle.setThumbTintList(new ColorStateList(new int[][]{{android.R.attr.state_checked},{}},new int[]{accent,secondary}));
        toggle.setOnCheckedChangeListener((button,value) -> save.accept(value)); group.addView(toggle,new LinearLayout.LayoutParams(-1,-2));
    }
    private void choice(LinearLayout group,String label,String detail,boolean selected,Runnable action) {
        LinearLayout row=row(group,null,label,detail,null,null); clickable(row,action); row.setSelected(selected);
        row.setContentDescription(label+(selected?"，已选择":"")+(detail==null?"":", "+detail));
        if(selected) row.addView(icon(KeyboardIcon.CHECK,accent,20));
    }
    private void segments(LinearLayout parent,String[] ids,String[] labels,String selected,java.util.function.Consumer<String> action) {
        LinearLayout bar=new LinearLayout(this); bar.setOrientation(LinearLayout.HORIZONTAL); bar.setPadding(dp(3),dp(3),dp(3),dp(3)); bar.setBackground(shape(separator,9));
        for(int i=0;i<ids.length;i++) {
            String id=ids[i]; boolean active=id.equals(selected); TextView button=text(labels[i],14,active?ink:secondary,active);
            button.setGravity(Gravity.CENTER); button.setMinHeight(dp(44)); button.setPadding(dp(4),dp(4),dp(4),dp(4));
            button.setBackground(shape(active?card:android.graphics.Color.TRANSPARENT,7)); button.setSelected(active);
            button.setContentDescription(labels[i]+(active?"，已选择":"")); clickable(button,() -> action.accept(id));
            bar.addView(button,new LinearLayout.LayoutParams(0,-2,1));
        }
        LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(-1,-2); frame.setMargins(dp(16),dp(12),dp(16),dp(12)); parent.addView(bar,frame);
    }
    private void preview(LinearLayout group) {
        SettingsKeyboardPreview preview=new SettingsKeyboardPreview(this,settings.getLayout(),KeyboardTheme.named(settings.getTheme()),chordProfile);
        LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(-1,-2); frame.setMargins(dp(12),dp(4),dp(12),dp(12)); group.addView(preview,frame);
    }
    /** Core-backed pages show this until the shared core is ready (or unavailable). */
    private boolean needCore() {
        if(core!=null) return true;
        note(coreReady?getString(R.string.engine_failed):getString(R.string.engine_loading));
        return false;
    }
    private void toast(String message) { Toast.makeText(this,message,Toast.LENGTH_SHORT).show(); }
    /** Writes one config key to the user layer, then reloads the core so the keyboard picks it up. */
    private boolean setConfig(String key,MoodflowCore.ConfigType type,String value) {
        if(core==null) return false;
        String error=core.configSet(key,type,value);
        if(error!=null) { Toast.makeText(this,error,Toast.LENGTH_LONG).show(); return false; }
        core.runCommand(MoodflowCore.Command.RELOAD_CONFIG,0,"");
        return true;
    }
    private ConfigSnapshot snapshot() { return new ConfigSnapshot(core==null?"":core.configSnapshot()); }
    private String layoutName(String id) {
        switch(id) { case KeyboardChoice.NINE_KEY:return "九键全拼";case KeyboardChoice.ORTHOGONAL:return "并击矩阵";case KeyboardChoice.SPLIT_ORTHOGONAL:return "分离并击";default:return "QWERTY"; }
    }
    private String activeSchemaName() {
        if(core==null) return null;
        for(SchemaEntry entry:core.schemas()) if(entry.getActive()) return entry.getName();
        return null;
    }

    private void home() {
        LinearLayout hero=new LinearLayout(this); hero.setOrientation(LinearLayout.VERTICAL);
        hero.setPadding(dp(16),dp(16),dp(16),dp(16)); hero.setBackground(shape((accent&0x00FFFFFF)|0x14000000,24));
        TextView brand=text(getString(R.string.app_name),28,ink,true); hero.addView(brand);
        TextView line=text("懂你心情的输入法：拼音、五笔、九宫格、并击，词库都在本机。",13,secondary,false); line.setPadding(0,dp(6),0,0); hero.addView(line);
        content.addView(hero);
        LinearLayout start=group("开始使用");
        row(start,KeyboardIcon.KEYBOARD,"启用 Moodflow 键盘",null,null,() -> navigate("setup"));
        row(start,KeyboardIcon.WRITE,"输入体验",null,null,() -> navigate("playground"));
        row(start,KeyboardIcon.PLAY,"打字测速",null,null,() -> navigate("practice"));
        LinearLayout typing=group("你的输入方式");
        row(typing,KeyboardIcon.BOOK,"输入方案",null,activeSchemaName(),() -> navigate("schema"));
        row(typing,KeyboardIcon.KEYBOARD,"键盘布局与配色",null,layoutName(settings.getLayout()),() -> navigate("appearance"));
        row(typing,KeyboardIcon.SLIDERS,"滑动并击与键位",null,null,() -> navigate("chords"));
        row(typing,KeyboardIcon.GRID_9,"插件",null,null,() -> navigate("plugins"));
        LinearLayout more=group("数据与隐私");
        row(more,KeyboardIcon.STACK_LAYERS,"数据管理","剪贴板历史、用户词库",null,() -> navigate("data"));
        row(more,KeyboardIcon.BOOK,"隐私",null,null,() -> navigate("privacy"));
        row(more,KeyboardIcon.BOOK,"第三方许可",null,null,() -> navigate("licenses"));
        String version;
        try { version=getPackageManager().getPackageInfo(getPackageName(),0).versionName; }
        catch(android.content.pm.PackageManager.NameNotFoundException error) { version="开发版"; }
        note(getString(R.string.app_name)+"  "+version);
    }
    private void setup() {
        LinearLayout status=group("键盘状态"); InputMethodManager manager=getSystemService(InputMethodManager.class);
        boolean enabled=false;
        if(manager!=null) for(InputMethodInfo method:manager.getEnabledInputMethodList()) enabled|=method.getPackageName().equals(getPackageName());
        String chosen=Settings.Secure.getString(getContentResolver(),Settings.Secure.DEFAULT_INPUT_METHOD);
        row(status,KeyboardIcon.KEYBOARD,"Moodflow 键盘",null,enabled?"已启用":"待启用",null);
        row(status,KeyboardIcon.CHECK,"当前输入法",null,chosen!=null && chosen.startsWith(getPackageName()+"/")?getString(R.string.ime_name):"其他键盘",null);
        LinearLayout steps=group("开始使用");
        row(steps,KeyboardIcon.SETTINGS,"1. 启用键盘","打开系统输入法设置，启用 Moodflow。",null,() -> startActivity(new Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)));
        row(steps,KeyboardIcon.GLOBE,"2. 选择键盘","在输入法列表中选择 Moodflow。",null,() -> { if(manager!=null) manager.showInputMethodPicker(); });
        row(steps,KeyboardIcon.KEYBOARD,"3. 试打",null,null,() -> navigate("playground"));
        note("拼音、五笔与英文词库随应用安装，首次使用无需联网下载；中文引擎准备好之前按键直接上屏。");
    }
    private void playground() {
        note("试打、切换输入框，或打开 Buffer 体验确认后发送。这里的内容不会保存。");
        String[] hints={"在此试打","第二个输入框：测试切换目标","密码框：不经引擎、Buffer 关闭","私密输入框：不学习、不记录剪贴板"};
        String[] headings={"试打","第二个输入框","密码输入","私密输入"};
        for(int i=0;i<hints.length;i++) {
            LinearLayout group=group(headings[i]); EditText input=new EditText(this); input.setHint(hints[i]);
            input.setInputType(InputType.TYPE_CLASS_TEXT|(i==2?InputType.TYPE_TEXT_VARIATION_PASSWORD:InputType.TYPE_TEXT_FLAG_MULTI_LINE));
            if(i==3) input.setImeOptions(EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING);
            input.setSaveEnabled(false); input.setImportantForAutofill(View.IMPORTANT_FOR_AUTOFILL_NO);
            input.setTextColor(ink); input.setHintTextColor(secondary); input.setTextSize(17); input.setGravity(Gravity.TOP|Gravity.START);
            input.setPadding(dp(16),dp(16),dp(16),dp(16)); input.setMinHeight(dp(i==0?170:64)); input.setBackgroundColor(android.graphics.Color.TRANSPARENT);
            group.addView(input,new LinearLayout.LayoutParams(-1,-2));
        }
        note(getString(R.string.privacy));
    }
    private void schemas() {
        if(!needCore()) return;
        LinearLayout choices=group("中文输入方案");
        List<SchemaEntry> list=core.schemas();
        for(int i=0;i<list.size();i++) {
            SchemaEntry entry=list.get(i); final int index=i;
            choice(choices,entry.getName(),entry.getId(),entry.getActive(),() -> {
                core.runCommand(MoodflowCore.Command.SELECT_SCHEMA,index,"");
                // Nine-key and chords are Pinyin layouts (XSIME KeyboardSettings.setSchema rule).
                if(!entry.getId().equals(KeyboardChoice.PINYIN_SCHEMA) && !settings.getLayout().equals(KeyboardChoice.QWERTY)) settings.setLayout(KeyboardChoice.QWERTY);
                render();
            });
        }
        note("方案列表是本机已安装的方案。九键与并击只配拼音；选择其他方案会回到 26 键。");
    }
    private void appearance() {
        content.addView(text("熟悉的键位，喜欢的外观",20,ink,true));
        note("标准键盘和并击共用配色，按下反馈也跟随配色。");
        LinearLayout standard=group("标准键盘");
        segments(standard,new String[]{KeyboardChoice.QWERTY,KeyboardChoice.NINE_KEY},new String[]{"QWERTY · 26 键","九键全拼"},settings.getLayout(),id -> { settings.setLayout(id); ensurePinyin(id); render(); }); preview(standard);
        note(settings.getLayout().equals(KeyboardChoice.NINE_KEY)?"九键仅用于拼音；英文和数字沿用标准键位。":"当前布局："+layoutName(settings.getLayout()));
        LinearLayout themes=group("键盘配色（18 套）"); paragraph(themes,"当前配色："+KeyboardTheme.named(settings.getTheme()).title);
        boolean dark=(getResources().getConfiguration().uiMode&Configuration.UI_MODE_NIGHT_MASK)==Configuration.UI_MODE_NIGHT_YES;
        for(int start=0;start<KeyboardTheme.ALL.length;start+=3) {
            LinearLayout line=new LinearLayout(this); line.setOrientation(LinearLayout.HORIZONTAL);
            for(int i=start;i<Math.min(start+3,KeyboardTheme.ALL.length);i++) {
                KeyboardTheme theme=KeyboardTheme.ALL[i]; KeyboardTheme.Palette palette=theme.palette(dark); boolean selected=theme.id.equals(settings.getTheme());
                LinearLayout tile=column(); tile.setPadding(dp(9),dp(9),dp(9),dp(9)); tile.setGravity(Gravity.CENTER);
                GradientDrawable tileShape=shape(palette.background,11); if(selected) tileShape.setStroke(dp(2),palette.accent); tile.setBackground(tileShape);
                TextView glyph=text(theme.id.equals("apple")?"●":theme.glyph,28,palette.ink,false); glyph.setGravity(Gravity.CENTER); tile.addView(glyph);
                TextView name=text(theme.title,12,palette.ink,true); name.setGravity(Gravity.CENTER); tile.addView(name);
                LinearLayout swatches=new LinearLayout(this); int[] colors={palette.key,palette.functional,palette.accent};
                for(int c:colors) { View swatch=new View(this); swatch.setBackground(shape(c,3)); LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(0,dp(10),1); frame.setMargins(dp(2),0,dp(2),0); swatches.addView(swatch,frame); }
                LinearLayout.LayoutParams strip=new LinearLayout.LayoutParams(-1,dp(10)); strip.topMargin=dp(7); tile.addView(swatches,strip);
                tile.setSelected(selected); tile.setContentDescription("配色："+theme.title+(selected?"，已选择":"")); clickable(tile,() -> { settings.setTheme(theme.id); render(); });
                LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(0,-2,1); frame.setMargins(i==start?0:dp(8),0,0,0); line.addView(tile,frame);
            }
            LinearLayout.LayoutParams frame=new LinearLayout.LayoutParams(-1,-2); frame.setMargins(dp(12),0,dp(12),dp(10)); themes.addView(line,frame);
        }
        note("在键盘上点候选栏左侧的设置图标，也能随时换配色、布局与方案。");
    }
    /** Nine-key and chord layouts select Pinyin (XSIME: "Nine-key selects Pinyin"). */
    private void ensurePinyin(String layout) {
        if(core==null || layout.equals(KeyboardChoice.QWERTY)) return;
        List<SchemaEntry> list=core.schemas();
        for(int i=0;i<list.size();i++) if(list.get(i).getId().equals(KeyboardChoice.PINYIN_SCHEMA) && !list.get(i).getActive()) {
            core.runCommand(MoodflowCore.Command.SELECT_SCHEMA,i,""); return;
        }
    }
    private void chords() {
        LinearLayout instructions=group("双手滑动并击");
        paragraph(instructions,"每手：起点＋终点。途中经过的键不计入；双手全部松开时提交。划回起点恢复单键。英文态与 Shift 在同一块面板上按单键打字母。");
        if(chordProfile==null) { note("并击方案不可用（原生库未加载）。"); return; }
        LinearLayout layouts=group("键位布局");
        segments(layouts,new String[]{KeyboardChoice.ORTHOGONAL,KeyboardChoice.SPLIT_ORTHOGONAL},new String[]{"并击矩阵","分离并击"},settings.getLayout(),id -> { settings.setLayout(id); ensurePinyin(id); render(); }); preview(layouts);
        row(layouts,KeyboardIcon.KEYBOARD,"返回标准键盘",null,null,() -> { settings.setLayout(KeyboardChoice.QWERTY); render(); });
        LinearLayout profile=group("当前映射"); paragraph(profile,"内置方案 · 全拼输出\n"+chordProfile.entries.size()+" 项映射（XSIME 默认并击方案），由输入法核心结算后送进拼音引擎。");
        row(profile,KeyboardIcon.BOOK,"查看全部映射",null,null,() -> navigate("mappings"));
        for(String keys:new String[]{"dvi","xck","ef","km"}) {
            ChordProfile.Resolution value=chordProfile.resolve(keys);
            if(value!=null) row(profile,null,keys.toUpperCase(java.util.Locale.ROOT),value.preview,null,null);
        }
        note("Android 暂不支持导入或编辑并击映射。");
    }
    private void mappings() {
        if(chordProfile==null) { note("并击方案不可用（原生库未加载）。"); return; }
        note("键组合 → 拼音 / 声韵母片段。内置映射仅供查阅。");
        LinearLayout group=group(chordProfile.entries.size()+" 项内置映射"); StringBuilder table=new StringBuilder();
        for(ChordProfile.Entry entry:chordProfile.entries) table.append(entry.keys).append("  →  ").append(entry.output).append(entry.fragment?"  ·  片段":"").append('\n');
        TextView view=text(table.toString(),16,ink,false); view.setTypeface(Typeface.MONOSPACE); view.setTextIsSelectable(true); view.setPadding(dp(16),dp(16),dp(16),dp(16)); group.addView(view);
    }
    private void plugins() {
        if(!needCore()) return;
        ConfigSnapshot snap=snapshot();
        boolean safe=Boolean.TRUE.equals(snap.bool("plugins.safe_mode"));
        LinearLayout mode=group("安全模式");
        toggle(mode,"安全模式（所有插件都不加载）",safe,value -> { setConfig("plugins.safe_mode",MoodflowCore.ConfigType.BOOL,value?"true":"false"); render(); });
        note("装了某个插件后输入异常时先打开安全模式排查。密码框与私密输入框里插件从不被调用。");
        List<PluginInfo> list=core.plugins();
        if(list.isEmpty()) { note("还没有安装插件。插件装在应用数据目录的 plugins/ 下，与桌面版同一种格式；Android 上的插件市场尚未接入。"); return; }
        for(PluginInfo plugin:list) {
            LinearLayout group=group(plugin.getName()+" · "+plugin.getVersion());
            String state=safe?"安全模式":plugin.getLoaded()?(plugin.getAutoDisabled()?"已自动停用":"已加载"):plugin.getEnabled()?"未能加载":"未启用";
            row(group,KeyboardIcon.GRID_9,plugin.getId(),plugin.getCapabilities().isEmpty()?null:String.join(" · ",plugin.getCapabilities()),state,null);
            toggle(group,"启用",plugin.getEnabled(),value -> { setPluginEnabled(plugin.getId(),value); render(); });
            if(plugin.getAutoDisabled()) row(group,null,"重新启用","连续出错被自动停用",null,() -> { core.pluginReenable(plugin.getId()); render(); });
        }
        note("启用插件后，插件候选从第 2 位起并进候选栏，首选永远是引擎的。");
    }
    private void setPluginEnabled(String id,boolean enabled) {
        List<String> current=snapshot().stringList("plugins.enabled");
        List<String> next=new ArrayList<>(current==null?java.util.Collections.emptyList():current);
        next.remove(id); if(enabled) next.add(id);
        setConfig("plugins.enabled",MoodflowCore.ConfigType.STR_LIST,Wire.INSTANCE.joinList(next));
    }
    private void closePractice() {
        if(practice!=null) { practice.close(); practice=null; }
        practiceStart=-1;
    }
    private void practice() {
        note("照着范文打一段，看有效速度、过程正确率与最终字准。计分规则与 Hub 看板的打字测速相同。这里打的字不保存。");
        String target=PASSAGES[passage%PASSAGES.length];
        LinearLayout passageGroup=group("范文"); paragraph(passageGroup,target);
        LinearLayout typing=group("你的输入");
        EditText input=new EditText(this); input.setHint("从这里开始打字，第一下按键开始计时");
        input.setInputType(InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_FLAG_MULTI_LINE);
        input.setSaveEnabled(false); input.setImportantForAutofill(View.IMPORTANT_FOR_AUTOFILL_NO);
        input.setTextColor(ink); input.setHintTextColor(secondary); input.setTextSize(17); input.setGravity(Gravity.TOP|Gravity.START);
        input.setPadding(dp(16),dp(16),dp(16),dp(16)); input.setMinHeight(dp(120)); input.setBackgroundColor(android.graphics.Color.TRANSPARENT);
        typing.addView(input,new LinearLayout.LayoutParams(-1,-2));
        LinearLayout result=group("成绩");
        TextView score=text(lastScore==null?"打完或点「结束」后出成绩。":describe(lastScore),16,ink,false);
        score.setPadding(dp(16),dp(16),dp(16),dp(16)); score.setAccessibilityLiveRegion(View.ACCESSIBILITY_LIVE_REGION_POLITE); result.addView(score);
        closePractice(); lastScore=null;
        practice=NativePractice.Companion.start(target);
        if(practice==null) { input.setEnabled(false); score.setText("计分需要原生库，当前不可用。"); }
        int targetLength=target.codePointCount(0,target.length());
        Runnable finish=() -> {
            if(practice==null || practiceStart<0) return;
            PracticeScore value=practice.finish(SystemClock.uptimeMillis()-practiceStart);
            closePractice(); lastScore=value;
            input.setEnabled(false);
            score.setText(value==null?"计分失败。":describe(value));
        };
        input.addTextChangedListener(new TextWatcher() {
            @Override public void beforeTextChanged(CharSequence s,int start,int count,int after) {}
            @Override public void onTextChanged(CharSequence s,int start,int before,int count) {}
            @Override public void afterTextChanged(Editable s) {
                if(practice==null) return;
                String text=s.toString();
                if(practiceStart<0) { if(text.isEmpty()) return; practiceStart=SystemClock.uptimeMillis(); }
                practice.onText(SystemClock.uptimeMillis()-practiceStart,text);
                if(text.codePointCount(0,text.length())>=targetLength) finish.run();
            }
        });
        LinearLayout actions=group("操作");
        row(actions,KeyboardIcon.CHECK,"结束",null,null,finish);
        row(actions,KeyboardIcon.PLAY,"换一段",null,null,() -> { passage++; closePractice(); render(); });
    }
    private static String describe(PracticeScore s) {
        return String.format(java.util.Locale.ROOT,"用时 %.1f 秒\n有效速度 %.1f 字/分\n过程正确率 %.1f%%\n最终字准 %.1f%%",
                s.getElapsedMs()/1000.0,s.getEffectiveCpm(),s.getProcessAccuracy()*100,s.getFinalAccuracy()*100);
    }
    private void data() {
        if(!needCore()) return;
        ConfigSnapshot snap=snapshot();
        LinearLayout clipboard=group("剪贴板历史");
        toggle(clipboard,"记录剪贴板历史",Boolean.TRUE.equals(snap.bool("clipboard.enabled")),value -> { setConfig("clipboard.enabled",MoodflowCore.ConfigType.BOOL,value?"true":"false"); render(); });
        row(clipboard,KeyboardIcon.CLEAR,"清空历史（保留收藏）",null,null,() -> { core.clipboardClearUnpinned(); toast("已清空"); });
        paragraph(clipboard,"只在键盘显示时读取剪贴板（Android 10 起系统也只允许前台输入法读取）。密码框、私密输入框和系统标记为敏感的内容不记录。历史与桌面版同一种格式，保存在应用私有目录，不参与系统备份。");
        LinearLayout learning=group("用户词库");
        paragraph(learning,core.userStoreAvailable()?"选词会改善本机词库。私密输入框完全不经引擎，不学习。":"⚠ 用户词库不可写：学到的词不会保存。原库不会被删除或重建。");
    }
    private void privacy() {
        LinearLayout input=group("输入与学习"); paragraph(input,getString(R.string.privacy));
        LinearLayout buffer=group("Buffer"); paragraph(buffer,"Buffer 只保留本次草稿：换输入框、隐藏键盘后清空，关掉 Buffer 不会发送已存的块。投递被拒时保留原文，可再点插入重试。");
        LinearLayout legal=group("开源软件与资源"); row(legal,KeyboardIcon.BOOK,"第三方许可",null,null,() -> navigate("licenses"));
    }
    private void licenses() {
        LinearLayout list=group("随应用提供的许可证");
        try { String[] files=getAssets().list("licenses"); if(files!=null) { java.util.Arrays.sort(files); for(String file:files) row(list,KeyboardIcon.BOOK,file,null,null,() -> openLicense(file)); } }
        catch(java.io.IOException error) { paragraph(list,"许可列表暂不可用，请返回重试。"); }
        note("键盘界面移植自 XSIME（Apache-2.0）并有修改；图标来自 Lucide（ISC）。完整清单见仓库 NOTICE.md。");
    }
    private void openLicense(String name) { license=name; navigate("license"); }
    private void document() {
        LinearLayout group=group("许可文本"); TextView body=text("正在读取…",14,ink,false); body.setTextIsSelectable(true); body.setPadding(dp(16),dp(16),dp(16),dp(16)); group.addView(body);
        long generation=documentGeneration; String filename=license;
        if(filename==null || filename.contains("/") || filename.contains("..")) { body.setText("许可不可用"); return; }
        documents.execute(() -> {
            String value;
            try(InputStream input=getAssets().open("licenses/"+filename);java.io.ByteArrayOutputStream bytes=new java.io.ByteArrayOutputStream()) {
                byte[] block=new byte[8192]; int count; while((count=input.read(block))!=-1) { if(bytes.size()+count>262144) throw new java.io.IOException("License too large"); bytes.write(block,0,count); }
                value=new String(bytes.toByteArray(),StandardCharsets.UTF_8);
            } catch(java.io.IOException error) { value="许可暂不可用，请返回重试。"; }
            String result=value; runOnUiThread(() -> { if(!destroyed && generation==documentGeneration && page.equals("license")) body.setText(result); });
        });
    }
}
