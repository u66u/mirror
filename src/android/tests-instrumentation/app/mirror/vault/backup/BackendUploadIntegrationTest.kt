package app.mirror.vault.backup

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.test.platform.app.InstrumentationRegistry
import app.mirror.vault.backup.data.MediaStoreSource
import app.mirror.vault.backup.support.TEST_DISPLAY_NAME
import app.mirror.vault.backup.support.insertTestPhoto
import app.mirror.vault.network.KtorMirrorApi
import app.mirror.vault.network.ServerEndpoint
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertNotNull
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.util.UUID

class BackendUploadIntegrationTest {
    @Test
    fun real_media_store_photo_uploads_to_actix_backend() =
        runBlocking {
            val arguments = InstrumentationRegistry.getArguments()
            val serverUrl = arguments.getString(ARG_SERVER_URL)
            val password = arguments.getString(ARG_PASSWORD)
            assumeTrue(!serverUrl.isNullOrBlank() && !password.isNullOrBlank())

            val context = ApplicationProvider.getApplicationContext<Context>()
            val resolver = context.contentResolver
            val photoUri = insertTestPhoto(resolver)
            val api = KtorMirrorApi()
            val credential =
                api
                    .login(
                        endpoint = ServerEndpoint.parse(requireNotNull(serverUrl), true),
                        password = requireNotNull(password),
                        deviceName = "Mirror integration test",
                    ).let { issued ->
                        app.mirror.vault.auth.DeviceCredential(
                            serverUrl = serverUrl,
                            deviceTokenId = issued.deviceTokenId,
                            token = issued.token,
                        )
                    }

            try {
                val mediaStore = MediaStoreSource(context)
                val local = requireNotNull(mediaStore.metadata(photoUri.toString()))
                val queue = RecordingQueue()
                val media =
                    BackupMedia(
                        uri = local.uri,
                        bucketId = local.bucketId,
                        displayName = TEST_DISPLAY_NAME,
                        mimeType = local.mimeType,
                        sizeBytes = local.sizeBytes,
                        modifiedAtSeconds = local.modifiedAtSeconds,
                        generationModified = local.generationModified,
                        remoteGeneration = 1,
                        clientUploadKey = UUID.randomUUID().toString(),
                        uploadId = null,
                        blake3 = null,
                    )

                UploadCoordinator(api, mediaStore, queue).upload(media, credential)

                assertNotNull(queue.verifiedAssetId)
            } finally {
                api.revoke(credential)
                resolver.delete(photoUri, null, null)
            }
        }

    private class RecordingQueue : UploadQueue {
        var verifiedAssetId: String? = null

        override suspend fun resetInterrupted() = Unit

        override suspend fun claimNextPending(): BackupMedia? = null

        override suspend fun hasPending(): Boolean = false

        override suspend fun isEligible(media: BackupMedia): Boolean = true

        override suspend fun saveHash(
            media: BackupMedia,
            blake3: String,
        ) = Unit

        override suspend fun saveUploadSession(
            media: BackupMedia,
            uploadId: String,
        ) = Unit

        override suspend fun clearUploadSession(media: BackupMedia) = Unit

        override suspend fun markPending(
            media: BackupMedia,
            error: String?,
        ) = Unit

        override suspend fun markFailed(
            media: BackupMedia,
            error: String,
        ) = Unit

        override suspend fun markVerified(
            media: BackupMedia,
            assetId: String,
        ) {
            verifiedAssetId = assetId
        }
    }

    companion object {
        private const val ARG_SERVER_URL = "mirrorServerUrl"
        private const val ARG_PASSWORD = "mirrorPassword"
    }
}
