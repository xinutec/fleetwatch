package org.xinutec.fleetwatch

import org.json.JSONArray
import org.json.JSONObject

/**
 * The parsed shape of `GET /api/problems` — "what's wrong right now".
 *
 * Two kinds of trouble, deliberately distinct:
 *  - [checks]: a check whose latest verdict is fail/warn (something reported bad news).
 *  - [stale]: a producer that has gone quiet (nobody reported at all).
 *
 * The second is the one that matters most and is easiest to miss: a monitor whose worst
 * failure mode is a dead producer looking green. Both are surfaced.
 */
data class Problems(
    val checks: List<Problem>,
    val stale: List<Stale>,
) {
    val isEmpty: Boolean get() = checks.isEmpty() && stale.isEmpty()
    val count: Int get() = checks.size + stale.size

    /**
     * The subset worth waking a phone for: failures and silent producers. Warnings are
     * dropped.
     *
     * A warning is something to know, not something to do — and some of them are true
     * indefinitely by design (amun is *deliberately* held a NixOS release behind, and the
     * fleet check says so on every run, forever). Letting those reach the notification
     * would mean the phone reports a standing decision as if it were news, and the first
     * thing that teaches you is to stop reading fleetwatch notifications — at which point
     * the real failure, the one this app exists for, arrives in a channel you've learned
     * to ignore. So warnings live on the dashboard, where you go to look; the notification
     * is reserved for what is actually broken.
     */
    fun notifiable(): Problems = copy(checks = checks.filter { it.verdict != WARN })

    /**
     * A stable identity for "the current problem set", used to notify only on CHANGE.
     *
     * Re-notifying every 30 minutes about a problem already seen trains you to swipe the
     * notification away without reading it — at which point the alert is worse than
     * nothing, because it looks like it's working. Sorted so map/DB ordering can't make
     * an unchanged set look new.
     *
     * Each entry is a JSON array, not concatenation: JSON escapes the quotes, so no
     * field content can forge an entry boundary — with `/`-joined strings, a crafted
     * label could make two DIFFERENT problem sets fingerprint-equal, and a matching
     * fingerprint here means a missed notification.
     */
    fun fingerprint(): String =
        (
            checks.map {
                JSONArray(listOf(it.source, it.collector, it.section, it.label, it.verdict))
                    .toString()
            } +
                stale.map { JSONArray(listOf(it.source, it.collector, "stale")).toString() }
        ).sorted()
            .joinToString("|")

    /** One line for the notification body: the worst offenders, named. */
    fun summary(): String {
        val parts =
            stale.map { "${it.source}/${it.collector} silent" } +
                checks.map { "${it.label} ${it.verdict}" }
        return when {
            parts.isEmpty() -> "All clear"
            parts.size <= MAX_NAMED -> parts.joinToString(", ")
            else -> parts.take(MAX_NAMED).joinToString(", ") + " +${parts.size - MAX_NAMED} more"
        }
    }

    companion object {
        private const val MAX_NAMED = 3

        /** The API's verdict string for a warning (src/report/types.rs). */
        const val WARN = "warn"

        /** Parse the API payload. Unknown/missing fields degrade to empty, never throw. */
        fun parse(json: String): Problems {
            val root = JSONObject(json)
            return Problems(
                checks =
                    root.optJSONArray("checks").objects().map { o ->
                        Problem(
                            source = o.optString("source"),
                            collector = o.optString("collector"),
                            section = o.optString("section"),
                            label = o.optString("label"),
                            verdict = o.optString("verdict"),
                            // NOT optString: the wire writes absent optionals as an
                            // explicit `"observed": null`, and Android's org.json
                            // (unlike the upstream one on the test classpath) renders
                            // that as the literal string "null".
                            observed = if (o.isNull("observed")) null else o.optString("observed"),
                        )
                    },
                stale =
                    root.optJSONArray("stale").objects().map { o ->
                        Stale(
                            source = o.optString("source"),
                            collector = o.optString("collector"),
                        )
                    },
            )
        }

        private fun JSONArray?.objects(): List<JSONObject> =
            (0 until (this?.length() ?: 0)).mapNotNull { i -> this?.optJSONObject(i) }
    }
}

data class Problem(
    val source: String,
    val collector: String,
    val section: String,
    val label: String,
    val verdict: String,
    val observed: String?,
)

data class Stale(
    val source: String,
    val collector: String,
)
