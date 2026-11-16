package com.moodflow.ime

import android.app.Activity
import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowInsets
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import com.moodflow.ime.core.CoreHolder
import com.moodflow.ime.core.DataInstaller
import com.moodflow.ime.jni.MoodflowNative as Native
import com.moodflow.ime.logic.Wire
import java.io.File
import java.io.IOException
import java.util.concurrent.Executors

/** 系统文件选择器 → 兼容性预览 → 确认安装单个方案；中文输入方案页负责切换。 */
class RimeImportActivity : Activity() {
    private var ready = false
    private var destroyed = false
    private var busy = false
    private var uri: Uri? = null
    private var preview: Wire.RimePreview? = null
    private var installed = mutableSetOf<String>()
    private var offset = 0
    private var status = "正在打开输入法核心…"
    private lateinit var scroll: ScrollView
    private lateinit var body: LinearLayout

    @Suppress("DEPRECATION")
    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        uri = state?.getParcelable("rime.uri")
        installed = state?.getStringArrayList("rime.installed")?.toMutableSet() ?: mutableSetOf()
        val frame = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val back = Button(this).apply {
            text = "返回输入方案"
            setOnClickListener { finish() }
        }
        frame.addView(back)
        scroll = ScrollView(this)
        body = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), dp(24))
        }
        scroll.addView(body)
        frame.addView(scroll, LinearLayout.LayoutParams(-1, 0, 1f))
        frame.setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val safe = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                view.setPadding(safe.left, safe.top, safe.right, safe.bottom)
            }
            insets
        }
        setContentView(frame)
        render()
        CoreHolder.acquire(this) { core ->
            if (!destroyed) {
                ready = core != null
                status = if (ready) "" else "输入法核心无法启动，暂不能导入。"
                val lastFile = uri
                if (ready && lastFile != null) loadPreview(lastFile) else render()
            }
        }
    }

    override fun onSaveInstanceState(state: Bundle) {
        super.onSaveInstanceState(state)
        state.putParcelable("rime.uri", uri)
        state.putStringArrayList("rime.installed", ArrayList(installed))
    }

    override fun onDestroy() {
        destroyed = true
        CoreHolder.release()
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    private fun pickArchive() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            // 部分文件提供器把 ZIP 标为 application/octet-stream 或 application/x-zip-compressed。
            type = "*/*"
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        try {
            startActivityForResult(intent, OPEN_ZIP)
        } catch (_: ActivityNotFoundException) {
            status = "系统文件选择器不可用。"
            render()
        }
    }

    @Deprecated("Activity file picker result")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != OPEN_ZIP || resultCode != RESULT_OK) return
        val source = data?.data
        if (source == null) {
            status = "文件选择器没有返回可读取的文件。"
            render()
        } else {
            loadPreview(source)
        }
    }

    private fun loadPreview(source: Uri) {
        uri = source
        preview = null
        installed.clear()
        offset = 0
        runFileJob(source, "正在检查 ZIP 和词库…", { data, archive ->
            Native.nativeRimePreview(data, archive)
        }) { result ->
            preview = Wire.rimePreview(result)
            status = if (preview == null) errorMessage(result) else ""
        }
    }

    private fun confirm(scheme: Wire.RimeScheme) {
        if (busy) return
        val source = uri ?: return
        val revision = preview?.revision ?: return
        AlertDialog.Builder(this)
            .setTitle("安装「${scheme.name.take(200)}」？")
            .setMessage("安装后可回到输入方案列表切换。首次切换可能需要构建词库。")
            .setNegativeButton("取消", null)
            .setPositiveButton("安装") { _, _ ->
                runFileJob(source, "正在安装 ${scheme.name}…", { data, archive ->
                    Native.nativeRimeInstall(data, archive, scheme.id, revision)
                }, scheme.id) { result ->
                    if (result?.firstOrNull() == "ok") {
                        installed += scheme.id
                        status = if (result.getOrNull(1) == "listed") {
                            "已安装「${scheme.name}」。返回输入方案列表即可选用。"
                        } else {
                            "已安装「${scheme.name}」。下次启动输入法后即可选用。"
                        }
                    } else {
                        status = errorMessage(result)
                    }
                }
            }
            .show()
    }

    private fun runFileJob(
        source: Uri,
        message: String,
        nativeCall: (String, String) -> Array<String>?,
        installedId: String? = null,
        complete: (Array<String>?) -> Unit,
    ) {
        if (!ready || busy) return
        busy = true
        status = message
        render()
        val app = applicationContext
        worker.execute {
            val result = try {
                val staged = File.createTempFile("rime-", ".zip", app.cacheDir)
                try {
                    val stream = app.contentResolver.openInputStream(source)
                        ?: throw IOException("无法从文件提供器打开 ZIP")
                    stream.use { input ->
                        staged.outputStream().use { out ->
                            val bytes = ByteArray(64 * 1024)
                            var copied = 0L
                            while (true) {
                                val count = input.read(bytes)
                                if (count < 0) break
                                copied += count
                                if (copied > MAX_ZIP_BYTES) throw TooLarge()
                                out.write(bytes, 0, count)
                            }
                        }
                    }
                    nativeCall(DataInstaller.dataDir(app).absolutePath, staged.absolutePath)
                } finally {
                    staged.delete()
                }
            } catch (_: TooLarge) {
                arrayOf("error", "ZIP 超过 32 MiB，请拆成较小的方案包。")
            } catch (_: SecurityException) {
                arrayOf("error", "文件访问权限已失效，请重新选择 ZIP。")
            } catch (_: IOException) {
                arrayOf("error", "无法读取文件或写入临时空间，请检查文件和剩余存储。")
            }
            runOnUiThread {
                // 即使设置页已关闭，也要把成功安装的方案登记到仍在运行的键盘核心。
                val listed = installedId != null && result?.firstOrNull() == "ok" &&
                    CoreHolder.current?.registerImportedSchema(installedId) == true
                if (!destroyed) {
                    busy = false
                    complete(if (listed) arrayOf("ok", "listed") else result)
                    render()
                }
            }
        }
    }

    private fun render() {
        body.removeAllViews()
        heading("从 Rime ZIP 导入", 24)
        label("选择包含 .schema.yaml 和 .dict.yaml 的 ZIP。支持全拼和码表，兼容性提示会在安装前列出。")
        button(if (preview == null) "选择 ZIP" else "选择其他 ZIP", ready && !busy, ::pickArchive)
        if (busy) body.addView(ProgressBar(this), LinearLayout.LayoutParams(dp(30), dp(30)))
        if (status.isNotEmpty()) label(status).accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        val current = preview ?: return
        heading("找到 ${current.schemes.size} 套方案", 18)
        if (current.warnings.isNotEmpty()) {
            button("查看兼容性提示（${current.warnings.size}）", !busy) {
                val details = TextView(this).apply {
                    setPadding(dp(20), dp(8), dp(20), dp(8))
                    text = current.warnings.joinToString("\n\n") { it.take(500) }
                    setTextIsSelectable(true)
                }
                AlertDialog.Builder(this)
                    .setTitle("兼容性提示")
                    .setView(ScrollView(this).apply { addView(details) })
                    .setPositiveButton("关闭", null)
                    .show()
            }
        }
        current.schemes.drop(offset).take(PAGE_SIZE).forEach { scheme ->
            label(scheme.name.take(200), 18)
            label("${scheme.id.take(140)} · ${scheme.kind} · ${scheme.dictionaries} 个词库")
            when {
                scheme.id in installed -> label("本次已安装")
                scheme.blocked -> label("同名方案已安装，或方案标识不可用")
                else -> button("安装 ${scheme.name.take(80)}", !busy) { confirm(scheme) }
            }
        }
        if (offset > 0) button("上一页", !busy) { offset -= PAGE_SIZE; render(); scroll.scrollTo(0, 0) }
        if (offset + PAGE_SIZE < current.schemes.size) {
            button("下一页", !busy) { offset += PAGE_SIZE; render(); scroll.scrollTo(0, 0) }
        }
    }

    private fun heading(value: String, size: Int) {
        val view = label(value, size)
        if (Build.VERSION.SDK_INT >= 28) view.isAccessibilityHeading = true
    }

    private fun label(value: String, size: Int = 15) = TextView(this).apply {
        text = value
        textSize = size.toFloat()
        setPadding(0, dp(8), 0, dp(6))
        body.addView(this)
    }

    private fun button(title: String, enabled: Boolean, action: () -> Unit) {
        body.addView(Button(this).apply {
            text = title
            isAllCaps = false
            isEnabled = enabled
            setOnClickListener { action() }
        })
    }

    private fun errorMessage(result: Array<String>?): String =
        result?.getOrNull(1)?.takeIf { result.firstOrNull() == "error" }?.take(600)
            ?: "导入未完成，请重新选择 ZIP。"

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    private class TooLarge : IOException()

    private companion object {
        const val OPEN_ZIP = 31
        const val PAGE_SIZE = 12
        // 与 moodflow_transfer::rime_install::MAX_SOURCE_BYTES 同值；Rust 也会再查一次。
        const val MAX_ZIP_BYTES = 32L * 1024 * 1024
        val worker = Executors.newSingleThreadExecutor()
    }
}
