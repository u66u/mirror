package app.mirror.vault.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class ServerEndpointTest {
    @Test
    fun httpRequiresExplicitConsentAndPrivateAddressLiteral() {
        assertThrows(IllegalArgumentException::class.java) {
            ServerEndpoint.parse("http://192.168.1.20:8080", false)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ServerEndpoint.parse("http://photos.example.test", true)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ServerEndpoint.parse("http://8.8.8.8", true)
        }

        assertEquals(
            "http://192.168.1.20:8080",
            ServerEndpoint.parse("http://192.168.1.20:8080/", true).baseUrl,
        )
        assertEquals(
            "http://[fd00::20]:8080",
            ServerEndpoint.parse("http://[fd00::20]:8080", true).baseUrl,
        )
    }

    @Test
    fun httpsAcceptsDomainButRejectsNonOriginComponents() {
        assertEquals(
            "https://photos.example.test",
            ServerEndpoint.parse("https://photos.example.test/", false).baseUrl,
        )
        assertThrows(IllegalArgumentException::class.java) {
            ServerEndpoint.parse("https://photos.example.test/api", false)
        }
        assertThrows(IllegalArgumentException::class.java) {
            ServerEndpoint.parse("https://user@photos.example.test", false)
        }
    }
}
