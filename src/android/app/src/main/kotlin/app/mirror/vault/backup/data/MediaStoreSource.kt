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
 * MediaStore boundary for owner-selected image folders.
 *
 * C007: partial grants return only visible rows. Callers must surface partial
 * state and must not infer that hidden rows were deleted.
 */
@Suppress("TooManyFunctions") // MediaStore query and permission policy share one Android boundary.
class MediaStoreSource(
    private val context: Context,
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
                    Manifest.permission.READ_MEDIA_VISUAL_USER_SELECTED,
                )
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
                arrayOf(Manifest.permission.READ_MEDIA_IMAGES)
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
                    cursor.readMediaRow(COLLECTION, parsed)?.toLocalMedia()
                } else {
                    null
                }
            }
    }

    override fun open(uri: String): InputStream =
        resolver.openInputStream(uri.toUri())
            ?: throw FileNotFoundException("media is unavailable")

    private fun queryMedia(bucketId: String?): List<MediaRow> {
        val selectionParts =
            mutableListOf(
                "${MediaStore.Images.Media.MIME_TYPE} IN (?,?,?,?)",
                "${MediaStore.Images.Media.SIZE} > 0",
                "${MediaStore.Images.Media.IS_PENDING} = 0",
            )
        val arguments = SUPPORTED_MIME_TYPES.toMutableList()
        if (bucketId != null) {
            selectionParts += "${MediaStore.Images.Media.BUCKET_ID} = ?"
            arguments += bucketId
        }

        return resolver
            .query(
                COLLECTION,
                projection(),
                selectionParts.joinToString(" AND "),
                arguments.toTypedArray(),
                "${MediaStore.Images.Media.DATE_MODIFIED} ASC, ${MediaStore.Images.Media._ID} ASC",
            )?.use { cursor ->
                buildList {
                    while (cursor.moveToNext()) {
                        cursor.readMediaRow(COLLECTION, null)?.let(::add)
                    }
                }
            }.orEmpty()
    }

    private fun projection(): Array<String> =
        buildList {
            add(MediaStore.Images.Media._ID)
            add(MediaStore.Images.Media.DISPLAY_NAME)
            add(MediaStore.Images.Media.MIME_TYPE)
            add(MediaStore.Images.Media.SIZE)
            add(MediaStore.Images.Media.DATE_MODIFIED)
            add(MediaStore.Images.Media.BUCKET_ID)
            add(MediaStore.Images.Media.BUCKET_DISPLAY_NAME)
            add(MediaStore.Images.Media.RELATIVE_PATH)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                add(MediaStore.Images.Media.GENERATION_MODIFIED)
            }
        }.toTypedArray()

    @Suppress("ReturnCount") // Missing provider columns make a row unusable; guard exits stay explicit.
    private fun Cursor.readMediaRow(
        baseUri: Uri,
        exactUri: Uri?,
    ): MediaRow? {
        val id = long(MediaStore.Images.Media._ID) ?: return null
        val displayName = string(MediaStore.Images.Media.DISPLAY_NAME) ?: return null
        val mimeType = string(MediaStore.Images.Media.MIME_TYPE) ?: return null
        val sizeBytes = long(MediaStore.Images.Media.SIZE)?.takeIf { it > 0 } ?: return null
        val bucketId = string(MediaStore.Images.Media.BUCKET_ID) ?: return null
        return MediaRow(
            uri = (exactUri ?: ContentUris.withAppendedId(baseUri, id)).toString(),
            bucketId = bucketId,
            folderName =
                string(MediaStore.Images.Media.BUCKET_DISPLAY_NAME)
                    ?.takeIf(String::isNotBlank)
                    ?: "Unknown",
            relativePath = string(MediaStore.Images.Media.RELATIVE_PATH),
            displayName = displayName,
            mimeType = mimeType,
            sizeBytes = sizeBytes,
            modifiedAtSeconds = long(MediaStore.Images.Media.DATE_MODIFIED) ?: 0,
            generationModified =
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    long(MediaStore.Images.Media.GENERATION_MODIFIED) ?: 0
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
        private val COLLECTION =
            MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
        private val SUPPORTED_MIME_TYPES =
            listOf("image/jpeg", "image/png", "image/gif", "image/webp")
    }
}
