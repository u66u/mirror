package app.mirror.vault.backup

import java.io.InputStream

enum class MediaPermissionState {
    FULL,
    PARTIAL,
    DENIED,
}

data class BackupFolder(
    val bucketId: String,
    val displayName: String,
    val relativePath: String?,
    val selected: Boolean,
    val available: Boolean,
)

data class LocalMedia(
    val uri: String,
    val bucketId: String,
    val displayName: String,
    val mimeType: String,
    val sizeBytes: Long,
    val modifiedAtSeconds: Long,
    val generationModified: Long,
)

data class BackupMedia(
    val uri: String,
    val bucketId: String,
    val displayName: String,
    val mimeType: String,
    val sizeBytes: Long,
    val modifiedAtSeconds: Long,
    val generationModified: Long,
    val remoteGeneration: Long,
    val clientUploadKey: String,
    val uploadId: String?,
    val blake3: String?,
)

data class BackupCounts(
    val pending: Long = 0,
    val uploading: Long = 0,
    val verified: Long = 0,
    val failed: Long = 0,
)

interface LocalMediaSource {
    suspend fun metadata(uri: String): LocalMedia?

    fun open(uri: String): InputStream
}

interface BackupScanService {
    suspend fun activateRemote(credential: app.mirror.vault.auth.DeviceCredential)

    fun permissionState(): MediaPermissionState

    suspend fun scanSelectedFolders()
}

fun interface MediaUploader {
    suspend fun upload(
        media: BackupMedia,
        credential: app.mirror.vault.auth.DeviceCredential,
    )
}

interface UploadQueue {
    suspend fun resetInterrupted()

    suspend fun claimNextPending(): BackupMedia?

    suspend fun hasPending(): Boolean

    suspend fun isEligible(media: BackupMedia): Boolean

    suspend fun saveHash(
        media: BackupMedia,
        blake3: String,
    )

    suspend fun saveUploadSession(
        media: BackupMedia,
        uploadId: String,
    )

    suspend fun clearUploadSession(media: BackupMedia)

    suspend fun markPending(
        media: BackupMedia,
        error: String?,
    )

    suspend fun markFailed(
        media: BackupMedia,
        error: String,
    )

    suspend fun markVerified(
        media: BackupMedia,
        assetId: String,
    )
}
