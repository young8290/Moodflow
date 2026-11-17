package com.moodflow.ime.logic

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.nio.file.FileVisitResult
import java.nio.file.Files
import java.nio.file.LinkOption.NOFOLLOW_LINKS
import java.nio.file.Path
import java.nio.file.SimpleFileVisitor
import java.nio.file.StandardCopyOption.ATOMIC_MOVE
import java.nio.file.attribute.BasicFileAttributes
import java.security.MessageDigest

data class AssetEntry(val path: String, val size: Long, val sha256: String)

/** Publishes an APK data tree only after all declared bytes match the package manifest. */
object AssetInstall {
    private const val DATA = "data"
    private const val STAGED = "data.staged"
    private const val PREVIOUS = "data.previous"
    private const val MARKER = ".installed"

    fun install(
        root: File,
        stamp: String,
        readEntries: () -> List<AssetEntry>,
        openAsset: (String) -> InputStream,
    ): File {
        val data = root.resolve(DATA)
        val staged = root.resolve(STAGED)
        val previous = root.resolve(PREVIOUS)
        val marker = "verified-v1:$stamp"
        // A crash between the two moves leaves a complete prior tree under PREVIOUS.
        if (!exists(data) && exists(previous)) move(previous, data)
        deleteTree(staged)
        val installedMarker = data.resolve(MARKER)
        if (Files.isDirectory(data.toPath(), NOFOLLOW_LINKS) &&
            Files.isRegularFile(installedMarker.toPath(), NOFOLLOW_LINKS) &&
            installedMarker.length() <= 256 && installedMarker.readText() == marker
        ) {
            runCatching { deleteTree(previous) }
            return data
        }

        val entries = readEntries()
        validate(entries)
        Files.createDirectory(staged.toPath())
        try {
            val buffer = ByteArray(64 * 1024)
            for (entry in entries) {
                val target = staged.resolve(entry.path)
                Files.createDirectories(target.toPath().parent)
                openAsset(entry.path).use { input -> copyVerified(input, target, entry, buffer) }
            }
            FileOutputStream(staged.resolve(MARKER)).use {
                it.write(marker.toByteArray(Charsets.UTF_8))
                it.fd.sync()
            }

            deleteTree(previous)
            if (exists(data)) move(data, previous)
            try {
                move(staged, data)
            } catch (error: IOException) {
                if (exists(previous) && !exists(data)) {
                    try {
                        move(previous, data)
                    } catch (restore: IOException) {
                        error.addSuppressed(restore)
                    }
                }
                throw error
            }
            runCatching { deleteTree(previous) }
            return data
        } finally {
            // Cleanup cannot mask the verification failure or the rollback result.
            runCatching { deleteTree(staged) }
        }
    }

    private fun validate(entries: List<AssetEntry>) {
        val names = HashSet<String>()
        if (entries.size > 20_000) throw IOException("出厂数据清单条目过多")
        for ((path, size, digest) in entries) {
            val parts = path.split('/')
            val valid = path.length in 1..1024 && !path.contains('\\') && !path.contains(':') &&
                !path.contains('\u0000') && parts.none { it.isEmpty() || it == "." || it == ".." } &&
                path != MARKER && path != "manifest.json"
            if (!valid || !names.add(path) || size < 0 ||
                digest.length != 64 || !digest.all { it in 'a'..'f' || it in '0'..'9' }
            ) {
                throw IOException("出厂数据清单存在无效条目")
            }
        }
        if ("config.toml" !in names) throw IOException("出厂数据清单缺少 config.toml")
    }

    private fun copyVerified(input: InputStream, target: File, entry: AssetEntry, buffer: ByteArray) {
        val digest = MessageDigest.getInstance("SHA-256")
        var count = 0L
        FileOutputStream(target).use { output ->
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                count += read
                if (count > entry.size) throw IOException("出厂文件长度不符：${entry.path}")
                digest.update(buffer, 0, read)
                output.write(buffer, 0, read)
            }
            val hash = digest.digest().joinToString("") { "%02x".format(it) }
            if (count != entry.size || hash != entry.sha256) {
                throw IOException("出厂文件校验失败：${entry.path}")
            }
            output.fd.sync()
        }
    }

    private fun move(from: File, to: File) {
        Files.move(from.toPath(), to.toPath(), ATOMIC_MOVE)
    }

    private fun exists(file: File) = Files.exists(file.toPath(), NOFOLLOW_LINKS)

    private fun deleteTree(file: File) {
        if (!exists(file)) return
        Files.walkFileTree(file.toPath(), object : SimpleFileVisitor<Path>() {
            override fun visitFile(path: Path, attrs: BasicFileAttributes): FileVisitResult {
                Files.delete(path)
                return FileVisitResult.CONTINUE
            }

            override fun postVisitDirectory(path: Path, error: IOException?): FileVisitResult {
                if (error != null) throw error
                Files.delete(path)
                return FileVisitResult.CONTINUE
            }
        })
    }
}
