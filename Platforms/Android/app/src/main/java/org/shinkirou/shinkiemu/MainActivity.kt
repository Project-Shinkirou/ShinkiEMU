package org.shinkirou.shinkiemu

import android.app.Dialog
import android.app.NativeActivity
import android.content.ComponentName
import android.content.Intent
import android.net.Uri
import android.util.Log
import android.view.PointerIcon
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.File
import org.shinkirou.krkr.KrkrHost
import org.shinkirou.krk2.KrkrHost as Krkr2Host

/**
 * NativeActivity host for the Rust Manager (`android_main` in libshinki_emu_manager).
 * Methods without `external` are called from the Slint event-loop thread over JNI;
 * `external` functions are implemented in `platform/android.rs`.
 */
class MainActivity : NativeActivity() {
    companion object {
        init {
            // The framework loads the lib for android.app.NativeActivity under
            // the boot classloader; loading it here also registers it for this
            // app classloader so the `external fun` bridges can bind.
            System.loadLibrary("shinki_emu_manager")
        }

        /** `pickDocument` kinds; must match `PICK_TREE` / `PICK_FILE` in Rust. */
        private const val PICK_TREE = 0
        private const val PICK_FILE = 1
        private const val PICK_LIBRARY_TREE = 2
        private const val FIRST_REQUEST_CODE = 0xA500
    }

    private external fun nativeInitializeHosts(): Int
    private val nativeHosts = java.util.concurrent.CompletableFuture<Int>()
    private val gameWindow by lazy { gameWindowPolicy(this) }

    /** JNI: change only this Activity's game-session presentation. */
    fun setGameWindow(active: Boolean, immersive: Boolean) {
        runOnUiThread {
            if (isFinishing || isDestroyed) return@runOnUiThread
            gameWindow.set(active, immersive)
        }
    }

    /** Called only by the native app thread, never by the UI thread. */
    fun awaitNativeHosts(): Int = nativeHosts.join()

    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        // NativeActivity has started its thread, but onStart has not begun its
        // synchronous poll handshake. Complete all UI-thread core setup here.
        try {
            val status = nativeInitializeHosts()
            nativeHosts.complete(status)
            check(status == 0) { "SHINKI_EMU_ANDROID_HOST_INITIALIZATION ($status)" }
        } catch (failure: Throwable) {
            nativeHosts.completeExceptionally(failure)
            throw failure
        }
    }

    override fun onResume() {
        super.onResume()
        gameWindow.resume()
        KrkrHost.onResume()
        Krkr2Host.onResume()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) gameWindow.resume()
    }

    override fun onPause() {
        KrkrHost.onPause()
        Krkr2Host.onPause()
        super.onPause()
    }

    override fun onDestroy() {
        nativeHosts.completeExceptionally(IllegalStateException("Activity destroyed"))
        // NativeActivity's teardown completes the native app/owner before
        // the private core helpers release their Java Activity references.
        super.onDestroy()
        KrkrHost.destroy(this)
        Krkr2Host.destroy(this)
    }

    private class PendingPick(val requestId: Long, val kind: Int, val destDir: File?)

    // UI thread only.
    private val pendingPicks = HashMap<Int, PendingPick>()
    private var nextRequestCode = FIRST_REQUEST_CODE

    /**
     * Completes `pickDocument`: `path` is a copied path or a library tree URI;
     * a diagnostic code and detail; both null when the user cancelled.
     */
    private external fun nativeOnPickResult(requestId: Long, path: String?, error: String?)

    fun libraryTreeDocument(uri: String): String = ShinkiLibraryTree.document(this, uri)
    fun libraryTreeChildren(uri: String, limit: Int, directoriesOnly: Boolean): String =
        ShinkiLibraryTree.children(this, uri, limit, directoriesOnly)
    fun openLibraryTreeFile(uri: String): Int = ShinkiLibraryTree.openFile(this, uri)
    fun localLibraryTreePath(uri: String): String? = ShinkiLibraryTree.localPath(this, uri)

    fun storageAccess(): Int {
        if (android.os.Build.VERSION.SDK_INT < 30) return 0
        return if (android.os.Environment.isExternalStorageManager()) 2 else 1
    }

    fun openStorageAccess() {
        runOnUiThread {
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                try {
                    startActivity(Intent(
                        android.provider.Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                        Uri.fromParts("package", packageName, null),
                    ))
                } catch (failure: RuntimeException) {
                    nativeOnStorageAccessSettingsError("SHINKI_EMU_ANDROID_STORAGE_SETTINGS: $failure")
                }
            }
        }
    }

    private external fun nativeOnStorageAccessSettingsError(error: String)

    /**
     * Pick a file or tree. Library roots (kind 2) retain only a read grant.
     * Other pickers copy into their own fresh `destDir`.
     */
    fun pickDocument(requestId: Long, kind: Int, destDir: String) {
        runOnUiThread {
            if (isFinishing || isDestroyed) {
                failPick(requestId, "SHINKI_EMU_ANDROID_PICK_REQUEST: activity is closing")
                return@runOnUiThread
            }
            val intent = when (kind) {
                PICK_TREE, PICK_LIBRARY_TREE -> Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                PICK_FILE -> Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "*/*"
                }
                else -> {
                    failPick(requestId, "SHINKI_EMU_ANDROID_PICK_REQUEST: unknown picker kind $kind")
                    return@runOnUiThread
                }
            }.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            if (kind == PICK_LIBRARY_TREE) {
                intent.addFlags(Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION)
            }
            val requestCode = nextRequestCode
            nextRequestCode = if (requestCode == 0xFFFF) FIRST_REQUEST_CODE else requestCode + 1
            pendingPicks[requestCode] = PendingPick(requestId, kind, if (kind == PICK_LIBRARY_TREE) null else File(destDir))
            try {
                // Launch implicitly in this activity's user context. Package visibility can
                // hide the system picker from a PackageManager lookup even when it can launch.
                startActivityForResult(intent, requestCode)
            } catch (failure: RuntimeException) {
                if (pendingPicks.remove(requestCode) != null) {
                    failPick(requestId, pickerFailure(failure, "SHINKI_EMU_ANDROID_PICK_REQUEST"))
                }
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        val pick = pendingPicks.remove(requestCode) ?: return
        handleDocumentPickerResult(resultCode, { data?.data }) { uri, error ->
            when {
                error != null -> failPick(pick.requestId, error)
                uri != null && pick.kind == PICK_LIBRARY_TREE -> {
                    try {
                        val tree = ShinkiLibraryTree.remember(this, uri)
                        nativeOnPickResult(pick.requestId, tree.toString(), null)
                    } catch (failure: RuntimeException) {
                        failPick(pick.requestId, pickerFailure(failure, "SHINKI_EMU_ANDROID_PICK_RESULT"))
                    }
                }
                uri != null -> copyPickedDocument(pick, uri)
                else -> nativeOnPickResult(pick.requestId, null, null)
            }
        }
    }

    private fun copyPickedDocument(pick: PendingPick, uri: Uri) {
        // Copies can be large; keep the UI thread free. The result is posted
        // back to the Slint event loop by the native side.
        try {
            Thread({
                val copied = try {
                    val dest = checkNotNull(pick.destDir)
                    when (pick.kind) {
                        PICK_TREE -> ShinkiPlatformBridge.copyTree(this, uri, dest)
                        PICK_FILE -> ShinkiPlatformBridge.copyDocument(this, uri, dest)
                        else -> error("library folders must not be copied")
                    }
                } catch (failure: Exception) {
                    failPick(pick.requestId, pickerFailure(failure, "SHINKI_EMU_ANDROID_PICK_COPY"))
                    return@Thread
                }
                nativeOnPickResult(pick.requestId, copied.absolutePath, null)
            }, "shinki-pick-copy").start()
        } catch (failure: RuntimeException) {
            failPick(pick.requestId, pickerFailure(failure, "SHINKI_EMU_ANDROID_PICK_COPY"))
        }
    }

    private fun failPick(requestId: Long, error: String) {
        Log.w("ShinkiEMU", "${error.substringBefore(':')} (request $requestId)")
        nativeOnPickResult(requestId, null, error)
    }

    /** Relaunch the activity in a fresh process. */
    fun restartApp() {
        runOnUiThread {
            startActivity(
                Intent.makeRestartActivityTask(ComponentName(this, MainActivity::class.java))
            )
            Runtime.getRuntime().exit(0)
        }
    }

    /** Hide or show the mouse pointer over the activity window. */
    fun setPointerHidden(hidden: Boolean) {
        runOnUiThread {
            val type = if (hidden) PointerIcon.TYPE_NULL else PointerIcon.TYPE_ARROW
            window.decorView.pointerIcon = PointerIcon.getSystemIcon(this, type)
        }
    }

    /** Page shown in the Store sign-in WebView; null while none is open. */
    @Volatile
    private var webLoginPage: String? = null

    // UI thread only.
    private var webLoginDialog: Dialog? = null
    private var webLoginView: WebView? = null

    /**
     * Show the Store sign-in page in a full-screen WebView. The cookie jar is
     * wiped first, so only this sign-in's cookies are read back; back closes it.
     */
    fun openWebLogin(url: String) {
        webLoginPage = url
        runOnUiThread {
            dismissWebLogin()
            val cookies = CookieManager.getInstance()
            cookies.removeAllCookies(null)
            cookies.setAcceptCookie(true)
            val view = WebView(this)
            cookies.setAcceptThirdPartyCookies(view, true)
            view.settings.javaScriptEnabled = true
            view.settings.domStorageEnabled = true
            view.webViewClient = object : WebViewClient() {
                override fun doUpdateVisitedHistory(view: WebView, url: String, isReload: Boolean) {
                    if (webLoginView === view) webLoginPage = url
                }

                override fun onPageFinished(view: WebView, url: String) {
                    if (webLoginView === view) webLoginPage = url
                }
            }
            val dialog = Dialog(this, android.R.style.Theme_DeviceDefault_NoActionBar)
            dialog.setContentView(view)
            dialog.setOnCancelListener {
                webLoginPage = null
                dismissWebLogin()
            }
            webLoginView = view
            webLoginDialog = dialog
            dialog.show()
            view.loadUrl(url)
        }
    }

    fun webLoginPage(): String? = webLoginPage

    /** `Cookie` header the sign-in WebView sends to [url], HttpOnly cookies included. */
    fun webLoginCookies(url: String): String? = CookieManager.getInstance().getCookie(url)

    /** Close the sign-in WebView and wipe its cookies and site data. */
    fun closeWebLogin() {
        webLoginPage = null
        runOnUiThread { dismissWebLogin() }
    }

    private fun dismissWebLogin() {
        webLoginDialog?.dismiss()
        webLoginDialog = null
        webLoginView?.let { view ->
            view.stopLoading()
            view.clearCache(true)
            view.destroy()
        }
        webLoginView = null
        CookieManager.getInstance().removeAllCookies(null)
        CookieManager.getInstance().flush()
        WebStorage.getInstance().deleteAllData()
    }
}
