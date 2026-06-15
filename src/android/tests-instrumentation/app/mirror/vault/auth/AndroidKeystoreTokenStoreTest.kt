package app.mirror.vault.auth

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class AndroidKeystoreTokenStoreTest {
    private lateinit var store: AndroidKeystoreTokenStore

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        store = AndroidKeystoreTokenStore(context)
        store.clear()
    }

    @After
    fun tearDown() {
        store.clear()
    }

    @Test
    fun encryptedCredentialRoundTripsAndClears() {
        val credential =
            DeviceCredential(
                serverUrl = "https://photos.example.test",
                deviceTokenId = "device-id",
                token = "raw-device-token",
            )

        store.write(credential)

        assertEquals(credential, store.read())
        store.clear()
        assertNull(store.read())
    }
}
