package org.shinkirou.shinkiemu

import android.app.Activity
import android.content.ClipData
import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.util.UUID

/** Grants read access to one user-requested summary, never to the original log or data directory. */
class ShinkiDiagnosticProvider : ContentProvider() {
    override fun onCreate() = true

    override fun getType(uri: Uri): String? = snapshot(uri)?.let { MIME }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw FileNotFoundException("Read-only diagnostic summary")
        val file = snapshot(uri) ?: throw FileNotFoundException("Diagnostic summary expired")
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor? {
        val file = snapshot(uri) ?: return null
        val columns = projection?.toList() ?: listOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        return MatrixCursor(columns.toTypedArray(), 1).apply {
            addRow(columns.map { column ->
                when (column) {
                    OpenableColumns.DISPLAY_NAME -> DISPLAY_NAME
                    OpenableColumns.SIZE -> file.length()
                    else -> null
                }
            })
        }
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = throw UnsupportedOperationException()

    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int =
        throw UnsupportedOperationException()

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int =
        throw UnsupportedOperationException()

    private fun snapshot(uri: Uri): File? {
        val ctx = context ?: return null
        val name = uri.pathSegments.singleOrNull() ?: return null
        if (!SNAPSHOT_NAME.matches(name) || uri != snapshotUri(ctx, name)) return null
        val file = File(snapshotDirectory(ctx), name)
        if (runCatching { file.canonicalFile }.getOrNull() != file || !file.isFile) return null
        val age = System.currentTimeMillis() - file.lastModified()
        return file.takeIf { age in 0..MAX_AGE_MS }
    }

    companion object {
        private const val MIME = "text/plain"
        private const val DISPLAY_NAME = "ShinkiEMU-diagnostic-summary.jsonl"
        private const val MAX_AGE_MS = 15 * 60 * 1000L
        private val SNAPSHOT_NAME = Regex("[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}\\.jsonl")

        private fun snapshotDirectory(context: Context) = File(context.cacheDir.canonicalFile, "diagnostic-share")

        private fun snapshotUri(context: Context, name: String): Uri = Uri.Builder()
            .scheme("content")
            .authority("${context.packageName}.diagnostics")
            .appendPath(name)
            .build()

        // Called only by Manager's user-initiated share callback. Each tap gets
        // a different URI; an earlier temporary grant cannot see a later export.
        @JvmStatic
        fun share(activity: Activity, summary: ByteArray, title: String) {
            val directory = snapshotDirectory(activity)
            if ((!directory.mkdirs() && !directory.isDirectory) || directory.canonicalFile != directory) {
                throw IOException("Diagnostic share cache unavailable")
            }
            val file = File(directory, "${UUID.randomUUID()}.jsonl")
            if (!file.createNewFile()) throw IOException("Diagnostic share snapshot already exists")
            try {
                file.writeBytes(summary)
                val uri = snapshotUri(activity, file.name)
                val send = Intent(Intent.ACTION_SEND).apply {
                    type = MIME
                    putExtra(Intent.EXTRA_STREAM, uri)
                    clipData = ClipData.newUri(activity.contentResolver, DISPLAY_NAME, uri)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                activity.startActivity(Intent.createChooser(send, title))
            } catch (error: Exception) {
                file.delete()
                throw error
            }
            // There is one current snapshot. Canceling the system chooser has
            // no app callback and the provider stops serving it at expiration.
            directory.listFiles()?.forEach { other ->
                if (other != file && SNAPSHOT_NAME.matches(other.name)) other.delete()
            }
        }
    }
}
