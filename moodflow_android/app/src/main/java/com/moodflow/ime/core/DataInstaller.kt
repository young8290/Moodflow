package com.moodflow.ime.core

import android.content.Context
import android.content.res.AssetManager
import android.os.Build
import com.moodflow.ime.logic.AssetEntry
import com.moodflow.ime.logic.AssetInstall
import org.json.JSONObject
import java.io.File
import java.io.IOException

/** Extracts verified APK assets into filesDir/data. User data lives separately under filesDir/rust. */
object DataInstaller {
    private const val MAX_MANIFEST_BYTES = 4 * 1024 * 1024

    fun dataDir(ctx: Context): File = File(ctx.filesDir, "data")

    fun userRoot(ctx: Context): File = File(ctx.filesDir, "rust").apply { mkdirs() }

    /** Called on the CoreHolder worker before starting the shared core. */
    fun install(ctx: Context): File = AssetInstall.install(
        ctx.filesDir,
        stamp(ctx),
        { readManifest(ctx.assets) },
        { path -> ctx.assets.open("data/$path", AssetManager.ACCESS_STREAMING) },
    )

    private fun readManifest(assets: AssetManager): List<AssetEntry> {
        val contents = assets.open("data/manifest.json").use { input ->
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(16 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (out.size() + count > MAX_MANIFEST_BYTES) throw IOException("出厂数据清单过大")
                out.write(buffer, 0, count)
            }
            out.toString(Charsets.UTF_8.name())
        }
        val document = JSONObject(contents)
        if (document.optInt("version") != 1) throw IOException("出厂数据清单版本不受支持")
        val files = document.getJSONObject("files")
        return files.keys().asSequence().map { path ->
            val entry = files.getJSONObject(path)
            AssetEntry(path, entry.getLong("size"), entry.getString("sha256"))
        }.toList()
    }

    private fun stamp(ctx: Context): String {
        val info = if (Build.VERSION.SDK_INT >= 33) {
            ctx.packageManager.getPackageInfo(ctx.packageName, android.content.pm.PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            ctx.packageManager.getPackageInfo(ctx.packageName, 0)
        }
        @Suppress("DEPRECATION")
        val code = if (Build.VERSION.SDK_INT >= 28) info.longVersionCode else info.versionCode.toLong()
        return "$code:${info.lastUpdateTime}"
    }
}
