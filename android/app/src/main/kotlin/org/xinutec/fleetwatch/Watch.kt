package org.xinutec.fleetwatch

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.TimeUnit

/**
 * The background half of the app: asks fleetwatch "is anything wrong?" every 30 minutes
 * and raises a notification when the answer *changes*.
 *
 * fleetwatch is a pull-based dashboard — it never reaches out — which means a problem is
 * only ever as visible as someone's willingness to open the page. That gap is not
 * hypothetical: the pixel5 sensor receiver went deaf and stayed silent for 7 hours,
 * caught only because a human happened to notice a line missing from a chart. So the
 * phone asks on a timer; the server stays passive.
 *
 * Battery: WorkManager coalesces this into existing wakeups (30 min is well above its
 * 15-minute floor) and it only runs with a network. One HTTPS GET per run.
 *
 * Auth: a read token (`FLEETWATCH_READ_TOKENS` server-side), not the WebView's Nextcloud
 * session — a background worker can't complete an interactive login, and a cookie that
 * quietly expires would leave a monitor that silently stops monitoring.
 */
class ProblemsWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val token = Prefs.readToken(applicationContext)
        if (token.isEmpty()) {
            // No token = not configured. Say so once, in the log; do NOT notify, or a
            // fresh install would nag forever about its own setup.
            Log.w(TAG, "no read token set — poller idle (see android/README.md)")
            return Result.success()
        }

        val problems =
            try {
                Problems.parse(fetch(PROBLEMS_URL, token))
            } catch (e: Exception) {
                // A failed poll is retried, not reported: the phone is often off the VPN,
                // and "can't reach fleetwatch" is usually a statement about the phone, not
                // the fleet. WorkManager backs off and tries again.
                Log.w(TAG, "poll failed: ${e.message}")
                return Result.retry()
            }

        val prefs = Prefs.of(applicationContext)
        val last = prefs.getString(KEY_LAST_FINGERPRINT, "") ?: ""
        val now = problems.fingerprint()
        prefs.edit { putString(KEY_LAST_FINGERPRINT, now) }

        when {
            // Nothing wrong: clear any standing alert, so the notification tracks reality
            // instead of lingering after the fleet recovers.
            problems.isEmpty -> notificationManager().cancel(NOTIFICATION_ID)

            // Same problems as last poll: already told you. Staying quiet is what keeps
            // the notification meaningful when it does fire.
            now == last -> Log.i(TAG, "unchanged (${problems.count} problem(s))")

            else -> notify(problems)
        }
        return Result.success()
    }

    private fun fetch(url: String, token: String): String {
        val conn =
            (URL(url).openConnection() as HttpURLConnection).apply {
                requestMethod = "GET"
                setRequestProperty("Authorization", "Bearer $token")
                connectTimeout = TIMEOUT_MS
                readTimeout = TIMEOUT_MS
            }
        try {
            val code = conn.responseCode
            if (code != HttpURLConnection.HTTP_OK) {
                throw IllegalStateException("HTTP $code from $url")
            }
            return conn.inputStream.bufferedReader().use { it.readText() }
        } finally {
            conn.disconnect()
        }
    }

    private fun notificationManager(): NotificationManager =
        applicationContext.getSystemService(NotificationManager::class.java)

    private fun notify(problems: Problems) {
        // Android 13+ can refuse to post without the runtime permission. MainActivity asks
        // for it; if it was denied, drop the notification rather than crash the worker.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(
                applicationContext,
                Manifest.permission.POST_NOTIFICATIONS,
            ) != PackageManager.PERMISSION_GRANTED
        ) {
            Log.w(TAG, "POST_NOTIFICATIONS not granted — cannot alert")
            return
        }

        notificationManager().createNotificationChannel(
            NotificationChannel(CHANNEL_ID, "Fleet problems", NotificationManager.IMPORTANCE_HIGH),
        )

        val open =
            PendingIntent.getActivity(
                applicationContext,
                0,
                Intent(applicationContext, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                    putExtra(MainActivity.EXTRA_OPEN_PROBLEMS, true)
                },
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )

        val title =
            if (problems.count == 1) "1 fleet problem" else "${problems.count} fleet problems"
        val notification =
            NotificationCompat
                .Builder(applicationContext, CHANNEL_ID)
                .setContentTitle(title)
                .setContentText(problems.summary())
                .setStyle(NotificationCompat.BigTextStyle().bigText(problems.summary()))
                .setSmallIcon(android.R.drawable.stat_notify_error)
                .setCategory(NotificationCompat.CATEGORY_STATUS)
                .setAutoCancel(true)
                .setContentIntent(open)
                .build()

        notificationManager().notify(NOTIFICATION_ID, notification)
        Log.i(TAG, "notified: ${problems.summary()}")
    }

    companion object {
        private const val TAG = "ProblemsWorker"
        private const val CHANNEL_ID = "fleet-problems"
        private const val NOTIFICATION_ID = 1
        private const val TIMEOUT_MS = 20_000
        private const val KEY_LAST_FINGERPRINT = "last_fingerprint"
        private const val PROBLEMS_URL = "https://fleetwatch.xinutec.org/api/problems"
        private const val WORK_NAME = "fleet-problems-poll"

        /** How often to ask. Above WorkManager's 15-minute floor; cheap enough to ignore. */
        val INTERVAL_MINUTES = 30L

        /**
         * Schedule the poll. Idempotent — KEEP means reopening the app doesn't reset the
         * timer, so the schedule survives app launches rather than restarting on each one.
         */
        fun schedule(ctx: Context) {
            val work =
                PeriodicWorkRequestBuilder<ProblemsWorker>(
                    INTERVAL_MINUTES,
                    TimeUnit.MINUTES,
                ).setConstraints(
                    Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build(),
                ).build()

            WorkManager.getInstance(ctx).enqueueUniquePeriodicWork(
                WORK_NAME,
                ExistingPeriodicWorkPolicy.KEEP,
                work,
            )
        }
    }
}

/**
 * App state: the last problem set we alerted about (SharedPreferences), and the read
 * token.
 *
 * The token is a secret, so — as in the govee app — it lives in a plain file in private
 * storage rather than SharedPreferences (which are backed up), written once by
 * `deploy.sh` over `adb run-as`. It never lands in argv, a log, or this repo.
 */
object Prefs {
    private const val FILE = "fleetwatch"
    private const val TOKEN_FILE = "read_token"

    fun of(ctx: Context): android.content.SharedPreferences =
        ctx.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    fun readToken(ctx: Context): String =
        runCatching {
            java.io
                .File(ctx.filesDir, TOKEN_FILE)
                .readText()
                .trim()
        }.getOrDefault("")

    fun setReadToken(ctx: Context, token: String) {
        java.io.File(ctx.filesDir, TOKEN_FILE).writeText(token.trim())
    }
}
