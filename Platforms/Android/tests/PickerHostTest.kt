package org.shinkirou.shinkiemu

import android.app.Activity
import android.provider.DocumentsContract.Document.MIME_TYPE_DIR
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit.SECONDS

// Compile alongside ShinkiPlatformBridge.kt and ShinkiLibraryTree.kt against the SDK android.jar,
// run PickerHostTestKt on the host JVM. No Android process or test runner.
fun main() {
    checkResult(Activity.RESULT_CANCELED, {
        error("cancellation must not read the returned intent")
    }) { path, error -> check(path == null && error == null) }
    checkResult(Activity.RESULT_OK, { "chosen folder" }) { path, error ->
        check(path == "chosen folder" && error == null)
    }
    checkResult(Activity.RESULT_OK, { null }) { path, error ->
        check(path == null && error?.startsWith("SHINKI_EMU_ANDROID_PICK_RESULT:") == true)
    }
    checkResult(Activity.RESULT_FIRST_USER, {
        error("an unexpected result must not read the returned intent")
    }) { path, error -> check(path == null && error?.startsWith("SHINKI_EMU_ANDROID_PICK_RESULT:") == true) }
    checkResult(Activity.RESULT_OK, { throw SecurityException("provider denied access") }) { path, error ->
        check(path == null && error?.startsWith("SHINKI_EMU_ANDROID_PICK_PERMISSION:") == true)
        check(error.contains("provider denied access"))
    }
    println("SAF result callback: success, cancel, missing URI, unexpected result, and permission denial")

    val provider = "com.android.externalstorage.documents"
    val parse = ShinkiLibraryTree::localDocumentPath
    check(parse(provider, true, "primary:Games/Play") == ("primary" to listOf("Games", "Play")))
    check(parse(provider, true, "ABCD-1234:Games") == ("ABCD-1234" to listOf("Games")))
    check(parse("cloud.provider", true, "primary:Games") == null)
    check(parse(provider, false, "primary:Games") == null)
    for (invalid in listOf("primary:/Games", "primary:../Games", "primary:Games//Play", "primary:Games/.")) {
        check(parse(provider, true, invalid) == null)
    }
    println("SAF volume mapping: primary and removable volumes; rejects non-system providers and escaping paths")

    val files = List(10) { "application/octet-stream" }
    val rows = files + MIME_TYPE_DIR + files
    fun countRows(input: List<String>, limit: Int, maxRows: Int, directoriesOnly: Boolean): Pair<Boolean, Int> {
        val cursor = input.iterator()
        var current = ""
        var selected = 0
        val complete = visitTreeRows(limit, maxRows, {
            if (!cursor.hasNext()) false else {
                current = cursor.next()
                true
            }
        }, { current }, directoriesOnly) { selected++ }
        return complete to selected
    }
    check(countRows(rows, 1, rows.size, directoriesOnly = true) == (true to 1))
    check(countRows(files, 0, files.size, directoriesOnly = true) == (true to 0))
    check(countRows(rows, 0, rows.size, directoriesOnly = true) == (false to 0))
    check(countRows(rows, 1, rows.size - 1, directoriesOnly = true) == (false to 1))
    check(countRows(rows, 1, rows.size, directoriesOnly = false) == (false to 1))
    println("SAF traversal: files do not consume directory slots; real directory and provider row limits still apply")

    val root = Files.createTempDirectory("shinki-saf-host-").toFile()
    try {
        val imports = File(root, "imports")
        val partial = File(imports, "partial")
        val failure = IOException("copy interrupted")
        val reported = runCatching {
            withNewImportDirectory(partial) {
                File(partial, "partial.bin").writeText("part")
                throw failure
            }
        }.exceptionOrNull()
        check(reported === failure)
        check(!partial.exists() && imports.isDirectory)

        // While one import is writing, another attempt at the same destination
        // must not run or remove the first import's files.
        val dest = File(imports, "same-request")
        val saved = File(dest, "game.bin")
        val ready = CountDownLatch(1)
        val competingFinished = CountDownLatch(1)
        val worker = Executors.newSingleThreadExecutor()
        try {
            val claimed = worker.submit<File> {
                withNewImportDirectory(dest) {
                    saved.writeText("owned by the first import")
                    ready.countDown()
                    check(competingFinished.await(10, SECONDS))
                    saved
                }
            }
            check(ready.await(10, SECONDS))
            var competingCopyRan = false
            val collision = runCatching {
                withNewImportDirectory(dest) { competingCopyRan = true }
            }.exceptionOrNull()
            check(collision is IllegalStateException && !competingCopyRan)
            competingFinished.countDown()
            check(claimed.get(10, SECONDS) == saved)
            check(saved.readText() == "owned by the first import")
        } finally {
            competingFinished.countDown()
            worker.shutdownNow()
            check(worker.awaitTermination(10, SECONDS))
        }
    } finally {
        check(root.deleteRecursively())
    }
    println("SAF copy cleanup: failed copy removed, existing destination and data preserved")
}

private fun checkResult(
    result: Int,
    readSelection: () -> String?,
    checkOutcome: (String?, String?) -> Unit,
) {
    var callbacks = 0
    handleDocumentPickerResult(result, readSelection) { path, error ->
        callbacks += 1
        checkOutcome(path, error)
    }
    check(callbacks == 1)
}
