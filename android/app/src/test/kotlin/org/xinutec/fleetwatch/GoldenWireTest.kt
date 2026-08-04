package org.xinutec.fleetwatch

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * Parses the committed golden `/api/problems` fixture — generated from the real
 * Rust wire types by `tests/golden_wire.rs` — and asserts on every field this
 * app consumes.
 *
 * The parser deliberately degrades instead of throwing, which means a renamed
 * server field would not crash anything: the poller would just quietly stop
 * seeing problems, the exact failure fleetwatch exists to catch. (The web UI
 * doesn't have this hole — its types are generated from the Rust ones.) This
 * test is the Android side of the drift gate: if either side moves, the build
 * breaks here instead of the monitor going blind in production.
 */
class GoldenWireTest {
    private fun golden(): String {
        var dir: File? = File(System.getProperty("user.dir")).absoluteFile
        while (dir != null) {
            val f = File(dir, "tests/golden/problems.json")
            if (f.isFile) return f.readText()
            dir = dir.parentFile
        }
        throw AssertionError(
            "tests/golden/problems.json not found above ${System.getProperty("user.dir")} — " +
                "regenerate with `cargo test export_golden`",
        )
    }

    @Test
    fun `every field the poller consumes survives the wire`() {
        val p = Problems.parse(golden())

        // Two problem checks + one silent collector. The fixture's `muted` array is
        // deliberately not represented: muted checks are already-suppressed by the
        // server and this app must keep ignoring them.
        assertEquals(2, p.checks.size)
        assertEquals(1, p.stale.size)

        val fail = p.checks[0]
        assertEquals("mac-mini", fail.source)
        assertEquals("home-receivers", fail.collector)
        assertEquals("receivers", fail.section)
        assertEquals("pixel5", fail.label)
        assertEquals("fail", fail.verdict)
        assertEquals("last push 414 min ago", fail.observed)

        val warn = p.checks[1]
        assertEquals("warn", warn.verdict)
        // The wire writes absent optionals as explicit nulls; they must come out as
        // Kotlin nulls, not "" or the string "null" (Android's org.json does that).
        assertNull(warn.observed)

        val stale = p.stale.single()
        assertEquals("isis", stale.source)
        assertEquals("backup-verify", stale.collector)
    }

    @Test
    fun `the poller's decisions come out right on real wire data`() {
        val p = Problems.parse(golden())

        // The warn is dropped from the notifiable set; the fail and the silent
        // collector survive.
        assertEquals(3, p.count)
        assertEquals(2, p.notifiable().count)
        assertEquals("isis/backup-verify silent, pixel5 fail", p.notifiable().summary())

        // The fingerprint keys on fields that must therefore all be parsed for
        // real: re-parsing the same wire data reproduces it exactly, and the
        // dropped warn distinguishes it from the full set's.
        val fp = p.notifiable().fingerprint()
        assertEquals(fp, Problems.parse(golden()).notifiable().fingerprint())
        assertNotEquals(fp, p.fingerprint())
    }
}
