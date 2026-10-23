package org.shinkirou.shinkiemu

import android.app.Activity
import android.database.Cursor
import android.net.Uri
import android.provider.DocumentsContract
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.io.File
import java.security.KeyStore
import java.security.MessageDigest
import java.util.ArrayDeque
import java.util.HashSet
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Static helpers for the native Manager. The secret functions are called over
 * JNI from `platform/android.rs`; the copy helpers back `MainActivity.pickDocument`.
 */
object ShinkiPlatformBridge {
    private const val SECRET_PREFERENCES = "shinkiemu-secure-references-v1"
    private const val MAX_SECRET_BYTES = 16 * 1024
    private const val MAX_TREE_ENTRIES = 100_000

    @JvmStatic
    fun storeSecret(activity: Activity, reference: String, secret: String): String {
        validateSecretReference(reference)
        require(secret.isNotEmpty() && secret.toByteArray(Charsets.UTF_8).size <= MAX_SECRET_BYTES)
        val key = secretKey(reference, create = true)
            ?: throw SecurityException("failed to create Android Keystore key")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val encrypted = cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
        val encoded = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) + "." +
            Base64.encodeToString(encrypted, Base64.NO_WRAP)
        val committed = activity.getSharedPreferences(SECRET_PREFERENCES, Activity.MODE_PRIVATE)
            .edit()
            .putString(reference, encoded)
            .commit()
        check(committed) { "secure reference persistence failed" }
        return ""
    }

    @JvmStatic
    fun resolveSecret(activity: Activity, reference: String): String {
        validateSecretReference(reference)
        val encoded = activity.getSharedPreferences(SECRET_PREFERENCES, Activity.MODE_PRIVATE)
            .getString(reference, null)
            ?: throw SecurityException("secure reference is missing")
        val parts = encoded.split('.', limit = 2)
        require(parts.size == 2)
        val iv = Base64.decode(parts[0], Base64.NO_WRAP)
        val encrypted = Base64.decode(parts[1], Base64.NO_WRAP)
        require(iv.size in 12..16 && encrypted.size <= MAX_SECRET_BYTES + 32)
        val key = secretKey(reference, create = false)
            ?: throw SecurityException("Android Keystore key is missing")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
        return cipher.doFinal(encrypted).toString(Charsets.UTF_8)
    }

    /** Remove the stored ciphertext and its Keystore key; missing entries are not an error. */
    @JvmStatic
    fun deleteSecret(activity: Activity, reference: String): String {
        validateSecretReference(reference)
        val committed = activity.getSharedPreferences(SECRET_PREFERENCES, Activity.MODE_PRIVATE)
            .edit()
            .remove(reference)
            .commit()
        check(committed) { "secure reference removal failed" }
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val alias = secretAlias(reference)
        if (keyStore.containsAlias(alias)) keyStore.deleteEntry(alias)
        return ""
    }

    /**
     * Copy the SAF tree `treeUri` to `destDir/<tree name>/` and return that
     * directory. `destDir` must not exist yet.
     */
    @JvmStatic
    fun copyTree(activity: Activity, treeUri: Uri, destDir: File): File {
        require(treeUri.scheme == "content")
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        val rootUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, rootId)
        val target = File(createDestination(destDir), displayName(activity, rootUri))
        check(target.mkdir()) { "cannot create $target" }
        val pending = ArrayDeque<Pair<String, File>>()
        val visitedDirectories = HashSet<String>()
        var entries = 0
        pending.add(rootId to target)
        while (pending.isNotEmpty()) {
            val (parentId, parentDir) = pending.removeFirst()
            check(visitedDirectories.add(parentId)) { "document provider directory cycle" }
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentId)
            activity.contentResolver.query(children, PROJECTION, null, null, null).use { cursor ->
                requireNotNull(cursor) { "document provider returned no cursor" }
                while (cursor.moveToNext()) {
                    entries += 1
                    check(entries <= MAX_TREE_ENTRIES) { "document entry bound exceeded" }
                    val id = cursor.requiredString(DOCUMENT_ID)
                    val name = cursor.requiredString(DISPLAY_NAME)
                    validateName(name)
                    val child = File(parentDir, name)
                    check(!child.exists()) { "duplicate document name $name" }
                    if (cursor.requiredString(MIME_TYPE) == DocumentsContract.Document.MIME_TYPE_DIR) {
                        check(child.mkdir()) { "cannot create $child" }
                        pending.add(id to child)
                    } else {
                        copyStream(activity, DocumentsContract.buildDocumentUriUsingTree(treeUri, id), child)
                    }
                }
            }
        }
        return target
    }

    /**
     * Copy the single SAF document `documentUri` to `destDir/<display name>`
     * and return that file. `destDir` must not exist yet.
     */
    @JvmStatic
    fun copyDocument(activity: Activity, documentUri: Uri, destDir: File): File {
        require(documentUri.scheme == "content")
        val target = File(createDestination(destDir), displayName(activity, documentUri))
        copyStream(activity, documentUri, target)
        return target
    }

    /** A single path component: no separators, NUL, `.` or `..`. */
    @JvmStatic
    fun validateName(value: String) {
        require(value.isNotEmpty() && value.length <= 255) { "invalid document name" }
        require(value != "." && value != "..") { "invalid document name $value" }
        require(!value.contains('/') && !value.contains('\\') && !value.contains('\u0000')) {
            "invalid document name $value"
        }
    }

    private fun createDestination(destDir: File): File {
        check(!destDir.exists()) { "destination $destDir already exists" }
        check(destDir.mkdirs()) { "cannot create $destDir" }
        return destDir
    }

    private fun displayName(activity: Activity, uri: Uri): String {
        activity.contentResolver.query(uri, arrayOf(DISPLAY_NAME), null, null, null).use { cursor ->
            requireNotNull(cursor) { "document provider returned no cursor" }
            check(cursor.moveToFirst()) { "document provider returned no row" }
            return cursor.requiredString(DISPLAY_NAME).also(::validateName)
        }
    }

    private fun copyStream(activity: Activity, uri: Uri, target: File) {
        activity.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "document provider returned no input stream" }
            target.outputStream().use { output -> input.copyTo(output, 64 * 1024) }
        }
    }

    private fun secretAlias(reference: String): String = "shinkiemu." + Base64.encodeToString(
        MessageDigest.getInstance("SHA-256").digest(reference.toByteArray(Charsets.UTF_8)),
        Base64.NO_WRAP or Base64.NO_PADDING or Base64.URL_SAFE,
    )

    private fun secretKey(reference: String, create: Boolean): SecretKey? {
        val alias = secretAlias(reference)
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }
        if (!create) return null
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private fun validateSecretReference(reference: String) {
        require(reference.isNotEmpty() && reference.length <= 128)
        require(reference.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '-' || it == '_' })
    }

    private fun Cursor.requiredString(column: String): String {
        val index = getColumnIndex(column)
        require(index >= 0 && !isNull(index)) { "missing document column" }
        return getString(index)
    }

    private val DOCUMENT_ID = DocumentsContract.Document.COLUMN_DOCUMENT_ID
    private val DISPLAY_NAME = DocumentsContract.Document.COLUMN_DISPLAY_NAME
    private val MIME_TYPE = DocumentsContract.Document.COLUMN_MIME_TYPE
    private val PROJECTION = arrayOf(DOCUMENT_ID, DISPLAY_NAME, MIME_TYPE)
}
