package app.mirror.vault.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ServerAddressTest {
    @Test
    fun bareHostsGetTheRightSchemeByNetwork() {
        assertEquals("https://photos.example.com", normalizeServer("photos.example.com/"))
        assertEquals("http://10.0.2.2:8080", normalizeServer("10.0.2.2:8080"))
        assertEquals("http://192.168.1.20", normalizeServer("192.168.1.20"))
        assertEquals("http://localhost:8080", normalizeServer("localhost:8080"))
    }

    @Test
    fun typedSchemeIsCaseFoldedSoUppercaseMeansTheSameThing() {
        assertEquals("http://10.0.2.2:8080", normalizeServer("HTTP://10.0.2.2:8080"))
        val check = checkServer("HTTP://10.0.2.2:8080", allowLocalHttp = false)
        assertTrue("uppercase scheme must still ask for local consent: $check", check is ServerCheck.NeedsLocalConsent)
    }

    @Test
    fun privateHttpNeedsConsentThenIsAccepted() {
        assertTrue(checkServer("10.0.2.2:8080", false) is ServerCheck.NeedsLocalConsent)
        assertEquals(
            ServerCheck.Ok("http://10.0.2.2:8080", local = true),
            checkServer("10.0.2.2:8080", true),
        )
    }

    @Test
    fun publicHttpIsRefusedEvenWithConsent() {
        assertTrue(checkServer("http://photos.example.com", true) is ServerCheck.Invalid)
    }

    @Test
    fun pathsAreRejectedUpFrontInsteadOfAfterConnecting() {
        val check = checkServer("10.0.2.2:8080/vault", true)
        assertTrue("$check", check is ServerCheck.Invalid)
        assertTrue((check as ServerCheck.Invalid).message.contains("no path"))
        assertTrue(checkServer("https://photos.example.com/app", false) is ServerCheck.Invalid)
    }

    @Test
    fun httpsHostsAreAcceptedAndBlankIsEmpty() {
        assertEquals(ServerCheck.Ok("https://photos.example.com", local = false), checkServer("photos.example.com", false))
        assertEquals(ServerCheck.Empty, checkServer("   ", false))
    }

    @Test
    fun unsupportedSchemesAndGarbageAreInvalid() {
        assertTrue(checkServer("ftp://example.com", false) is ServerCheck.Invalid)
        assertTrue(checkServer("https://", false) is ServerCheck.Invalid)
    }
}
