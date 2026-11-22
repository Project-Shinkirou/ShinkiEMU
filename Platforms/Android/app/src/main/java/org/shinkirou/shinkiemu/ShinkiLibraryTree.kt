package org.shinkirou.shinkiemu

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.system.Os
import android.system.OsConstants
import java.io.File
import org.json.JSONArray
import org.json.JSONObject

/** The game library reads its SAF grants in place. File and patch pickers use a separate bridge. */
internal object ShinkiLibraryTree {
    private const val EXTERNAL_STORAGE = "com.android.externalstorage.documents"
    private const val MAX_URI_LENGTH = 4096
    private const val MAX_ROWS = 4096
    private const val ID = DocumentsContract.Document.COLUMN_DOCUMENT_ID
    private const val NAME = DocumentsContract.Document.COLUMN_DISPLAY_NAME
    private const val MIME = DocumentsContract.Document.COLUMN_MIME_TYPE
    private val PROJECTION = arrayOf(ID, NAME, MIME)

    fun remember(context: Context, selected: Uri): Uri {
        val tree = treeGrant(selected)
        require(tree == selected) { "picker did not return a canonical tree URI" }
        context.contentResolver.takePersistableUriPermission(tree, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        return tree
    }

    fun document(context: Context, raw: String): String = rows {
        val uri = requireGrant(context, raw)
        val id = if (uri.pathSegments.size == 2) DocumentsContract.getTreeDocumentId(uri)
            else DocumentsContract.getDocumentId(uri)
        val document = DocumentsContract.buildDocumentUriUsingTree(uri, id)
        context.contentResolver.query(document, PROJECTION, null, null, null).use { cursor ->
            checkNotNull(cursor) { "document provider returned no cursor" }
            if (!cursor.moveToFirst()) throw java.io.FileNotFoundException("selected folder no longer exists")
            put(readRow(cursor, uri))
        }
    }

    fun children(context: Context, raw: String, limit: Int, directoriesOnly: Boolean): String = rows {
        require(limit in 0..MAX_ROWS)
        val uri = requireGrant(context, raw)
        val id = DocumentsContract.getDocumentId(uri)
        val children = DocumentsContract.buildChildDocumentsUriUsingTree(uri, id)
        context.contentResolver.query(children, PROJECTION, null, null, null).use { cursor ->
            checkNotNull(cursor) { "document provider returned no cursor" }
            val mime = cursor.getColumnIndexOrThrow(MIME)
            if (!visitTreeRows(limit, MAX_ROWS, cursor::moveToNext, { cursor.getString(mime) }, directoriesOnly) {
                put(readRow(cursor, uri))
            }) throw TreeBounds()
        }
    }

    // Ownership of a nonnegative descriptor transfers to Rust; failures use negative status.
    fun openFile(context: Context, raw: String): Int = try {
        val uri = requireGrant(context, raw)
        DocumentsContract.getDocumentId(uri)
        context.contentResolver.openFileDescriptor(uri, "r")?.use { it.detachFd() } ?: -1
    } catch (_: SecurityException) {
        -2
    } catch (_: java.io.FileNotFoundException) {
        -3
    } catch (failure: Exception) {
        android.util.Log.w("ShinkiEMU", "SHINKI_EMU_ANDROID_TREE_OPEN: ${failure.javaClass.simpleName}")
        -1
    }

    /** Optional path for the system external-storage provider, never for another authority. */
    fun localPath(context: Context, raw: String): String? {
        if (Build.VERSION.SDK_INT < 30) return null
        return try {
            val uri = requireGrant(context, raw)
            if (uri.encodedAuthority != EXTERNAL_STORAGE) return null
            val provider = context.packageManager.resolveContentProvider(EXTERNAL_STORAGE, 0) ?: return null
            val id = DocumentsContract.getDocumentId(uri)
            val system = provider.applicationInfo.flags and ApplicationInfo.FLAG_SYSTEM != 0
            val (volumeId, components) = localDocumentPath(uri.encodedAuthority, system, id) ?: return null
            val manager = context.getSystemService(StorageManager::class.java) ?: return null
            val volume = manager.storageVolumes.firstOrNull {
                if (volumeId == "primary") it.isPrimary else volumeId.equals(it.uuid, ignoreCase = true)
            }?.directory?.canonicalFile ?: return null
            val candidate = components.fold(volume) { parent, part -> File(parent, part) }
            // Refuse links and check access as this app, not as the picker.
            if (candidate != candidate.canonicalFile || !candidate.isDirectory ||
                !Os.access(candidate.path, OsConstants.R_OK or OsConstants.X_OK)
            ) return null
            candidate.path
        } catch (_: Exception) {
            null // This is only a path hint; the real SAF scan reports provider errors.
        }
    }

    internal fun localDocumentPath(authority: String?, systemProvider: Boolean, id: String): Pair<String, List<String>>? {
        if (authority != EXTERNAL_STORAGE || !systemProvider) return null
        val separator = id.indexOf(':')
        if (separator < 1) return null
        val volume = id.substring(0, separator)
        val relative = id.substring(separator + 1)
        val components = if (relative.isEmpty()) emptyList() else relative.split('/')
        try {
            components.forEach(ShinkiPlatformBridge::validateName)
        } catch (_: IllegalArgumentException) {
            return null
        }
        return volume to components
    }

    private fun treeGrant(uri: Uri): Uri {
        require(uri.scheme == "content" && uri.encodedAuthority?.let { it.isNotEmpty() && '@' !in it } == true)
        require(uri.encodedQuery == null && uri.encodedFragment == null && uri.toString().length <= MAX_URI_LENGTH)
        require(DocumentsContract.isTreeUri(uri)) { "picker did not return a document tree" }
        val parts = uri.pathSegments
        require(parts.size == 2 || (parts.size == 4 && parts[2] == "document"))
        return DocumentsContract.buildTreeDocumentUri(checkNotNull(uri.authority), DocumentsContract.getTreeDocumentId(uri))
    }

    private fun requireGrant(context: Context, raw: String): Uri {
        val uri = Uri.parse(raw)
        val tree = treeGrant(uri)
        if (context.contentResolver.persistedUriPermissions.none { it.isReadPermission && it.uri == tree }) {
            throw SecurityException("folder read permission was revoked; select it again")
        }
        return uri
    }

    private fun readRow(cursor: Cursor, tree: Uri): JSONArray {
        val name = cursor.getString(cursor.getColumnIndexOrThrow(NAME))
        ShinkiPlatformBridge.validateName(name)
        val id = cursor.getString(cursor.getColumnIndexOrThrow(ID))
        val uri = DocumentsContract.buildDocumentUriUsingTree(tree, id).toString()
        require(uri.length <= MAX_URI_LENGTH)
        val directory = cursor.getString(cursor.getColumnIndexOrThrow(MIME)) == DocumentsContract.Document.MIME_TYPE_DIR
        return JSONArray().put(uri).put(name).put(directory)
    }

    private class TreeBounds : IllegalStateException("document directory exceeds the scan bound")

    private inline fun rows(query: JSONArray.() -> Unit): String = try {
        JSONObject().put("rows", JSONArray().apply(query)).toString()
    } catch (failure: Exception) {
        val kind = when (failure) {
            is SecurityException -> "permission"
            is java.io.FileNotFoundException -> "missing"
            is TreeBounds -> "bounds"
            else -> "io"
        }
        JSONObject().put("kind", kind).put("error", failure.toString()).toString()
    }
}

/** Bounds both visited rows and the results returned to the Rust caller. */
internal inline fun visitTreeRows(
    limit: Int,
    maxRows: Int,
    next: () -> Boolean,
    mime: () -> String?,
    directoriesOnly: Boolean,
    visit: () -> Unit,
): Boolean {
    var visited = 0
    var selected = 0
    while (next()) {
        if (visited++ == maxRows) return false
        if (directoriesOnly && mime() != DocumentsContract.Document.MIME_TYPE_DIR) continue
        if (selected++ == limit) return false
        visit()
    }
    return true
}
