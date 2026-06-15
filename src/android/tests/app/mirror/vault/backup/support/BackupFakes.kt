package app.mirror.vault.backup.support

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.backup.BackupMedia
import app.mirror.vault.backup.LocalMedia
import app.mirror.vault.backup.LocalMediaSource
import app.mirror.vault.backup.UploadQueue
import app.mirror.vault.network.CompletedUpload
import app.mirror.vault.network.CreateUpload
import app.mirror.vault.network.UploadApi
import app.mirror.vault.network.UploadSession
import java.io.ByteArrayInputStream
import java.io.InputStream

class FakeLocalMediaSource(
    private val bytes: ByteArray,
    var current: LocalMedia,
) : LocalMediaSource {
    override suspend fun metadata(uri: String): LocalMedia? = current.takeIf { it.uri == uri }

    override fun open(uri: String): InputStream {
        require(uri == current.uri)
        return ByteArrayInputStream(bytes)
    }
}

class FakeUploadQueue : UploadQueue {
    var savedHash: String? = null
    var savedSession: String? = null
    var verifiedAsset: String? = null
    var clearedSessions = 0
    var eligible = true

    override suspend fun resetInterrupted() = Unit

    override suspend fun claimNextPending(): BackupMedia? = null

    override suspend fun hasPending(): Boolean = false

    override suspend fun isEligible(media: BackupMedia): Boolean = eligible

    override suspend fun saveHash(
        media: BackupMedia,
        blake3: String,
    ) {
        savedHash = blake3
    }

    override suspend fun saveUploadSession(
        media: BackupMedia,
        uploadId: String,
    ) {
        savedSession = uploadId
    }

    override suspend fun clearUploadSession(media: BackupMedia) {
        clearedSessions += 1
    }

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
        verifiedAsset = assetId
    }
}

class FakeUploadApi(
    var session: UploadSession,
) : UploadApi {
    val uploadedParts = mutableListOf<Int>()
    val cancelledUploads = mutableListOf<String>()
    var createKey: String? = null

    override suspend fun createUpload(
        credential: DeviceCredential,
        upload: CreateUpload,
    ): UploadSession {
        createKey = upload.clientUploadKey
        return session
    }

    override suspend fun getUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): UploadSession = session

    override suspend fun putUploadPart(
        credential: DeviceCredential,
        uploadId: String,
        partIndex: Int,
        bytes: ByteArray,
    ) {
        uploadedParts += partIndex
    }

    override suspend fun completeUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): CompletedUpload = CompletedUpload(assetId = "asset-1")

    override suspend fun cancelUpload(
        credential: DeviceCredential,
        uploadId: String,
    ) {
        cancelledUploads += uploadId
    }
}

fun backupMedia(
    sizeBytes: Long,
    uploadId: String? = "upload-1",
): BackupMedia =
    BackupMedia(
        uri = "content://media/photo/1",
        bucketId = "camera",
        displayName = "photo.jpg",
        mimeType = "image/jpeg",
        sizeBytes = sizeBytes,
        modifiedAtSeconds = 10,
        generationModified = 20,
        remoteGeneration = 1,
        clientUploadKey = "client-key-1",
        uploadId = uploadId,
        blake3 = "known-hash",
    )

fun localMedia(sizeBytes: Long): LocalMedia =
    LocalMedia(
        uri = "content://media/photo/1",
        bucketId = "camera",
        displayName = "photo.jpg",
        mimeType = "image/jpeg",
        sizeBytes = sizeBytes,
        modifiedAtSeconds = 10,
        generationModified = 20,
    )

val testCredential =
    DeviceCredential(
        serverUrl = "https://mirror.example",
        deviceTokenId = "device-1",
        token = "secret",
    )
