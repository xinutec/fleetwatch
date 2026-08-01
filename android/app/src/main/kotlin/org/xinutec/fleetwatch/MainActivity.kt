package org.xinutec.fleetwatch

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import org.xinutec.shell.ShellConfig
import org.xinutec.shell.WebShellActivity

/**
 * fleetwatch — the fleet monitoring dashboard, an Angular app served at
 * [FLEETWATCH_URL], in the fleet's shared [WebShellActivity]. It's private
 * (reachable only over the VPN) and reads are behind a Nextcloud login, which the
 * WebView performs interactively; the app itself needs only `INTERNET`.
 *
 * What the shell does not do, and this file does: ask for the notification
 * permission the background poller needs, keep that poller scheduled, and send a
 * tapped problem notification to the problems page.
 */
class MainActivity : WebShellActivity() {
    override val shell = ShellConfig(url = FLEETWATCH_URL)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermission()
        // Idempotent (KEEP): opening the app doesn't restart the 30-minute cycle.
        ProblemsWorker.schedule(this)
    }

    /** Tapped a problem notification? Go straight to what's wrong, not to wherever
     *  the app happened to be left. */
    override fun startUrl(intent: Intent?): String? =
        PROBLEMS_URL.takeIf { intent?.getBooleanExtra(EXTRA_OPEN_PROBLEMS, false) == true }

    /**
     * Ask for POST_NOTIFICATIONS (API 33+). Without it the poller runs but cannot tell
     * you anything — a monitor that watches in silence. Asked once on launch; if it's
     * refused, Android simply won't show the alerts and the dashboard still works.
     */
    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    private val notificationPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { /* best effort */ }

    companion object {
        // The fleetwatch dashboard (HTTPS, VPN-only; reads are behind a Nextcloud login).
        private const val FLEETWATCH_URL = "https://fleetwatch.xinutec.org/"
        private const val PROBLEMS_URL = "https://fleetwatch.xinutec.org/problems"

        /** Set by the poller's notification: open on the problems page, not the last page. */
        const val EXTRA_OPEN_PROBLEMS = "open_problems"
    }
}
