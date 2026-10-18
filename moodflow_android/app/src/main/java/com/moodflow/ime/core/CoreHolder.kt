package com.moodflow.ime.core

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.moodflow.ime.jni.NativeEventSink

/**
 * 进程内唯一的核心会话，键盘服务与设置页共用。
 *
 * 为什么不能各建一个：核心构造会把 XDG 目录钉进进程环境变量，用户词库（redb）也只能被一个实例
 * 打开——设置页再起一个核心，要么拿不到词库锁，要么两边写同一份配置互相覆盖。
 *
 * 引用计数：[acquire] / [release] 成对调用，最后一个使用者离开时关闭核心。核心的推送回调转给
 * 当前登记的 [listener]（键盘服务）；设置页不需要推送。
 *
 * 全部方法在主线程调用；核心构造在后台线程。
 */
object CoreHolder {
    private const val TAG = "Moodflow"
    const val FALLBACK_SCHEMA = "pinyin"

    private val main = Handler(Looper.getMainLooper())
    private var core: MoodflowCore? = null
    private var starting = false
    private var failed = false
    private var users = 0
    private val waiting = ArrayList<(MoodflowCore?) -> Unit>()

    /** 推送回调的去处（来自核心泵线程，**不是**主线程）。 */
    @Volatile
    var listener: NativeEventSink? = null

    private val sink = object : NativeEventSink {
        override fun onCandidates(
            preedit: String,
            caret: Int,
            modeLabel: String,
            items: Array<String>,
            selected: Int,
            page: Int,
            totalPages: Int,
        ) {
            listener?.onCandidates(preedit, caret, modeLabel, items, selected, page, totalPages)
        }

        override fun onHideCandidates() {
            listener?.onHideCandidates()
        }

        override fun onStatus(
            chineseMode: Boolean,
            iconLabel: String,
            fullWidth: Boolean,
            chinesePunct: Boolean,
            s2tEnabled: Boolean,
            s2tShown: Boolean,
        ) {
            listener?.onStatus(chineseMode, iconLabel, fullWidth, chinesePunct, s2tEnabled, s2tShown)
        }

        override fun onToast(text: String) {
            listener?.onToast(text)
        }
    }

    /** 当前核心（未就绪或失败为 null）。 */
    val current: MoodflowCore? get() = core

    /** 构造失败过（原生库缺失、数据坏了），之后不再重试。 */
    val unavailable: Boolean get() = failed

    /** 登记一个使用者；核心就绪（或确定不可用）时在主线程回调 [ready]。 */
    fun acquire(ctx: Context, ready: (MoodflowCore?) -> Unit) {
        users += 1
        val c = core
        when {
            c != null -> ready(c)
            failed -> ready(null)
            else -> {
                waiting += ready
                if (!starting) start(ctx.applicationContext)
            }
        }
    }

    /** 使用者离开。最后一个离开时关闭核心（下次 [acquire] 重新构造）。 */
    fun release() {
        users = (users - 1).coerceAtLeast(0)
        if (users == 0 && !starting) {
            core?.close()
            core = null
            failed = false
            Log.i(TAG, "核心已关闭")
        }
    }

    private fun start(app: Context) {
        starting = true
        Thread({
            val c = try {
                val data = DataInstaller.install(app)
                MoodflowCore.create(data, DataInstaller.userRoot(app), FALLBACK_SCHEMA, sink)
            } catch (e: Exception) {
                Log.e(TAG, "核心启动失败", e)
                null
            }
            main.post { onStarted(c) }
        }, "moodflow-core-init").start()
    }

    private fun onStarted(c: MoodflowCore?) {
        starting = false
        if (users == 0) {
            // 等的人都走了：不留一个没人用的核心占着词库
            c?.close()
            waiting.clear()
            return
        }
        core = c
        failed = c == null
        if (c != null) Log.i(TAG, "核心就绪")
        val callbacks = waiting.toList()
        waiting.clear()
        callbacks.forEach { it(c) }
    }
}
