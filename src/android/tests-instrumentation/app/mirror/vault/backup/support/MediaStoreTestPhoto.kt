package app.mirror.vault.backup.support

import android.content.ContentResolver
import android.content.ContentValues
import android.net.Uri
import android.provider.MediaStore

const val TEST_RELATIVE_PATH = "Pictures/MirrorTest/"
const val TEST_DISPLAY_NAME = "mirror-test.jpg"
val TEST_JPEG_BYTES =
    byteArrayOf(
        0xff.toByte(),
        0xd8.toByte(),
        0xff.toByte(),
        0xd9.toByte(),
    )

fun insertTestPhoto(resolver: ContentResolver): Uri {
    val collection =
        MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
    val values =
        ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, TEST_DISPLAY_NAME)
            put(MediaStore.Images.Media.MIME_TYPE, "image/jpeg")
            put(MediaStore.Images.Media.RELATIVE_PATH, TEST_RELATIVE_PATH)
            put(MediaStore.Images.Media.IS_PENDING, 1)
        }
    val uri = requireNotNull(resolver.insert(collection, values))
    resolver.openOutputStream(uri)?.use { it.write(TEST_JPEG_BYTES) }
    values.clear()
    values.put(MediaStore.Images.Media.IS_PENDING, 0)
    resolver.update(uri, values, null, null)
    return uri
}
