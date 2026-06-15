package app.mirror.vault.backup.data

import android.content.Context
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import app.mirror.vault.backup.LocalMedia
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test

class BackupDaoTest {
    private lateinit var database: MirrorDatabase
    private lateinit var dao: BackupDao

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        database =
            Room
                .inMemoryDatabaseBuilder(context, MirrorDatabase::class.java)
                .allowMainThreadQueries()
                .build()
        dao = database.backupDao()
    }

    @After
    fun tearDown() {
        database.close()
    }

    @Test
    fun changed_media_fingerprint_discards_verified_upload_progress() =
        runBlocking {
            val generation = prepareRemote()
            val original = media(sizeBytes = 100)
            dao.reconcileFolder(original.bucketId, listOf(original), generation)
            val first = requireNotNull(dao.claimNextPending())
            dao.saveHash(first.uri, first.remoteGeneration, "old-hash")
            dao.saveUploadSession(first.uri, first.remoteGeneration, "old-upload")
            dao.markVerified(first.uri, first.remoteGeneration, "asset-1")

            dao.reconcileFolder(
                original.bucketId,
                listOf(original.copy(sizeBytes = 101, generationModified = 2)),
                generation,
            )
            val changed = requireNotNull(dao.claimNextPending())

            assertNull(changed.uploadId)
            assertNull(changed.blake3)
            assertNotEquals(first.clientUploadKey, changed.clientUploadKey)
            assertEquals(101, changed.sizeBytes)
        }

    @Test
    fun deselected_folder_cannot_be_claimed_or_counted() =
        runBlocking {
            val generation = prepareRemote()
            val original = media(sizeBytes = 100)
            dao.reconcileFolder(original.bucketId, listOf(original), generation)

            dao.setFolderSelected(original.bucketId, false)

            assertNull(dao.claimNextPending())
            assertEquals(0, dao.observeCounts().first().pending)
        }

    @Test
    fun stale_worker_cannot_update_media_after_remote_switch() =
        runBlocking {
            val original = media(sizeBytes = 100)
            val firstGeneration = prepareRemote("https://first.example")
            dao.reconcileFolder(original.bucketId, listOf(original), firstGeneration)
            val stale = requireNotNull(dao.claimNextPending())

            val secondGeneration = dao.activateRemote("https://second.example")
            dao.reconcileFolder(original.bucketId, listOf(original), secondGeneration)
            dao.markVerified(stale.uri, stale.remoteGeneration, "wrong-asset")
            val current = requireNotNull(dao.claimNextPending())

            assertNotEquals(stale.remoteGeneration, current.remoteGeneration)
            assertNull(current.uploadId)
            assertEquals(1, dao.observeCounts().first().uploading)
        }

    @Test
    fun unscoped_media_cannot_be_claimed_before_remote_activation() =
        runBlocking {
            dao.insertDefaultSettings(BackupSettingsEntity())
            dao.upsertFolders(
                listOf(
                    BackupFolderEntity(
                        bucketId = "camera",
                        displayName = "Camera",
                        relativePath = "DCIM/Camera",
                        selected = true,
                        available = true,
                    ),
                ),
            )
            val original = media(sizeBytes = 100)
            dao.reconcileFolder(original.bucketId, listOf(original), remoteGeneration = 0)

            assertNull(dao.claimNextPending())
            assertEquals(0, dao.observeCounts().first().pending)
        }

    private suspend fun prepareRemote(remoteScope: String = "https://mirror.example"): Long {
        dao.insertDefaultSettings(BackupSettingsEntity())
        dao.upsertFolders(
            listOf(
                BackupFolderEntity(
                    bucketId = "camera",
                    displayName = "Camera",
                    relativePath = "DCIM/Camera",
                    selected = true,
                    available = true,
                ),
            ),
        )
        return dao.activateRemote(remoteScope)
    }

    private fun media(sizeBytes: Long): LocalMedia =
        LocalMedia(
            uri = "content://media/photo/1",
            bucketId = "camera",
            displayName = "photo.jpg",
            mimeType = "image/jpeg",
            sizeBytes = sizeBytes,
            modifiedAtSeconds = 1,
            generationModified = 1,
        )
}
