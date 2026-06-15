package app.mirror.vault.network

import app.mirror.vault.auth.DeviceCredential

data class UploadSession(
    val uploadId: String,
    val status: String,
    val committedParts: Set<Int>,
)

data class CompletedUpload(
    val assetId: String,
)

data class CreateUpload(
    val filename: String,
    val sizeBytes: Long,
    val blake3: String,
    val mimeType: String,
    val clientUploadKey: String,
)

interface UploadApi {
    suspend fun createUpload(
        credential: DeviceCredential,
        upload: CreateUpload,
    ): UploadSession

    suspend fun getUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): UploadSession

    suspend fun putUploadPart(
        credential: DeviceCredential,
        uploadId: String,
        partIndex: Int,
        bytes: ByteArray,
    )

    suspend fun completeUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): CompletedUpload

    suspend fun cancelUpload(
        credential: DeviceCredential,
        uploadId: String,
    )
}
