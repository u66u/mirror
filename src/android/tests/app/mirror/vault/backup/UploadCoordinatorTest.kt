package app.mirror.vault.backup

import app.mirror.vault.backup.support.FakeLocalMediaSource
import app.mirror.vault.backup.support.FakeUploadApi
import app.mirror.vault.backup.support.FakeUploadQueue
import app.mirror.vault.backup.support.backupMedia
import app.mirror.vault.backup.support.localMedia
import app.mirror.vault.backup.support.testCredential
import app.mirror.vault.network.UploadSession
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class UploadCoordinatorTest {
    @Test
    fun persisted_session_skips_server_committed_parts_after_restart() =
        runTest {
            val bytes = ByteArray(UploadCoordinator.PART_SIZE_BYTES.toInt() + 3) { 7 }
            val media = backupMedia(sizeBytes = bytes.size.toLong())
            val source = FakeLocalMediaSource(bytes, localMedia(bytes.size.toLong()))
            val queue = FakeUploadQueue()
            val api =
                FakeUploadApi(
                    UploadSession(
                        uploadId = "upload-1",
                        status = "open",
                        committedParts = setOf(0),
                    ),
                )

            UploadCoordinator(api, source, queue).upload(media, testCredential)

            assertEquals(listOf(1), api.uploadedParts)
            assertEquals("asset-1", queue.verifiedAsset)
        }

    @Test
    fun changed_media_cancels_stale_server_session_before_verification() =
        runTest {
            val bytes = ByteArray(32) { 7 }
            val media = backupMedia(sizeBytes = bytes.size.toLong())
            val source = FakeLocalMediaSource(bytes, localMedia(bytes.size.toLong() + 1))
            val queue = FakeUploadQueue()
            val api =
                FakeUploadApi(
                    UploadSession(
                        uploadId = "upload-1",
                        status = "open",
                        committedParts = emptySet(),
                    ),
                )

            val error =
                try {
                    UploadCoordinator(api, source, queue).upload(media, testCredential)
                    null
                } catch (failure: BackupFailure.Permanent) {
                    failure
                }

            assertTrue(error?.message?.contains("rescan") == true)
            assertEquals(listOf("upload-1"), api.cancelledUploads)
            assertEquals(null, queue.verifiedAsset)
        }

    @Test
    fun deselected_media_stops_before_network_upload() =
        runTest {
            val bytes = ByteArray(32) { 7 }
            val media = backupMedia(sizeBytes = bytes.size.toLong())
            val source = FakeLocalMediaSource(bytes, localMedia(bytes.size.toLong()))
            val queue = FakeUploadQueue().apply { eligible = false }
            val api =
                FakeUploadApi(
                    UploadSession(
                        uploadId = "upload-1",
                        status = "open",
                        committedParts = emptySet(),
                    ),
                )

            val error =
                try {
                    UploadCoordinator(api, source, queue).upload(media, testCredential)
                    null
                } catch (failure: BackupFailure.Ineligible) {
                    failure
                }

            assertTrue(error != null)
            assertTrue(api.uploadedParts.isEmpty())
            assertEquals(null, queue.verifiedAsset)
        }
}
