package app.mirror.vault.network

import app.mirror.vault.auth.DeviceCredential

data class AssetDerivative(
    val format: String,
    val width: Int,
    val height: Int,
)

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
