package app.mirror.vault.library

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.AssetTimelinePage
import app.mirror.vault.network.FaceItem
import app.mirror.vault.network.LibraryApi
import app.mirror.vault.network.PersonSummary
import app.mirror.vault.network.SearchMode
import app.mirror.vault.network.ServerEndpoint

private const val PAGE_SIZE = 60
private const val SEARCH_LIMIT = 80
private const val FACE_LIMIT = 200
private const val DEFAULT_SHARE_LIFETIME_SECONDS = 7L * 24 * 60 * 60

@Suppress("TooManyFunctions") // Thin pass-through over LibraryApi plus URL helpers.
class LibraryRepository(
    private val api: LibraryApi,
    private val shareLifetimeSeconds: () -> Long = { DEFAULT_SHARE_LIFETIME_SECONDS },
) {
    suspend fun setFavorite(
        credential: DeviceCredential,
        assetId: String,
        favorite: Boolean,
    ) = api.setFavorite(credential, assetId, favorite)

    suspend fun trash(
        credential: DeviceCredential,
        assetId: String,
    ) = api.trash(credential, assetId)

    suspend fun restore(
        credential: DeviceCredential,
        assetId: String,
    ) = api.restore(credential, assetId)

    suspend fun purge(
        credential: DeviceCredential,
        assetId: String,
    ) = api.purge(credential, assetId)

    suspend fun trashPage(
        credential: DeviceCredential,
        cursor: String?,
    ): AssetTimelinePage = api.listTrash(credential, cursor, PAGE_SIZE)

    suspend fun search(
        credential: DeviceCredential,
        query: String,
        mode: SearchMode,
    ): List<AssetTimelineItem> = api.search(credential, query, mode, SEARCH_LIMIT)

    /**
     * Creates a private share (lifetime from settings) and returns a link that opens the
     * privacy-filtered preview image directly in any browser.
     */
    suspend fun shareLink(
        credential: DeviceCredential,
        assetId: String,
    ): String {
        val share = api.createShare(credential, assetId, shareLifetimeSeconds())
        return "${baseUrl(credential)}/shares/${share.token}/derivatives/preview"
    }

    suspend fun people(credential: DeviceCredential): List<PersonSummary> = api.listPeople(credential)

    suspend fun personFaces(
        credential: DeviceCredential,
        personId: String,
        limit: Int = FACE_LIMIT,
    ): List<FaceItem> = api.personFaces(credential, personId, limit)

    suspend fun unassignedFaces(credential: DeviceCredential) = api.unassignedFaces(credential, FACE_LIMIT)

    suspend fun renamePerson(
        credential: DeviceCredential,
        personId: String,
        displayName: String,
    ) = api.renamePerson(credential, personId, displayName)

    suspend fun hidePerson(
        credential: DeviceCredential,
        personId: String,
    ) = api.hidePerson(credential, personId)

    fun faceChipUrl(
        credential: DeviceCredential,
        faceId: String,
    ): String = "${baseUrl(credential)}/people/faces/$faceId/chip"

    fun originalUrl(
        credential: DeviceCredential,
        assetId: String,
    ): String = "${baseUrl(credential)}/assets/$assetId/original"

    fun thumbnailUrl(
        credential: DeviceCredential,
        assetId: String,
    ): String = "${baseUrl(credential)}/assets/$assetId/derivatives/thumbnail"

    /** Trashed assets are only reachable through the trash route. */
    fun trashedThumbnailUrl(
        credential: DeviceCredential,
        assetId: String,
    ): String = "${baseUrl(credential)}/trash/assets/$assetId/derivatives/thumbnail"

    private fun baseUrl(credential: DeviceCredential): String =
        ServerEndpoint
            .parse(
                rawValue = credential.serverUrl,
                allowInsecurePrivateLan = true,
            ).baseUrl
}
