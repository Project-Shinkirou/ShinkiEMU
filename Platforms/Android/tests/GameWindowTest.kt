package org.shinkirou.shinkiemu

// Compile with ShinkiGameWindow.kt against the SDK android.jar and run on the host JVM.
private data class WindowState(val orientation: String, val status: Boolean, val navigation: Boolean)

fun main() {
    val library = WindowState("unspecified", status = true, navigation = false)
    val landscape = WindowState("landscape", status = false, navigation = false)
    val shownBars = WindowState("landscape", status = true, navigation = true)
    var state = library
    var captures = 0
    val policy = GameWindowPolicy(
        capture = { captures++; state },
        apply = { immersive -> state = if (immersive) landscape else shownBars },
        restore = { state = it },
    )

    // Synchronous launch failure never enters the game policy.
    policy.set(active = false, immersive = false)
    policy.resume()
    check(state == library && captures == 0)

    // Cancel from the loading view, including a resume during startup.
    policy.set(active = true, immersive = true)
    check(state == landscape && captures == 1)
    state = shownBars // The system temporarily exposed bars while unfocused.
    policy.resume()
    check(state == landscape && captures == 1)
    policy.set(active = false, immersive = false)
    check(state == library)
    policy.resume()
    check(state == library)

    // The next session snapshots a new library orientation/bar state. Toggling
    // bars must not replace it; natural completion restores it once.
    val rotatedLibrary = WindowState("sensor", status = false, navigation = true)
    state = rotatedLibrary
    policy.set(active = true, immersive = true)
    policy.set(active = true, immersive = false)
    check(state == shownBars && captures == 2)
    state = landscape
    policy.resume()
    check(state == shownBars)
    policy.set(active = false, immersive = false)
    check(state == rotatedLibrary)
    state = library
    policy.set(active = false, immersive = false)
    check(state == library)
    println("Android game window lifecycle: passed")
}
