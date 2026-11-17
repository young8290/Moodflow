package com.moodflow.ime.logic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.security.MessageDigest

class AssetInstallTest {
    @get:Rule val folder = TemporaryFolder()

    private fun asset(path: String, bytes: String) = AssetEntry(
        path,
        bytes.toByteArray().size.toLong(),
        MessageDigest.getInstance("SHA-256").digest(bytes.toByteArray()).joinToString("") { "%02x".format(it) },
    )

    private fun install(root: File, stamp: String, files: Map<String, String>) =
        AssetInstall.install(root, stamp, { files.map { (path, value) -> asset(path, value) } }) {
            files.getValue(it).byteInputStream()
        }

    @Test
    fun verifiedUpgradeMakesOnlyTheNewDataVisible() {
        val root = folder.newFolder()
        val user = root.resolve("rust").apply { mkdir() }.resolve("userdata.redb")
        user.writeText("user data")
        install(root, "1", mapOf("config.toml" to "old", "schemas/removed" to "old schema"))
        val installed = install(root, "2", mapOf("config.toml" to "new", "schemas/added" to "new schema"))
        assertEquals("new", installed.resolve("config.toml").readText())
        assertEquals("new schema", installed.resolve("schemas/added").readText())
        assertFalse(installed.resolve("schemas/removed").exists())
        assertEquals("user data", user.readText())
        assertFalse(root.resolve("data.previous").exists())
        AssetInstall.install(root, "2", { error("已校验的同版本不重复解包") }) { error("不得打开资产") }
    }

    @Test
    fun damagedOrMissingAssetLeavesPreviousDataAndCanRetry() {
        val root = folder.newFolder()
        val data = install(root, "1", mapOf("config.toml" to "old"))
        val entries = listOf(asset("config.toml", "new"), asset("schemas/pinyin", "good"))
        for (missing in listOf(false, true)) {
            assertThrows(IOException::class.java) {
                AssetInstall.install(root, "2", { entries }) { path ->
                    if (path == "config.toml") "new".byteInputStream()
                    else if (missing) throw FileNotFoundException()
                    else "evil".byteInputStream()
                }
            }
            assertEquals("old", data.resolve("config.toml").readText())
            assertFalse(root.resolve("data.staged").exists())
        }
        install(root, "2", mapOf("config.toml" to "new", "schemas/pinyin" to "good"))
        assertEquals("new", data.resolve("config.toml").readText())
    }

    @Test
    fun interruptedSwapRecoversTheOldTreeBeforeRetrying() {
        val root = folder.newFolder()
        val data = install(root, "1", mapOf("config.toml" to "old"))
        java.nio.file.Files.move(data.toPath(), root.resolve("data.previous").toPath())
        root.resolve("data.staged").mkdir()
        root.resolve("data.staged/broken").writeText("copy still in progress")
        assertThrows(IOException::class.java) {
            AssetInstall.install(root, "2", { listOf(asset("config.toml", "ok")) }) {
                throw IOException("source missing")
            }
        }
        assertEquals("old", data.resolve("config.toml").readText())
        assertFalse(root.resolve("data.staged").exists())
    }

    @Test
    fun rejectsPathsOutsideTheVerifiedTreeBeforeOpeningAssets() {
        val root = folder.newFolder()
        assertThrows(IOException::class.java) {
            AssetInstall.install(root, "1", { listOf(asset("config.toml", "ok"), asset("../rust/a", "x")) }) {
                error("路径无效，不得打开资产")
            }
        }
        assertFalse(root.resolve("rust/a").exists())
        assertFalse(root.resolve("data").exists())
    }
}
