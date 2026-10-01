package app.mirror.vault.network

import app.mirror.vault.auth.DeviceCredential

enum class SearchMode(
    val value: String,
) {
    SEMANTIC("semantic"),
    FILENAME("filename"),
}

data class CreatedShare(
    val shareId: String,
    val token: String,
    val expiresAt: String,
)

data class PersonSummary(
    val personId: String,
    val displayName: String?,
    val reviewStatus: String,
    val faceCount: Long,
)

data class FaceItem(
    val faceId: String,
    val assetId: String,
    val assetCreatedAt: String,
    val mediaType: String,
    val chipAvailable: Boolean,
)

/**
 * Owner library operations beyond the timeline page.
 *
 * Every route accepts the Android device bearer token; cookie-only CSRF rules
 * do not apply to bearer credentials.
 */
@Suppress("TooManyFunctions") // Mirrors the backend library route set one-to-one.
interface LibraryApi {
    suspend fun setFavorite(
        credential: DeviceCredential,
        assetId: String,
        favorite: Boolean,
    )

    suspend fun trash(
        credential: DeviceCredential,
        assetId: String,
    )

    suspend fun restore(
        credential: DeviceCredential,
        assetId: String,
    )

    suspend fun purge(
        credential: DeviceCredential,
        assetId: String,
    )

    suspend fun listTrash(
        credential: DeviceCredential,
        cursor: String?,
        limit: Int,
    ): AssetTimelinePage

    suspend fun search(
        credential: DeviceCredential,
        query: String,
        mode: SearchMode,
        limit: Int,
    ): List<AssetTimelineItem>

    suspend fun createShare(
        credential: DeviceCredential,
        assetId: String,
        expiresInSeconds: Long?,
    ): CreatedShare

    suspend fun listPeople(credential: DeviceCredential): List<PersonSummary>

    suspend fun personFaces(
        credential: DeviceCredential,
        personId: String,
        limit: Int,
    ): List<FaceItem>

    suspend fun unassignedFaces(
        credential: DeviceCredential,
        limit: Int,
    ): List<FaceItem>

    suspend fun renamePerson(
        credential: DeviceCredential,
        personId: String,
        displayName: String,
    )

    suspend fun hidePerson(
        credential: DeviceCredential,
        personId: String,
    )
}
