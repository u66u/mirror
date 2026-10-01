package app.mirror.vault.backup.data

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Transaction
import androidx.room.Upsert
import app.mirror.vault.backup.BackupMedia
import app.mirror.vault.backup.LocalMedia
import app.mirror.vault.backup.UploadQueue
import kotlinx.coroutines.flow.Flow
import java.util.UUID

@Dao
@Suppress("TooManyFunctions") // One Room boundary keeps related SQL transactions atomic and visible.
abstract class BackupDao {
    @Query(
        """
        SELECT *
        FROM backup_folders
        ORDER BY selected DESC, lower(displayName), bucketId
        """,
    )
    abstract fun observeFolders(): Flow<List<BackupFolderEntity>>

    @Query("SELECT * FROM backup_folders")
    abstract suspend fun folders(): List<BackupFolderEntity>

    @Query(
        """
        SELECT *
        FROM backup_folders
        WHERE selected = 1
          AND available = 1
        ORDER BY bucketId
        """,
    )
    abstract suspend fun selectedAvailableFolders(): List<BackupFolderEntity>

    @Query("UPDATE backup_folders SET available = 0")
    abstract suspend fun markFoldersUnavailable()

    @Upsert
    abstract suspend fun upsertFolders(folders: List<BackupFolderEntity>)

    @Query(
        """
        UPDATE backup_folders
        SET selected = :selected
        WHERE bucketId = :bucketId
        """,
    )
    abstract suspend fun setFolderSelected(
        bucketId: String,
        selected: Boolean,
    )

    @Query(
        """
        SELECT *
        FROM backup_media
        WHERE bucketId = :bucketId
          AND remoteGeneration = :remoteGeneration
        """,
    )
    protected abstract suspend fun mediaInFolder(
        bucketId: String,
        remoteGeneration: Long,
    ): List<BackupMediaEntity>

    @Query(
        """
        UPDATE backup_media
        SET present = 0
        WHERE bucketId = :bucketId
          AND remoteGeneration = :remoteGeneration
        """,
    )
    protected abstract suspend fun markFolderMediaMissing(
        bucketId: String,
        remoteGeneration: Long,
    )

    @Upsert
    protected abstract suspend fun upsertMedia(media: List<BackupMediaEntity>)

    /**
     * Reconciles one MediaStore folder without losing durable upload progress.
     *
     * C007: inaccessible media is marked absent, not deleted. Partial Android
     * grants can hide previously known media and later reveal it again.
     */
    @Transaction
    open suspend fun reconcileFolder(
        bucketId: String,
        scanned: List<LocalMedia>,
        remoteGeneration: Long,
    ) {
        val existing =
            mediaInFolder(bucketId, remoteGeneration).associateBy(BackupMediaEntity::uri)
        val reconciled =
            scanned.map { media ->
                val previous = existing[media.uri]
                if (previous != null && previous.sameFingerprint(media)) {
                    previous.copy(
                        displayName = media.displayName,
                        present = true,
                    )
                } else {
                    BackupMediaEntity(
                        uri = media.uri,
                        bucketId = media.bucketId,
                        displayName = media.displayName,
                        mimeType = media.mimeType,
                        sizeBytes = media.sizeBytes,
                        modifiedAtSeconds = media.modifiedAtSeconds,
                        generationModified = media.generationModified,
                        remoteGeneration = remoteGeneration,
                        present = true,
                        state = STATE_PENDING,
                        clientUploadKey = UUID.randomUUID().toString(),
                        uploadId = null,
                        blake3 = null,
                        assetId = null,
                        lastError = null,
                    )
                }
            }

        markFolderMediaMissing(bucketId, remoteGeneration)
        if (reconciled.isNotEmpty()) {
            upsertMedia(reconciled)
        }
    }

    @Query(
        """
        SELECT media.*
        FROM backup_media AS media
        JOIN backup_folders AS folder
          ON folder.bucketId = media.bucketId
        JOIN backup_settings AS settings
          ON settings.id = 1
         AND settings.remoteGeneration = media.remoteGeneration
         AND settings.remoteScope IS NOT NULL
        WHERE media.present = 1
          AND media.state = 'pending'
          AND folder.selected = 1
          AND folder.available = 1
        ORDER BY media.modifiedAtSeconds, media.uri
        LIMIT 1
        """,
    )
    protected abstract suspend fun nextPendingEntity(): BackupMediaEntity?

    @Query(
        """
        UPDATE backup_media
        SET state = 'uploading',
            lastError = NULL
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
          AND present = 1
          AND state = 'pending'
          AND EXISTS (
              SELECT 1
              FROM backup_folders AS folder
              WHERE folder.bucketId = backup_media.bucketId
                AND folder.selected = 1
                AND folder.available = 1
          )
          AND remoteGeneration = (
              SELECT settings.remoteGeneration
              FROM backup_settings AS settings
              WHERE settings.id = 1
          )
        """,
    )
    protected abstract suspend fun claimPending(
        uri: String,
        remoteGeneration: Long,
    ): Int

    @Transaction
    open suspend fun claimNextPending(): BackupMedia? {
        val candidate = nextPendingEntity() ?: return null
        return if (claimPending(candidate.uri, candidate.remoteGeneration) == 1) {
            candidate.toModel()
        } else {
            null
        }
    }

    @Query(
        """
        SELECT EXISTS(
            SELECT 1
            FROM backup_media AS media
            JOIN backup_folders AS folder
              ON folder.bucketId = media.bucketId
            JOIN backup_settings AS settings
              ON settings.id = 1
             AND settings.remoteGeneration = media.remoteGeneration
             AND settings.remoteScope IS NOT NULL
            WHERE media.present = 1
              AND media.state = 'pending'
              AND folder.selected = 1
              AND folder.available = 1
        )
        """,
    )
    abstract suspend fun hasPending(): Boolean

    @Query(
        """
        SELECT EXISTS(
            SELECT 1
            FROM backup_media AS media
            JOIN backup_folders AS folder
              ON folder.bucketId = media.bucketId
            JOIN backup_settings AS settings
              ON settings.id = 1
             AND settings.remoteGeneration = media.remoteGeneration
             AND settings.remoteScope IS NOT NULL
            WHERE media.uri = :uri
              AND media.remoteGeneration = :remoteGeneration
              AND media.present = 1
              AND folder.selected = 1
              AND folder.available = 1
        )
        """,
    )
    abstract suspend fun isEligible(
        uri: String,
        remoteGeneration: Long,
    ): Boolean

    @Query(
        """
        UPDATE backup_media
        SET state = 'pending',
            lastError = 'interrupted'
        WHERE state = 'uploading'
          AND remoteGeneration = (
              SELECT settings.remoteGeneration
              FROM backup_settings AS settings
              WHERE settings.id = 1
          )
        """,
    )
    abstract suspend fun resetInterrupted()

    @Query(
        """
        UPDATE backup_media
        SET blake3 = :blake3
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
        """,
    )
    abstract suspend fun saveHash(
        uri: String,
        remoteGeneration: Long,
        blake3: String,
    )

    @Query(
        """
        UPDATE backup_media
        SET uploadId = :uploadId
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
        """,
    )
    abstract suspend fun saveUploadSession(
        uri: String,
        remoteGeneration: Long,
        uploadId: String,
    )

    @Query(
        """
        UPDATE backup_media
        SET uploadId = NULL
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
        """,
    )
    abstract suspend fun clearUploadSession(
        uri: String,
        remoteGeneration: Long,
    )

    @Query(
        """
        UPDATE backup_media
        SET state = 'pending',
            lastError = :error
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
        """,
    )
    abstract suspend fun markPending(
        uri: String,
        remoteGeneration: Long,
        error: String?,
    )

    @Query(
        """
        UPDATE backup_media
        SET state = 'failed',
            lastError = :error
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
        """,
    )
    abstract suspend fun markFailed(
        uri: String,
        remoteGeneration: Long,
        error: String,
    )

    @Query(
        """
        UPDATE backup_media
        SET state = 'verified',
            assetId = :assetId,
            lastError = NULL
        WHERE uri = :uri
          AND remoteGeneration = :remoteGeneration
          AND EXISTS (
              SELECT 1
              FROM backup_folders AS folder
              WHERE folder.bucketId = backup_media.bucketId
                AND folder.selected = 1
                AND folder.available = 1
          )
        """,
    )
    abstract suspend fun markVerified(
        uri: String,
        remoteGeneration: Long,
        assetId: String,
    )

    @Query(
        """
        SELECT
            COALESCE(SUM(CASE WHEN state = 'pending' THEN 1 ELSE 0 END), 0) AS pending,
            COALESCE(SUM(CASE WHEN state = 'uploading' THEN 1 ELSE 0 END), 0) AS uploading,
            COALESCE(SUM(CASE WHEN state = 'verified' THEN 1 ELSE 0 END), 0) AS verified,
            COALESCE(SUM(CASE WHEN state = 'failed' THEN 1 ELSE 0 END), 0) AS failed
        FROM backup_media AS media
        JOIN backup_folders AS folder
          ON folder.bucketId = media.bucketId
        JOIN backup_settings AS settings
          ON settings.id = 1
         AND settings.remoteGeneration = media.remoteGeneration
         AND settings.remoteScope IS NOT NULL
        WHERE media.present = 1
          AND folder.selected = 1
          AND folder.available = 1
        """,
    )
    abstract fun observeCounts(): Flow<BackupCountsRow>

    @Insert(onConflict = OnConflictStrategy.IGNORE)
    abstract suspend fun insertDefaultSettings(settings: BackupSettingsEntity)

    @Query(
        """
        SELECT COALESCE(
            (SELECT wifiOnly FROM backup_settings WHERE id = 1),
            1
        )
        """,
    )
    abstract fun observeWifiOnly(): Flow<Boolean>

    @Query("SELECT wifiOnly FROM backup_settings WHERE id = 1")
    abstract suspend fun wifiOnly(): Boolean

    @Query("UPDATE backup_settings SET wifiOnly = :wifiOnly WHERE id = 1")
    abstract suspend fun setWifiOnly(wifiOnly: Boolean)

    @Query("SELECT folderSelectionInitialized FROM backup_settings WHERE id = 1")
    abstract suspend fun folderSelectionInitialized(): Boolean

    @Query("UPDATE backup_settings SET folderSelectionInitialized = 1 WHERE id = 1")
    abstract suspend fun markFolderSelectionInitialized()

    @Query("SELECT remoteScope, remoteGeneration FROM backup_settings WHERE id = 1")
    protected abstract suspend fun remoteState(): RemoteStateRow

    @Query(
        """
        UPDATE backup_settings
        SET remoteScope = :remoteScope,
            remoteGeneration = remoteGeneration + 1
        WHERE id = 1
        """,
    )
    protected abstract suspend fun replaceRemoteScope(remoteScope: String)

    @Query("DELETE FROM backup_media")
    protected abstract suspend fun clearMedia()

    /**
     * Activates one remote vault and invalidates all unscoped prior progress.
     *
     * Old workers retain the previous generation, so their later writes cannot
     * mutate rows discovered for the new server.
     */
    @Transaction
    open suspend fun activateRemote(remoteScope: String): Long {
        val current = remoteState()
        if (current.remoteScope != remoteScope) {
            replaceRemoteScope(remoteScope)
            clearMedia()
        }
        return remoteState().remoteGeneration
    }

    @Query("SELECT remoteGeneration FROM backup_settings WHERE id = 1")
    abstract suspend fun remoteGeneration(): Long

    /**
     * Device media in the folders chosen for backup, newest first. Read-only
     * view over the upload queue so the library can show local photos instantly
     * and offline, tagged with their backup state.
     */
    @Query(
        """
        SELECT media.uri, media.displayName, media.mimeType, media.sizeBytes,
               media.modifiedAtSeconds, media.state, media.assetId
        FROM backup_media AS media
        JOIN backup_folders AS folder
          ON folder.bucketId = media.bucketId
        JOIN backup_settings AS settings
          ON settings.id = 1
         AND settings.remoteGeneration = media.remoteGeneration
        WHERE media.present = 1
          AND folder.selected = 1
          AND folder.available = 1
        ORDER BY media.modifiedAtSeconds DESC, media.uri DESC
        """,
    )
    abstract fun observeLocalLibrary(): Flow<List<LocalLibraryRow>>

    @Query(
        """
        UPDATE backup_media
        SET state = 'pending',
            lastError = NULL
        WHERE state = 'failed'
          AND present = 1
          AND remoteGeneration = (
              SELECT settings.remoteGeneration
              FROM backup_settings AS settings
              WHERE settings.id = 1
          )
          AND EXISTS (
              SELECT 1
              FROM backup_folders AS folder
              WHERE folder.bucketId = backup_media.bucketId
                AND folder.selected = 1
                AND folder.available = 1
          )
        """,
    )
    abstract suspend fun retryFailed()

    companion object {
        const val STATE_PENDING = "pending"
    }
}

class RoomUploadQueue(
    private val dao: BackupDao,
) : UploadQueue {
    override suspend fun resetInterrupted() = dao.resetInterrupted()

    override suspend fun claimNextPending(): BackupMedia? = dao.claimNextPending()

    override suspend fun hasPending(): Boolean = dao.hasPending()

    override suspend fun isEligible(media: BackupMedia): Boolean = dao.isEligible(media.uri, media.remoteGeneration)

    override suspend fun saveHash(
        media: BackupMedia,
        blake3: String,
    ) = dao.saveHash(media.uri, media.remoteGeneration, blake3)

    override suspend fun saveUploadSession(
        media: BackupMedia,
        uploadId: String,
    ) = dao.saveUploadSession(media.uri, media.remoteGeneration, uploadId)

    override suspend fun clearUploadSession(media: BackupMedia) {
        dao.clearUploadSession(media.uri, media.remoteGeneration)
    }

    override suspend fun markPending(
        media: BackupMedia,
        error: String?,
    ) = dao.markPending(media.uri, media.remoteGeneration, error)

    override suspend fun markFailed(
        media: BackupMedia,
        error: String,
    ) = dao.markFailed(media.uri, media.remoteGeneration, error)

    override suspend fun markVerified(
        media: BackupMedia,
        assetId: String,
    ) = dao.markVerified(media.uri, media.remoteGeneration, assetId)
}

private fun BackupMediaEntity.sameFingerprint(media: LocalMedia): Boolean =
    sizeBytes == media.sizeBytes &&
        mimeType == media.mimeType &&
        modifiedAtSeconds == media.modifiedAtSeconds &&
        generationModified == media.generationModified

private fun BackupMediaEntity.toModel(): BackupMedia =
    BackupMedia(
        uri = uri,
        bucketId = bucketId,
        displayName = displayName,
        mimeType = mimeType,
        sizeBytes = sizeBytes,
        modifiedAtSeconds = modifiedAtSeconds,
        generationModified = generationModified,
        remoteGeneration = remoteGeneration,
        clientUploadKey = clientUploadKey,
        uploadId = uploadId,
        blake3 = blake3,
    )
