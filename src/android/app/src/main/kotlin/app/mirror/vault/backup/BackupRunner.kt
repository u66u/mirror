package app.mirror.vault.backup

import app.mirror.vault.auth.TokenStore
import kotlinx.coroutines.CancellationException

enum class BackupRunResult {
    COMPLETE,
    MORE_WORK,
    RETRY,
    AUTHENTICATION_REQUIRED,
    PERMISSION_REQUIRED,
}

fun interface BackupRunner {
    suspend fun run(): BackupRunResult
}

class DefaultBackupRunner(
    private val repository: BackupScanService,
    private val tokenStore: TokenStore,
    private val queue: UploadQueue,
    private val coordinator: MediaUploader,
) : BackupRunner {
    @Suppress("ReturnCount") // Each exit maps a distinct durable WorkManager outcome.
    override suspend fun run(): BackupRunResult {
        val credential = tokenStore.read() ?: return BackupRunResult.AUTHENTICATION_REQUIRED
        if (repository.permissionState() == MediaPermissionState.DENIED) {
            return BackupRunResult.PERMISSION_REQUIRED
        }

        try {
            repository.activateRemote(credential)
            repository.scanSelectedFolders()
            queue.resetInterrupted()
            repeat(MAX_ITEMS_PER_RUN) {
                val media = queue.claimNextPending() ?: return BackupRunResult.COMPLETE
                try {
                    coordinator.upload(media, credential)
                } catch (error: BackupFailure.Ineligible) {
                    queue.markPending(media, error.message)
                } catch (error: BackupFailure.AuthenticationRequired) {
                    queue.markPending(media, error.message)
                    return BackupRunResult.AUTHENTICATION_REQUIRED
                } catch (error: BackupFailure.Retryable) {
                    queue.markPending(media, error.message)
                    return BackupRunResult.RETRY
                } catch (error: BackupFailure.Permanent) {
                    queue.markFailed(media, error.message ?: "upload failed")
                }
            }
            return if (queue.hasPending()) {
                BackupRunResult.MORE_WORK
            } else {
                BackupRunResult.COMPLETE
            }
        } catch (error: CancellationException) {
            throw error
        } catch (_: SecurityException) {
            return BackupRunResult.PERMISSION_REQUIRED
        } catch (_: Exception) {
            return BackupRunResult.RETRY
        }
    }

    companion object {
        const val MAX_ITEMS_PER_RUN = 4
    }
}
