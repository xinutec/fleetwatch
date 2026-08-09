package org.xinutec.fleetwatch

/**
 * What a poll should do, and what it may remember afterwards.
 *
 * ⚠ **This exists because remembering came BEFORE telling, and one path told nobody.**
 * `ProblemsWorker` used to write the fingerprint into preferences and only then decide
 * whether to notify — and `notify()` returns early, without posting anything, when
 * POST_NOTIFICATIONS has been denied (Android 13+). The stored fingerprint then said
 * "already told you" about a notification that was never delivered, so every later poll
 * took the unchanged branch and stayed quiet. **Silent for ever, for that problem set**,
 * including after the permission was granted, because nothing re-derives what the phone
 * has actually shown you.
 *
 * The rule is one sentence and it is the whole of this file: *what is remembered is what
 * was DELIVERED, never what was merely computed.* It lives here, free of Android types,
 * so it is a test rather than a comment — `Watch.kt` cannot be unit-tested on the JVM,
 * which is exactly how the ordering went unexamined.
 */
enum class Step {
    /** Nothing is wrong: cancel any standing notification. */
    CLEAR,

    /** The same problems as last time. Already said; saying it again teaches you to swipe. */
    QUIET,

    /** Something is newly wrong, or newly worse. Tell them. */
    FIRE,
}

/** What this poll should do, given the fingerprint the phone last *delivered*. */
fun step(alerting: Problems, last: String): Step =
    when {
        alerting.isEmpty -> Step.CLEAR
        alerting.fingerprint() == last -> Step.QUIET
        else -> Step.FIRE
    }

/**
 * The fingerprint to store after acting — `delivered` says whether the notification
 * actually reached the shade.
 *
 * A [Step.FIRE] that could not be delivered keeps the OLD value on purpose, so the next
 * poll sees a changed set and tries again. That is the difference between a monitor that
 * retries and one that has quietly given up, and it costs one retry every 30 minutes
 * against a permission the user can grant at any time.
 */
fun remembered(step: Step, delivered: Boolean, now: String, last: String): String =
    when (step) {
        // Recovery has to be recorded, or the next failure would look unchanged and
        // never be announced.
        Step.CLEAR -> now

        Step.QUIET -> last

        Step.FIRE -> if (delivered) now else last
    }

/**
 * The whole sequence — decide, act, remember — with the acting passed in.
 *
 * The order is the defect, so the order is what wants testing, and it cannot be tested
 * where it used to live: inside a `CoroutineWorker`, which needs a device. Taking [act]
 * as a parameter puts the sequence on the JVM side of that line while leaving the
 * Android calls (cancel, log, post) in `Watch.kt` where they belong.
 *
 * @param act performs the step and answers whether the phone was actually told.
 * @return the fingerprint to store.
 */
fun poll(alerting: Problems, last: String, act: (Step) -> Boolean): String {
    val step = step(alerting, last)
    return remembered(step, act(step), alerting.fingerprint(), last)
}
