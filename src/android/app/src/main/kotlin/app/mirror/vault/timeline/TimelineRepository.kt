package app.mirror.vault.timeline

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.AssetTimelinePage
import app.mirror.vault.network.ServerEndpoint
import app.mirror.vault.network.TimelineApi

private const val TIMELINE_PAGE_SIZE = 60

class TimelineRepository(
    private val api: TimelineApi,
) {
    suspend fun loadPage(
        credential: DeviceCredential,
        cursor: String?,
    ): AssetTimelinePage =
        api.listAssets(
            credential = credential,
            cursor = cursor,
            limit = TIMELINE_PAGE_SIZE,
        )

    fun derivativeUrl(
        credential: DeviceCredential,
        asset: AssetTimelineItem,
        kind: TimelineDerivativeKind,
    ): String {
        val endpoint =
            ServerEndpoint.parse(
                rawValue = credential.serverUrl,
                allowInsecurePrivateLan = true,
            )
        return "${endpoint.baseUrl}/assets/${asset.assetId}/derivatives/${kind.value}"
    }

    fun authorizationHeader(credential: DeviceCredential): String = "Bearer ${credential.token}"
}

enum class TimelineDerivativeKind(
    val value: String,
) {
    THUMBNAIL("thumbnail"),
    PREVIEW("preview"),
}
