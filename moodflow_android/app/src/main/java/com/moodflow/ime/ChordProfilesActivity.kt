package com.moodflow.ime

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.res.Configuration
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.text.Editable
import android.text.InputFilter
import android.text.InputType
import android.text.TextWatcher
import android.view.View
import android.view.WindowInsets
import android.widget.Button
import android.widget.CheckBox
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import com.moodflow.ime.core.ChordDraftCodec
import com.moodflow.ime.core.ChordProfileStore
import com.moodflow.ime.core.NativeChord
import com.moodflow.ime.logic.ChordDraft
import com.moodflow.ime.logic.ChordKeys
import java.io.IOException
import java.util.concurrent.Executors

/** Android editing surface. A saved draft and the version running in the keyboard are separate. */
class ChordProfilesActivity : Activity() {
    private data class Form(
        var name: String,
        var left: String,
        var right: String,
        var interval: String,
        var rows: List<ChordDraft.Row>,
    ) {
        constructor(draft: ChordDraft) : this(draft.name, draft.left.joinToString(" "),
            draft.right.joinToString(" "), draft.intervalMs.toString(), draft.mappings)

        fun draft(): ChordDraft = ChordDraft(name.trim(), ChordDraft.keys(left), ChordDraft.keys(right),
            interval.toIntOrNull() ?: 0, rows)
    }

    private data class Editor(
        val builtin: ChordDraft,
        var snapshot: ChordProfileStore.Snapshot,
        var baseline: ChordDraft,
        var form: Form,
        var query: String = "",
        var page: Int = 0,
        var export: String? = null,
    ) {
        fun dirty() = form.draft() != baseline
        fun load(draft: ChordDraft) {
            form = Form(draft)
            query = ""
            page = 0
        }
    }

    private val io = Executors.newSingleThreadExecutor()
    private lateinit var store: ChordProfileStore
    private var editor: Editor? = null
    private var destroyed = false
    private var busy = false
    private var message = ""
    private lateinit var root: LinearLayout
    private lateinit var status: TextView
    private lateinit var list: LinearLayout
    private val controls = ArrayList<View>()
    private val rowControls = ArrayList<View>()
    private var textColor = 0
    private var background = 0

    override fun onCreate(state: Bundle?) {
        val dark = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
        if (dark) setTheme(android.R.style.Theme_Material_NoActionBar)
        super.onCreate(state)
        store = ChordProfileStore(this)
        @Suppress("DEPRECATION")
        val retained = lastNonConfigurationInstance as? Editor
        editor = retained
        if (Build.VERSION.SDK_INT >= 33) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(0) { leave() }
        }
        render()
        if (editor == null) reload()
    }

    @Suppress("DEPRECATION")
    override fun onRetainNonConfigurationInstance(): Any? = editor

    @Suppress("DEPRECATION")
    override fun onBackPressed() = leave()

    override fun onDestroy() {
        destroyed = true
        io.shutdown()
        super.onDestroy()
    }

    private fun leave() {
        if (busy) return
        confirmDiscard("放弃未保存的编辑？") { finish() }
    }

    private fun confirmDiscard(title: String, proceed: () -> Unit) {
        if (editor?.dirty() != true) return proceed()
        AlertDialog.Builder(this).setTitle(title)
            .setMessage("已保存的草稿和正在使用的方案会保留。")
            .setNegativeButton("取消", null)
            .setPositiveButton("放弃编辑") { _, _ -> proceed() }.show()
    }

    private fun reload() = runTask("读取方案") {
        val builtin = ChordDraftCodec.decode(NativeChord.builtinJson() ?: error("原生库未加载。"))
        val snapshot = store.read()
        val draft = snapshot.draft?.let { ChordDraftCodec.decode(it) } ?: builtin
        Editor(builtin, snapshot, draft, Form(draft))
    }.then {
        editor = it
        message = ""
        render()
    }

    /** Run file/native work off the editing thread; after Activity destruction never touch views. */
    private fun <T> runTask(label: String, task: () -> T): Task<T> = Task(label, task)

    private inner class Task<T>(val label: String, val task: () -> T) {
        fun then(complete: (T) -> Unit) {
            if (busy) return
            busy = true
            report("正在${label}…")
            setControls()
            io.execute {
                val result = runCatching(task)
                runOnUiThread {
                    if (destroyed) return@runOnUiThread
                    busy = false
                    setControls()
                    result.fold(complete) { e -> report("${label}失败：${e.localizedMessage ?: "请重试。"}") }
                }
            }
        }
    }

    private fun report(value: String) {
        message = value
        if (::status.isInitialized) status.text = value
    }

    private fun setControls() {
        controls.forEach { it.isEnabled = !busy }
        rowControls.forEach { it.isEnabled = !busy }
    }

    private fun dp(n: Int) = (n * resources.displayMetrics.density + 0.5f).toInt()
    private fun column() = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }

    private fun label(parent: LinearLayout, value: String, size: Float = 15f): TextView =
        TextView(this).apply {
            text = value
            textSize = size
            setTextColor(textColor)
            setPadding(dp(4), dp(7), dp(4), dp(7))
            parent.addView(this)
        }

    private fun button(parent: LinearLayout, title: String, controlled: Boolean = true, action: () -> Unit): Button =
        Button(this).apply {
            text = title
            isAllCaps = false
            setOnClickListener { if (!busy) action() }
            isEnabled = !busy
            parent.addView(this, LinearLayout.LayoutParams(-1, -2))
            if (controlled) controls.add(this)
        }

    private fun field(
        parent: LinearLayout, title: String, value: String, limit: Int,
        numeric: Boolean = false, controlled: Boolean = true, changed: (String) -> Unit = {},
    ): EditText {
        val input = EditText(this).apply {
            id = View.generateViewId()
            isSingleLine = true
            inputType = if (numeric) InputType.TYPE_CLASS_NUMBER
                else InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            imeOptions = android.view.inputmethod.EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
            filters = arrayOf(InputFilter.LengthFilter(limit))
            setTextColor(textColor)
            setText(value)
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
                override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = changed(s.toString())
                override fun afterTextChanged(s: Editable?) = Unit
            })
            isEnabled = !busy
        }
        label(parent, title).labelFor = input.id
        parent.addView(input, LinearLayout.LayoutParams(-1, -2))
        if (controlled) controls.add(input)
        return input
    }

    private fun render(scrollY: Int = 0) {
        controls.clear()
        rowControls.clear()
        val dark = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES
        textColor = if (dark) 0xFFF5F5F7.toInt() else 0xFF111113.toInt()
        background = if (dark) 0xFF161719.toInt() else 0xFFF2F2F7.toInt()
        root = column().apply {
            setPadding(dp(16), dp(12), dp(16), dp(28))
            isFocusableInTouchMode = true
        }
        val scroller = ScrollView(this).apply {
            isFillViewport = true
            setBackgroundColor(background)
            addView(root)
            setOnApplyWindowInsetsListener { view, insets ->
                if (Build.VERSION.SDK_INT >= 30) {
                    val safe = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                    view.setPadding(safe.left, safe.top, safe.right, safe.bottom)
                }
                insets
            }
        }
        button(root, "返回键盘设置") { leave() }
        label(root, getString(R.string.chord_profiles_title), 24f)
        status = label(root, message).apply { accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE }
        val e = editor
        if (e == null) {
            button(root, "重新读取") { reload() }
        } else {
            val active = e.snapshot.active?.let { runCatching { ChordDraftCodec.decode(it).name }.getOrNull() }
            label(root, if (e.snapshot.active == null) "当前生效：内置方案"
                else "当前保存的启用方案：${active ?: "无法读取，键盘使用内置方案"}")
            label(root, "导入和编辑先进入草稿。只有「保存并启用」会换掉键盘正在使用的方案。")

            button(root, "导入 JSON…") { selectImport() }
            button(root, "导出当前草稿…") { selectExport(e) }
            button(root, "复制内置方案为草稿") {
                confirmDiscard("用内置方案替换编辑区？") {
                    e.load(e.builtin.copy(name = "内置方案副本"))
                    report("已复制到编辑区。")
                    render()
                }
            }
            button(root, "新建空白草稿") {
                confirmDiscard("新建空白草稿？") {
                    e.load(ChordDraft("我的并击方案", e.builtin.left, e.builtin.right, 300, emptyList()))
                    report("请添加映射后启用。")
                    render()
                }
            }

            val form = e.form
            field(root, "方案名称", form.name, 80) { form.name = it }
            field(root, "左手键位（空格分隔）", form.left, 180) { form.left = it }
            field(root, "右手键位（空格分隔）", form.right, 180) { form.right = it }
            field(root, "物理键盘两手间隔（50–2000 毫秒）", form.interval, 4, numeric = true) { form.interval = it }
            button(root, "验证草稿") {
                val saved = validated(e, apply = false)
                if (saved != null) report(if (e.form.rows.isEmpty()) "草稿可保存；至少添加一条映射才能启用。" else "草稿验证通过。")
            }
            button(root, "保存草稿") { save(e, apply = false) }
            button(root, "保存并启用") { save(e, apply = true) }
            if (e.snapshot.active != null) button(root, "切回内置方案") { restoreBuiltin(e) }
            label(root, "组合映射", 19f)
            field(root, "按组合键或拼音搜索", e.query, 64) { value ->
                e.query = value
                e.page = 0
                renderMappings(e)
            }
            list = column()
            root.addView(list)
            renderMappings(e)
        }
        setContentView(scroller)
        root.requestFocus()
        if (scrollY > 0) scroller.post { scroller.scrollTo(0, scrollY) }
    }

    private fun renderMappings(e: Editor) {
        if (!::list.isInitialized) return
        rowControls.clear()
        list.removeAllViews()
        val matches = e.form.draft().matching(e.query)
        val pages = maxOf(1, (matches.size + PAGE_SIZE - 1) / PAGE_SIZE)
        e.page = e.page.coerceIn(0, pages - 1)
        label(list, "匹配 ${matches.size} / ${e.form.rows.size} 条 · 第 ${e.page + 1} / $pages 页")
        if (e.form.rows.size < ChordDraft.MAX_ROWS) rowControls += button(list, "添加映射", controlled = false) { editRow(e, null) }
        if (matches.isEmpty()) label(list, "暂无匹配。")
        matches.drop(e.page * PAGE_SIZE).take(PAGE_SIZE).forEach { (index, row) ->
            val keys = row.keys.joinToString(" + ") { ChordKeys.char(it).uppercase() }
            rowControls += button(list, "$keys → ${row.output} · ${if (row.fragment) "片段" else "音节"}", controlled = false) {
                editRow(e, index)
            }
        }
        if (e.page > 0) rowControls += button(list, "上一页", controlled = false) { e.page--; renderMappings(e) }
        if (e.page + 1 < pages) rowControls += button(list, "下一页", controlled = false) { e.page++; renderMappings(e) }
    }

    private fun editRow(e: Editor, index: Int?) {
        val row = index?.let { e.form.rows[it] }
        val panel = column().apply { setPadding(dp(16), dp(4), dp(16), dp(4)) }
        val keys = field(panel, "组合键（空格分隔）", row?.keys?.joinToString(" ").orEmpty(), 48, controlled = false)
        val output = field(panel, "拼音输出", row?.output.orEmpty(), 16, controlled = false)
        val fragment = CheckBox(this).apply {
            text = "声母或韵母片段"
            isChecked = row?.fragment == true
            panel.addView(this)
        }
        val dialog = AlertDialog.Builder(this).setTitle(if (row == null) "添加映射" else "编辑映射")
            .setView(panel).setNegativeButton("取消", null).setPositiveButton("确定", null)
        if (index != null) dialog.setNeutralButton("删除") { _, _ ->
            e.form.rows = e.form.rows.filterIndexed { i, _ -> i != index }
            renderMappings(e)
        }
        val shown = dialog.create()
        shown.show()
        shown.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val mapping = ChordDraft.Row(ChordDraft.keys(keys.text.toString()), output.text.toString().trim(), fragment.isChecked)
            val next = e.form.rows.toMutableList()
            if (index == null) next.add(0, mapping) else next[index] = mapping
            val issue = e.form.draft().copy(mappings = next).issue(apply = false)
            if (issue != null && (issue.row == (index ?: 0) || issue.message.startsWith("这个组合"))) {
                output.error = issue.message
                return@setOnClickListener
            }
            // Other invalid rows can still be repaired independently; save/apply validates the full table.
            if (!Regex("[a-z']{1,16}").matches(mapping.output) || mapping.keys.size < 2) {
                output.error = "请填写组合键和 1–16 位小写拼音。"
                return@setOnClickListener
            }
            e.form.rows = next
            shown.dismiss()
            renderMappings(e)
        }
    }

    private fun validated(e: Editor, apply: Boolean): ChordDraft? {
        val draft = e.form.draft()
        val issue = draft.issue(apply)
        if (issue != null) {
            report((issue.row?.let { "第 ${it + 1} 条映射：" } ?: "") + issue.message)
            issue.row?.let { row ->
                e.page = row / PAGE_SIZE
                if (e.query.isNotEmpty()) {
                    e.query = ""
                    render()
                } else renderMappings(e)
            }
            return null
        }
        return draft
    }

    private fun save(e: Editor, apply: Boolean) {
        val draft = validated(e, apply) ?: return
        val encoded = ChordDraftCodec.encode(draft)
        val expected = e.snapshot
        runTask(if (apply) "保存并启用" else "保存草稿") {
            val error = if (apply) NativeChord.validate(encoded) else NativeChord.validateDraft(encoded)
            check(error == null) { "输入法核心不接受这份方案，请核对键位和映射。" }
            store.write(expected, expected.copy(draft = encoded, active = if (apply) encoded else expected.active))
        }.then {
            e.snapshot = it
            e.baseline = draft
            report(if (apply) "已启用。并击键盘将使用新方案，未完成的并击会取消。" else "草稿已保存，当前输入方案保持不变。")
            render((root.parent as? ScrollView)?.scrollY ?: 0)
        }
    }

    private fun restoreBuiltin(e: Editor) {
        AlertDialog.Builder(this).setTitle("切回内置方案？").setMessage("自定义草稿仍会保留，可随时再次启用。")
            .setNegativeButton("取消", null).setPositiveButton("切回") { _, _ ->
                val expected = e.snapshot
                runTask("切回内置方案") { store.write(expected, expected.copy(active = null)) }.then {
                    e.snapshot = it
                    report("内置方案已生效。")
                    render()
                }
            }.show()
    }

    @Suppress("DEPRECATION")
    private fun selectImport() = confirmDiscard("用导入的文件替换编辑区？") {
        picker(Intent(Intent.ACTION_OPEN_DOCUMENT).setType("*/*").addCategory(Intent.CATEGORY_OPENABLE), IMPORT)
    }

    @Suppress("DEPRECATION")
    private fun selectExport(e: Editor) {
        val draft = validated(e, apply = false) ?: return
        e.export = ChordDraftCodec.encode(draft)
        picker(Intent(Intent.ACTION_CREATE_DOCUMENT).setType("application/json")
            .addCategory(Intent.CATEGORY_OPENABLE).putExtra(Intent.EXTRA_TITLE, "moodflow-chord.json"), EXPORT)
    }

    @Suppress("DEPRECATION")
    private fun picker(intent: Intent, request: Int) {
        try {
            startActivityForResult(intent, request)
        } catch (_: android.content.ActivityNotFoundException) {
            report("设备没有可用的文件选择器。")
        }
    }

    @Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val e = editor ?: return
        val uri = data?.data
        if (resultCode != RESULT_OK || uri == null) {
            if (requestCode == EXPORT) e.export = null
            return
        }
        when (requestCode) {
            IMPORT -> importFile(e, uri)
            EXPORT -> {
                val export = e.export
                e.export = null
                if (export == null) return report("未找到要导出的草稿，请重试。")
                runTask("导出方案") {
                    val stream = contentResolver.openOutputStream(uri, "wt") ?: throw IOException("无法打开文件。")
                    stream.use { it.write(export.toByteArray(Charsets.UTF_8)) }
                }.then { report("已导出 JSON。") }
            }
        }
    }

    private fun importFile(e: Editor, uri: Uri) {
        runTask("导入方案") {
            val stream = contentResolver.openInputStream(uri) ?: throw IOException("无法打开文件。")
            val json = stream.use { ChordDraftCodec.boundedText(it, ChordDraft.MAX_BYTES) }
            check(NativeChord.validateDraft(json) == null) { "文件不是有效的版本 1 并击 JSON。" }
            val draft = ChordDraftCodec.decode(json)
            val issue = draft.issue(apply = false)
            check(issue == null) { (issue?.row?.let { "第 ${it + 1} 条映射：" } ?: "") + issue?.message.orEmpty() }
            draft
        }.then {
            e.load(it)
            report("已导入 ${it.mappings.size} 条映射到编辑区。检查后可保存或启用。")
            render()
        }
    }

    companion object {
        private const val PAGE_SIZE = 16
        private const val IMPORT = 701
        private const val EXPORT = 702
    }
}
