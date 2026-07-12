package org.xinutec.fleetwatch

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The poller's decisions — parse, "has anything changed", and what the notification says
 * — are pure, so they're tested here without a device.
 */
class ProblemsTest {
    private fun problems(json: String) = Problems.parse(json)

    @Test
    fun `an empty payload is all clear`() {
        val p = problems("""{"checks":[],"stale":[]}""")
        assertTrue(p.isEmpty)
        assertEquals(0, p.count)
        assertEquals("All clear", p.summary())
    }

    @Test
    fun `a failing check is parsed and counted`() {
        val p =
            problems(
                """{"checks":[{"source":"mac-mini","collector":"home-receivers",
               "section":"receivers","label":"pixel5","verdict":"fail",
               "observed":"last push 414 min ago"}],"stale":[]}""",
            )
        assertFalse(p.isEmpty)
        assertEquals(1, p.count)
        assertEquals("pixel5", p.checks.single().label)
        assertEquals("fail", p.checks.single().verdict)
    }

    @Test
    fun `a silent producer counts as a problem`() {
        // The failure mode fleetwatch cares most about: nobody reported at all.
        val p =
            problems("""{"checks":[],"stale":[{"source":"mac-mini","collector":"fleet-health"}]}""")
        assertEquals(1, p.count)
        assertTrue(p.summary().contains("mac-mini/fleet-health silent"))
    }

    @Test
    fun `the real pixel5 outage produces a useful notification`() {
        val p =
            problems(
                """{"checks":[{"source":"mac-mini","collector":"home-receivers",
               "section":"receivers","label":"pixel5","verdict":"fail",
               "observed":"last push 414 min ago"}],"stale":[]}""",
            )
        assertEquals("pixel5 fail", p.summary())
    }

    @Test
    fun `an unchanged problem set keeps its fingerprint`() {
        // The property the whole no-nagging design rests on: the same problems, polled
        // again 30 min later, must not look new.
        val json = """{"checks":[{"source":"s","collector":"c","section":"receivers",
            "label":"pixel5","verdict":"fail","observed":"x"}],"stale":[]}"""
        assertEquals(problems(json).fingerprint(), problems(json).fingerprint())
    }

    @Test
    fun `a new problem changes the fingerprint`() {
        val one =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"receivers",
               "label":"pixel5","verdict":"fail","observed":"x"}],"stale":[]}""",
            )
        val two =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"receivers",
               "label":"pixel5","verdict":"fail","observed":"x"},
               {"source":"s","collector":"c","section":"receivers",
               "label":"bes","verdict":"fail","observed":"y"}],"stale":[]}""",
            )
        assertNotEquals(one.fingerprint(), two.fingerprint())
    }

    @Test
    fun `a changed verdict changes the fingerprint`() {
        // warn -> fail is escalation, and must re-notify rather than be swallowed as
        // "same label, already told you".
        val warn =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"warn","observed":"x"}],"stale":[]}""",
            )
        val fail =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"x"}],"stale":[]}""",
            )
        assertNotEquals(warn.fingerprint(), fail.fingerprint())
    }

    @Test
    fun `a changed observation does not re-notify`() {
        // "last push 20 min ago" becomes "last push 50 min ago" on every poll of the same
        // ongoing outage. If that re-notified, one dead receiver would ping every 30 min
        // forever and you'd learn to ignore the alerts — so `observed` is deliberately
        // NOT part of the fingerprint.
        val early =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"last push 20 min ago"}],"stale":[]}""",
            )
        val later =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"last push 50 min ago"}],"stale":[]}""",
            )
        assertEquals(early.fingerprint(), later.fingerprint())
    }

    @Test
    fun `problem order does not change the fingerprint`() {
        // Row order out of the DB isn't guaranteed; an unchanged set must not look new
        // just because it came back shuffled.
        val a =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"a",
               "verdict":"fail","observed":""},{"source":"s","collector":"c",
               "section":"r","label":"b","verdict":"fail","observed":""}],"stale":[]}""",
            )
        val b =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"b",
               "verdict":"fail","observed":""},{"source":"s","collector":"c",
               "section":"r","label":"a","verdict":"fail","observed":""}],"stale":[]}""",
            )
        assertEquals(a.fingerprint(), b.fingerprint())
    }

    @Test
    fun `a long problem list is summarised, not dumped`() {
        val checks =
            (1..7).joinToString(",") { i ->
                "{\"source\":\"s\",\"collector\":\"c\",\"section\":\"r\"," +
                    "\"label\":\"check$i\",\"verdict\":\"fail\"}"
            }
        val p = problems("""{"checks":[$checks],"stale":[]}""")
        assertEquals(7, p.count)
        assertTrue("summary should elide the tail: ${p.summary()}", p.summary().contains("+4 more"))
    }

    @Test
    fun `malformed json fields degrade instead of throwing`() {
        // The worker must never crash on a payload shape it didn't expect — a monitor that
        // dies on bad input is a monitor that stops monitoring.
        val p = problems("""{"checks":[{"label":"orphan"}],"stale":[{}]}""")
        assertEquals(2, p.count)
        assertEquals("orphan", p.checks.single().label)
    }

    @Test
    fun `missing arrays are treated as no problems`() {
        assertTrue(problems("{}").isEmpty)
    }

    @Test
    fun `a warning alone does not warrant a notification`() {
        // The case that forced this: fleet-health warns, permanently and on purpose, that
        // amun is held a NixOS release behind. A phone that reports a standing decision as
        // news teaches you to swipe fleetwatch away.
        val p =
            problems(
                """{"checks":[{"source":"mac-mini","collector":"fleet-health",
               "section":"NIXOS RELEASE PARITY","label":"release parity","verdict":"warn",
               "observed":"amun on 25.05 — the fleet is on 26.05"}],"stale":[]}""",
            )
        assertEquals(1, p.count) // the dashboard still shows it
        assertTrue(p.notifiable().isEmpty) // the phone stays quiet
    }

    @Test
    fun `a failure alongside a warning notifies about the failure only`() {
        val p =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"x"},{"source":"s","collector":"c","section":"r",
               "label":"release parity","verdict":"warn","observed":"y"}],"stale":[]}""",
            )
        assertEquals(1, p.notifiable().count)
        assertEquals("pixel5 fail", p.notifiable().summary())
    }

    @Test
    fun `a silent producer still notifies even with warnings present`() {
        // Warnings are dropped; a dead producer is not — that is the failure fleetwatch
        // exists to catch, and it must survive the filter.
        val p =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"w",
               "verdict":"warn","observed":""}],
               "stale":[{"source":"mac-mini","collector":"fleet-health"}]}""",
            )
        assertEquals(1, p.notifiable().count)
        assertTrue(p.notifiable().summary().contains("mac-mini/fleet-health silent"))
    }

    @Test
    fun `a warning appearing beside a standing failure does not re-notify`() {
        // The poller fingerprints the notifiable set, so a new warning must not make an
        // unchanged failure look new — otherwise every warning re-rings the old alarm.
        val failOnly =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"x"}],"stale":[]}""",
            )
        val failPlusWarn =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"pixel5",
               "verdict":"fail","observed":"x"},{"source":"s","collector":"c","section":"r",
               "label":"release parity","verdict":"warn","observed":"y"}],"stale":[]}""",
            )
        assertEquals(failOnly.notifiable().fingerprint(), failPlusWarn.notifiable().fingerprint())
    }

    @Test
    fun `a warning escalating to a failure does notify`() {
        // The filter must not swallow escalation: quiet while it is a warning, loud the
        // moment the same check turns into a failure.
        val warn =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"disk",
               "verdict":"warn","observed":"85%"}],"stale":[]}""",
            )
        val fail =
            problems(
                """{"checks":[{"source":"s","collector":"c","section":"r","label":"disk",
               "verdict":"fail","observed":"96%"}],"stale":[]}""",
            )
        assertTrue(warn.notifiable().isEmpty)
        assertEquals(1, fail.notifiable().count)
        assertNotEquals(warn.notifiable().fingerprint(), fail.notifiable().fingerprint())
    }
}
