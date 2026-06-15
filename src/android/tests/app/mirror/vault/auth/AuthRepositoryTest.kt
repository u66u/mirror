package app.mirror.vault.auth

import app.mirror.vault.network.IssuedDeviceToken
import app.mirror.vault.network.MirrorApi
import app.mirror.vault.network.MirrorTransportException
import app.mirror.vault.network.ServerEndpoint
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.io.IOException

class AuthRepositoryTest {
    @Test
    fun successfulLoginPersistsServerIssuedCredential() =
        runTest {
            val store = MemoryTokenStore()
            val repository =
                AuthRepository(
                    api = FakeMirrorApi(IssuedDeviceToken("device-id", "raw-token")),
                    tokenStore = store,
                )

            val credential =
                repository.login(
                    rawServerUrl = "https://photos.example.test/",
                    allowInsecurePrivateLan = false,
                    password = "password",
                    deviceName = "Pixel",
                )

            assertEquals(
                DeviceCredential(
                    serverUrl = "https://photos.example.test",
                    deviceTokenId = "device-id",
                    token = "raw-token",
                ),
                credential,
            )
            assertEquals(credential, store.credential)
        }

    @Test
    fun failedLoginDoesNotDestroyExistingCredential() {
        val existing = DeviceCredential("https://old.example.test", "old-id", "old-token")
        val store = MemoryTokenStore(existing)
        val repository =
            AuthRepository(
                api = FakeMirrorApi(failure = IOException("offline")),
                tokenStore = store,
            )

        assertThrows(IOException::class.java) {
            runTest {
                repository.login(
                    rawServerUrl = "https://new.example.test",
                    allowInsecurePrivateLan = false,
                    password = "password",
                    deviceName = "Pixel",
                )
            }
        }
        assertEquals(existing, store.credential)
    }

    @Test
    fun failedRemoteRevokeStillClearsLocalCredential() =
        runTest {
            val existing = DeviceCredential("https://old.example.test", "old-id", "old-token")
            val store = MemoryTokenStore(existing)
            val repository =
                AuthRepository(
                    api =
                        FakeMirrorApi(
                            revokeFailure = MirrorTransportException(IOException("offline")),
                        ),
                    tokenStore = store,
                )

            val result = repository.logout(existing)

            assertEquals(LogoutResult.LOCAL_ONLY, result)
            assertEquals(null, store.credential)
        }
}

private class MemoryTokenStore(
    var credential: DeviceCredential? = null,
) : TokenStore {
    override fun read(): DeviceCredential? = credential

    override fun write(credential: DeviceCredential) {
        this.credential = credential
    }

    override fun clear() {
        credential = null
    }
}

private class FakeMirrorApi(
    private val issued: IssuedDeviceToken? = null,
    private val failure: Exception? = null,
    private val revokeFailure: Exception? = null,
) : MirrorApi {
    override suspend fun login(
        endpoint: ServerEndpoint,
        password: String,
        deviceName: String,
    ): IssuedDeviceToken {
        failure?.let { throw it }
        return checkNotNull(issued)
    }

    override suspend fun revoke(credential: DeviceCredential) {
        revokeFailure?.let { throw it }
    }
}
