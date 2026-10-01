package app.mirror.vault.backup.data

import android.Manifest
import android.content.ContentUris
import android.content.Context
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import app.mirror.vault.backup.LocalMedia
import app.mirror.vault.backup.LocalMediaSource
import app.mirror.vault.backup.MediaPermissionState
import java.io.FileNotFoundException
import java.io.InputStream

data class DiscoveredFolder(
    val bucketId: String,
    val displayName: String,
    val relativePath: String?,
)

/**
 * MediaStore boundary for owner-selected photo and video folders.
 *
 * C007: partial grants return only visible rows. Callers must surface partial
 * state and must not infer that hidden rows were deleted.
 *
 * [includeVideos] is consulted on every scan so the preference applies without
 * rebuilding the source; videos are only read when that permission is granted.
 */
@Suppress("TooManyFunctions") // MediaStore query and permission policy share one Android boundary.
class MediaStoreSource(
    private val context: Context,
    private val includeVideos: () -> Boolean = { true },
) : LocalMediaSource {
    private val resolver = context.contentResolver

    fun permissionState(): MediaPermissionState =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                granted(Manifest.permission.READ_MEDIA_IMAGES) -> MediaPermissionState.FULL
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE &&
                granted(Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED) ->
                MediaPermissionState.PARTIAL
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU &&
                granted(Manifest.permission.READ_EXTERNAL_STORAGE) -> MediaPermissionState.FULL
            else -> MediaPermissionState.DENIED
        }

    fun requiredPermissions(): Array<String> =
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE ->
                arrayOf(
                    Manifest.permission.READ_MEDIA_IMAGES,
                    Manifest.permission.READ_MEDIA_VIDEO,
                    Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
                )
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
                arrayOf(Manifest.permission.READ_MEDIA_IMAGES, Manifest.permission.READ_MEDIA_VIDEO)
            else -> arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        }

    suspend fun discoverFolders(): List<DiscoveredFolder> {
        val folders = linkedMapOf<String, DiscoveredFolder>()
        queryMedia(bucketId = null).forEach { media ->
            folders.putIfAbsent(
                media.bucketId,
                DiscoveredFolder(
                    bucketId = media.bucketId,
                    displayName = media.folderName,
                    relativePath = media.relativePath,
                ),
            )
        }
        return folders.values.sortedWith(
            compareBy<DiscoveredFolder> { it.displayName.lowercase() }.thenBy { it.bucketId },
        )
    }

    suspend fun scanFolder(bucketId: String): List<LocalMedia> = queryMedia(bucketId).map(MediaRow::toLocalMedia)

    override suspend fun metadata(uri: String): LocalMedia? {
        val parsed = uri.toUri()
        return resolver
            .query(parsed, projection(), null, null, null)
            ?.use { cursor ->
                if (cursor.moveToFirst()) {
                    cursor.readMediaRow(IMAGE_COLLECTION, parsed)?.toLocalMedia()
                } else {
                    null
                }
            }
    }

    override fun open(uri: String): InputStream =
        resolver.openInputStream(uri.toUri())
            ?: throw FileNotFoundException("media is unavailable")

    private fun queryMedia(bucketId: String?): List<MediaRow> =
        buildList {
            addAll(queryCollection(IMAGE_COLLECTION, IMAGE_MIME_TYPES, bucketId))
            if (includeVideos() && canReadVideos()) {
                addAll(queryCollection(VIDEO_COLLECTION, VIDEO_MIME_TYPES, bucketId))
            }
        }

    private fun queryCollection(
        collection: Uri,
        mimeTypes: List<String>,
        bucketId: String?,
    ): List<MediaRow> {
        val selectionParts =
            mutableListOf(
                "${MediaStore.MediaColumns.MIME_TYPE} IN (${mimeTypes.joinToString(",") { "?" }})",
                "${MediaStore.MediaColumns.SIZE} > 0",
                "${MediaStore.MediaColumns.IS_PENDING} = 0",
            )
        val arguments = mimeTypes.toMutableList()
        if (bucketId != null) {
            selectionParts += "${MediaStore.MediaColumns.BUCKET_ID} = ?"
            arguments += bucketId
        }

        return resolver
            .query(
                collection,
                projection(),
                selectionParts.joinToString(" AND "),
                arguments.toTypedArray(),
                "${MediaStore.MediaColumns.DATE_MODIFIED} ASC, ${MediaStore.MediaColumns._ID} ASC",
            )?.use { cursor ->
                buildList {
                    while (cursor.moveToNext()) {
                        cursor.readMediaRow(collection, null)?.let(::add)
                    }
                }
            }.orEmpty()
    }

    /** Videos need their own grant on Android 13+; older releases use one storage permission. */
    fun videoPermissionGranted(): Boolean = canReadVideos()

    private fun canReadVideos(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            granted(Manifest.permission.READ_MEDIA_VIDEO)
        } else {
            granted(Manifest.permission.READ_EXTERNAL_STORAGE)
        }

    private fun projection(): Array<String> =
        buildList {
            add(MediaStore.MediaColumns._ID)
            add(MediaStore.MediaColumns.DISPLAY_NAME)
            add(MediaStore.MediaColumns.MIME_TYPE)
            add(MediaStore.MediaColumns.SIZE)
            add(MediaStore.MediaColumns.DATE_MODIFIED)
            add(MediaStore.MediaColumns.BUCKET_ID)
            add(MediaStore.MediaColumns.BUCKET_DISPLAY_NAME)
            add(MediaStore.MediaColumns.RELATIVE_PATH)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                add(MediaStore.MediaColumns.GENERATION_MODIFIED)
            }
        }.toTypedArray()

    @Suppress("ReturnCount") // Missing provider columns make a row unusable; guard exits stay explicit.
    private fun Cursor.readMediaRow(
        baseUri: Uri,
        exactUri: Uri?,
    ): MediaRow? {
        val id = long(MediaStore.MediaColumns._ID) ?: return null
        val displayName = string(MediaStore.MediaColumns.DISPLAY_NAME) ?: return null
        val mimeType = string(MediaStore.MediaColumns.MIME_TYPE) ?: return null
        val sizeBytes = long(MediaStore.MediaColumns.SIZE)?.takeIf { it > 0 } ?: return null
        val bucketId = string(MediaStore.MediaColumns.BUCKET_ID) ?: return null
        return MediaRow(
            uri = (exactUri ?: ContentUris.withAppendedId(baseUri, id)).toString(),
            bucketId = bucketId,
            folderName =
                string(MediaStore.MediaColumns.BUCKET_DISPLAY_NAME)
                    ?.takeIf(String::isNotBlank)
                    ?: "Unknown",
            relativePath = string(MediaStore.MediaColumns.RELATIVE_PATH),
            displayName = displayName,
            mimeType = mimeType,
            sizeBytes = sizeBytes,
            modifiedAtSeconds = long(MediaStore.MediaColumns.DATE_MODIFIED) ?: 0,
            generationModified =
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    long(MediaStore.MediaColumns.GENERATION_MODIFIED) ?: 0
                } else {
                    0
                },
        )
    }

    private fun Cursor.string(column: String): String? {
        val index = getColumnIndex(column)
        return if (index >= 0 && !isNull(index)) getString(index) else null
    }

    private fun Cursor.long(column: String): Long? {
        val index = getColumnIndex(column)
        return if (index >= 0 && !isNull(index)) getLong(index) else null
    }

    private fun granted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    private data class MediaRow(
        val uri: String,
        val bucketId: String,
        val folderName: String,
        val relativePath: String?,
        val displayName: String,
        val mimeType: String,
        val sizeBytes: Long,
        val modifiedAtSeconds: Long,
        val generationModified: Long,
    ) {
        fun toLocalMedia(): LocalMedia =
            LocalMedia(
                uri = uri,
                bucketId = bucketId,
                displayName = displayName,
                mimeType = mimeType,
                sizeBytes = sizeBytes,
                modifiedAtSeconds = modifiedAtSeconds,
                generationModified = generationModified,
            )
    }

    companion object {
        private val IMAGE_COLLECTION =
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
        private val VIDEO_COLLECTION =
            MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)

        // Mirrors the server's upload allow-list; anything else would be rejected after hashing.
        private val IMAGE_MIME_TYPES =
            listOf("image/jpeg", "image/png", "image/gif", "image/webp", "image/heic", "image/heif")
        private val VIDEO_MIME_TYPES = listOf("video/mp4")
    }
}
