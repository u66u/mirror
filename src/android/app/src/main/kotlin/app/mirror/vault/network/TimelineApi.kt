package app.mirror.vault.network

import app.mirror.vault.auth.DeviceCredential

data class AssetDerivative(
    val format: String,
    val width: Int,
    val height: Int,
)

/** Where an item stands with respect to the vault. */
enum class BackupState {
    /** Stored in the vault (and possibly also on this device). */
    IN_VAULT,

    /** On this device only, queued for backup. */
    WAITING,

    /** On this device only, uploading right now. */
    UPLOADING,

    /** On this device only; the last upload attempt failed. */
    FAILED,
}

data class AssetTimelineItem(
    val assetId: String,
    val createdAt: String,
    val favoriteAt: String?,
    val originalBlake3: String,
    val mediaType: String,
    val sizeBytes: Long,
    val originalFilename: String?,
    val thumbnail: AssetDerivative?,
    val preview: AssetDerivative?,
    val trashedAt: String? = null,
    /** Content URI of a copy on this device, when there is one. */
    val localUri: String? = null,
    val backupState: BackupState = BackupState.IN_VAULT,
)

data class AssetTimelinePage(
    val items: List<AssetTimelineItem>,
    val nextCursor: String?,
)

interface TimelineApi {
    suspend fun listAssets(
        credential: DeviceCredential,
        cursor: String?,
        limit: Int,
    ): AssetTimelinePage
}
