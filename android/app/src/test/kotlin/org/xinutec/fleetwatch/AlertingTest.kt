package org.xinutec.fleetwatch

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * What the poller does, and what it is then allowed to remember.
 *
 * The last two tests are the reason this file exists: the worker used to store the
 * fingerprint BEFORE notifying, so an alert that could not be posted — POST_NOTIFICATIONS
 * denied — was recorded as delivered and the set never announced again. That ordering
 * lived inside a `CoroutineWorker`, which cannot be exercised on the JVM, which is how it
 * went three days without anyone noticing a repository red in CI.
 */
class AlertingTest {
    private val failing =
        Problems.parse(
            """{"checks":[{"source":"mac-mini","collector":"ci-status",
               "section":"builds","label":"memview","verdict":"fail",
               "observed":"failure at d3f7a79"}],"stale":[]}""",
        )
    private val clear = Problems.parse("""{"checks":[],"stale":[]}""")

    @Test
    fun `nothing wrong is a clear`() {
        assertEquals(Step.CLEAR, step(clear, "anything"))
    }

    @Test
    fun `a new problem fires`() {
        assertEquals(Step.FIRE, step(failing, ""))
    }

    @Test
    fun `the same problem stays quiet`() {
        assertEquals(Step.QUIET, step(failing, failing.fingerprint()))
    }

    @Test
    fun `recovery is remembered, so the next failure is news again`() {
        assertEquals("", remembered(Step.CLEAR, delivered = true, now = "", last = "old"))
    }

    @Test
    fun `staying quiet keeps the mark it was quiet about`() {
        assertEquals("old", remembered(Step.QUIET, delivered = true, now = "old", last = "old"))
    }

    @Test
    fun `a delivered alert advances the mark`() {
        assertEquals("new", remembered(Step.FIRE, delivered = true, now = "new", last = "old"))
    }

    @Test
    fun `an UNDELIVERED alert does not advance the mark`() {
        // The defect, in one line: with `now` stored here, the next poll would compare
        // equal and take the quiet branch for ever.
        assertEquals("old", remembered(Step.FIRE, delivered = false, now = "new", last = "old"))
    }

    @Test
    fun `an undelivered alert is still pending on the next poll`() {
        val first = step(failing, "")
        assertEquals(Step.FIRE, first)
        val mark = remembered(first, delivered = false, now = failing.fingerprint(), last = "")
        // The whole point: the permission can be granted at any time, and the next poll
        // must still have something to say.
        assertEquals(Step.FIRE, step(failing, mark))
    }

    // The sequence itself — decide, act, remember — rather than its three pieces. These
    // are the tests that would have failed against the old worker.

    @Test
    fun `a poll that cannot post keeps announcing until one gets through`() {
        val seen = mutableListOf<Step>()
        var granted = false
        var mark = ""

        // Three polls with the permission denied, then it is granted.
        repeat(3) {
            mark =
                poll(failing, mark) { step ->
                    seen += step
                    granted
                }
        }
        assertEquals(listOf(Step.FIRE, Step.FIRE, Step.FIRE), seen)

        granted = true
        mark =
            poll(failing, mark) { step ->
                seen += step
                granted
            }
        assertEquals(Step.FIRE, seen.last())
        // …and only now does it fall quiet, because only now has anyone been told.
        assertEquals(failing.fingerprint(), mark)
        assertEquals(Step.QUIET, step(failing, mark))
    }

    @Test
    fun `a delivered alert falls quiet on the next poll`() {
        val mark = poll(failing, "") { true }
        val second = mutableListOf<Step>()
        poll(failing, mark) { step ->
            second += step
            true
        }
        assertEquals(listOf(Step.QUIET), second)
    }

    @Test
    fun `recovery then a repeat of the same failure is announced again`() {
        val told = poll(failing, "") { true }
        val cleared = poll(clear, told) { true }
        val again = mutableListOf<Step>()
        poll(failing, cleared) { step ->
            again += step
            true
        }
        assertEquals(listOf(Step.FIRE), again)
    }
}
