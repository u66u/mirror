package app.mirror.vault.backup.data

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import app.mirror.vault.backup.support.TEST_DISPLAY_NAME
import app.mirror.vault.backup.support.TEST_JPEG_BYTES
import app.mirror.vault.backup.support.TEST_RELATIVE_PATH
import app.mirror.vault.backup.support.insertTestPhoto
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

class MediaStoreSourceTest {
    @Test
    fun scanner_discovers_and_reads_real_media_store_folder() =
        runBlocking {
            val context = ApplicationProvider.getApplicationContext<Context>()
            val resolver = context.contentResolver
            val uri = insertTestPhoto(resolver)

            try {
                val scanner = MediaStoreSource(context)
                val folder =
                    scanner
                        .discoverFolders()
                        .firstOrNull { it.relativePath == TEST_RELATIVE_PATH }
                assertNotNull(folder)

                val media = scanner.scanFolder(requireNotNull(folder).bucketId)
                val inserted = media.single { it.displayName == TEST_DISPLAY_NAME }
                assertEquals(TEST_DISPLAY_NAME, inserted.displayName)
                assertEquals("image/jpeg", inserted.mimeType)
                assertEquals(TEST_JPEG_BYTES.size.toLong(), inserted.sizeBytes)
                assertEquals(
                    TEST_JPEG_BYTES.toList(),
                    scanner.open(inserted.uri).use { it.readBytes() }.toList(),
                )
            } finally {
                resolver.delete(uri, null, null)
            }
        }
}
