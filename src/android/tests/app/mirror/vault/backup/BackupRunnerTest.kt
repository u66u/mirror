package app.mirror.vault.backup

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.auth.TokenStore
import app.mirror.vault.backup.support.backupMedia
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

class BackupRunnerTest {
    @Test
    fun run_processes_bounded_batch_and_reports_more_work() =
        runTest {
            val pending =
                ArrayDeque(
                    (1..5).map { index ->
                        backupMedia(sizeBytes = index.toLong()).copy(uri = "content://media/$index")
                    },
                )
            val queue = BatchQueue(pending)
            val uploaded = mutableListOf<String>()
            val scanner = GrantedScanService()
            val runner =
                DefaultBackupRunner(
                    repository = scanner,
                    tokenStore = FixedTokenStore,
                    queue = queue,
                    coordinator = MediaUploader { media, _ -> uploaded += media.uri },
                )

            val result = runner.run()

            assertEquals(BackupRunResult.MORE_WORK, result)
            assertEquals(4, uploaded.size)
            assertEquals(1, pending.size)
            assertEquals("https://mirror.example", scanner.activatedServer)
        }
}

private class GrantedScanService : BackupScanService {
    var activatedServer: String? = null

    override suspend fun activateRemote(credential: DeviceCredential) {
        activatedServer = credential.serverUrl
    }

    override fun permissionState(): MediaPermissionState = MediaPermissionState.FULL

    override suspend fun scanSelectedFolders() = Unit
}

private object FixedTokenStore : TokenStore {
    private val credential =
        DeviceCredential(
            serverUrl = "https://mirror.example",
            deviceTokenId = "device-1",
            token = "secret",
        )

    override fun read(): DeviceCredential = credential

    override fun write(credential: DeviceCredential) = Unit

    override fun clear() = Unit
}

private class BatchQueue(
    private val pending: ArrayDeque<BackupMedia>,
) : UploadQueue {
    override suspend fun resetInterrupted() = Unit

    override suspend fun claimNextPending(): BackupMedia? = pending.removeFirstOrNull()

    override suspend fun hasPending(): Boolean = pending.isNotEmpty()

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
    ) = Unit
}
