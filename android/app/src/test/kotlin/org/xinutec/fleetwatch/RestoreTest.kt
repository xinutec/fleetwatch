package org.xinutec.fleetwatch

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * What the app is allowed to reopen on. The case that matters is the spent OAuth
 * callback: saving one stranded the Pixel 9 in a launch-and-fail loop it could not get
 * out of, because the dead page was also the only page it would ever load.
 */
class RestoreTest {
    private val base = "https://fleetwatch.xinutec.org/"

    private fun restorable(url: String) = Restore.isRestorable(base, url)

    @Test
    fun `the dashboard root restores`() {
        assertTrue(restorable(base))
    }

    @Test
    fun `app pages restore, with their query and fragment`() {
        assertTrue(restorable(base + "problems"))
        assertTrue(restorable(base + "history?source=amun&collector=vpn-nodes"))
        assertTrue(restorable(base + "problems#muted"))
    }

    @Test
    fun `the spent OAuth callback that stranded the phone never restores`() {
        assertFalse(restorable(base + "auth/callback?state=&code=h1EaSwY19M1ExjvPyAgm"))
    }

    @Test
    fun `neither end of the login round-trip restores`() {
        assertFalse(restorable(base + "login"))
        assertFalse(restorable(base + "login?return_to=%2Fproblems"))
        assertFalse(restorable(base + "logout"))
        assertFalse(restorable(base + "auth/callback"))
    }

    @Test
    fun `a page whose name merely starts with a transient one still restores`() {
        assertTrue(restorable(base + "logins"))
        assertTrue(restorable(base + "authors"))
    }

    @Test
    fun `another origin never restores`() {
        assertFalse(restorable("https://dash.xinutec.org/login/flow"))
        assertFalse(restorable("https://fleetwatch.xinutec.org.evil.test/problems"))
    }

    @Test
    fun `a base written without its trailing slash behaves the same`() {
        // coach's Config.BASE_URL is written that way; the predicate is shared verbatim.
        val bare = "https://fleetwatch.xinutec.org"
        assertTrue(Restore.isRestorable(bare, bare))
        assertTrue(Restore.isRestorable(bare, "$bare/problems"))
        assertFalse(Restore.isRestorable(bare, "$bare/auth/callback?state=&code=x"))
        assertFalse(Restore.isRestorable(bare, "https://fleetwatch.xinutec.org.evil.test/"))
    }
}
