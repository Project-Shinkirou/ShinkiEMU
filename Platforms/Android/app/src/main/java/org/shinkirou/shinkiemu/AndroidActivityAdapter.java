package org.shinkirou.shinkiemu;

import android.app.Activity;

/** Lifecycle for a packaged platform adapter. Callbacks run on the Activity UI thread. */
public interface AndroidActivityAdapter {
    void attached(Activity activity);
    void resumed();
    void paused();

    /** Cancel pending UI work before the native owner's shutdown can block the UI thread. */
    void detaching(Activity activity);

    /** Release Activity-bound services after the native owner has stopped. */
    void detached(Activity activity);
}
