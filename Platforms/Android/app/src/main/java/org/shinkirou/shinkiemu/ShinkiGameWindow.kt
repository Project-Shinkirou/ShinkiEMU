package org.shinkirou.shinkiemu

import android.app.Activity
import android.content.pm.ActivityInfo
import android.graphics.Color
import android.os.Build
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController

/** UI-thread policy: snapshot once on entry; a toggle or resume must not replace it. */
internal class GameWindowPolicy<S : Any>(
    private val capture: () -> S,
    private val apply: (immersive: Boolean) -> Unit,
    private val restore: (S) -> Unit,
) {
    private var previous: S? = null
    private var immersive = false

    fun set(active: Boolean, immersive: Boolean) {
        if (!active) {
            val snapshot = previous ?: return
            previous = null
            restore(snapshot)
        } else {
            if (previous == null) {
                previous = capture()
            } else if (this.immersive == immersive) {
                return
            }
            this.immersive = immersive
            apply(immersive)
        }
    }

    fun resume() {
        if (previous != null) apply(immersive)
    }
}

@Suppress("DEPRECATION") // Bar visibility uses decor flags on API 28–29.
internal fun gameWindowPolicy(activity: Activity): GameWindowPolicy<*> {
    val window = activity.window
    val decor = window.decorView
    val modern = Build.VERSION.SDK_INT >= 30
    val colorsSupported = Build.VERSION.SDK_INT < 35 // Target SDK 37 enforces edge-to-edge.
    val statusContrastSupported = Build.VERSION.SDK_INT in 29..34
    val navigationContrastSupported = Build.VERSION.SDK_INT >= 29
    val legacyHide = View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
        View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
    val legacyMask = legacyHide or View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or
        View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
    val lightBars = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
        WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS

    data class Saved(
        val orientation: Int,
        val legacyFlags: Int,
        val appearance: Int,
        val behavior: Int,
        val statusVisible: Boolean,
        val navigationVisible: Boolean,
        val statusColor: Int,
        val navigationColor: Int,
        val statusContrast: Boolean,
        val navigationContrast: Boolean,
    )

    return GameWindowPolicy(
        capture = {
            val controller = if (modern) window.insetsController else null
            val insets = if (modern) {
                decor.rootWindowInsets ?: activity.windowManager.currentWindowMetrics.windowInsets
            } else {
                null
            }
            Saved(
                activity.requestedOrientation,
                decor.systemUiVisibility and legacyMask,
                controller?.systemBarsAppearance ?: 0,
                controller?.systemBarsBehavior ?: WindowInsetsController.BEHAVIOR_DEFAULT,
                insets?.isVisible(WindowInsets.Type.statusBars()) ?: true,
                insets?.isVisible(WindowInsets.Type.navigationBars()) ?: true,
                if (colorsSupported) window.statusBarColor else Color.TRANSPARENT,
                if (colorsSupported) window.navigationBarColor else Color.TRANSPARENT,
                statusContrastSupported && window.isStatusBarContrastEnforced,
                navigationContrastSupported && window.isNavigationBarContrastEnforced,
            )
        },
        apply = { immersive ->
            if (activity.requestedOrientation != ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE) {
                activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
            }
            if (colorsSupported) {
                window.statusBarColor = Color.BLACK
                window.navigationBarColor = Color.BLACK
            }
            if (statusContrastSupported) window.isStatusBarContrastEnforced = false
            if (navigationContrastSupported) window.isNavigationBarContrastEnforced = false
            if (modern) {
                window.insetsController?.apply {
                    setSystemBarsAppearance(0, lightBars)
                    systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                    val bars = WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars()
                    if (immersive) hide(bars) else show(bars)
                }
            } else {
                decor.systemUiVisibility = (decor.systemUiVisibility and legacyMask.inv()) or
                    (if (immersive) legacyHide else 0)
            }
        },
        restore = { saved ->
            if (colorsSupported) {
                window.statusBarColor = saved.statusColor
                window.navigationBarColor = saved.navigationColor
            }
            if (statusContrastSupported) window.isStatusBarContrastEnforced = saved.statusContrast
            if (navigationContrastSupported) window.isNavigationBarContrastEnforced = saved.navigationContrast
            if (modern) {
                window.insetsController?.apply {
                    setSystemBarsAppearance(saved.appearance, lightBars)
                    systemBarsBehavior = saved.behavior
                    val status = WindowInsets.Type.statusBars()
                    val navigation = WindowInsets.Type.navigationBars()
                    if (saved.statusVisible) show(status) else hide(status)
                    if (saved.navigationVisible) show(navigation) else hide(navigation)
                }
            } else {
                decor.systemUiVisibility = (decor.systemUiVisibility and legacyMask.inv()) or saved.legacyFlags
            }
            if (activity.requestedOrientation != saved.orientation) {
                activity.requestedOrientation = saved.orientation
            }
        },
    )
}
