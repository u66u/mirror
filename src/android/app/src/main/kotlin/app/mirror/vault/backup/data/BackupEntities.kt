package app.mirror.vault.backup.data

import androidx.room.Entity
import androidx.room.Index
import androidx.room.PrimaryKey

@Entity(tableName = "backup_folders")
data class BackupFolderEntity(
    @PrimaryKey val bucketId: String,
    val displayName: String,
    val relativePath: String?,
    val selected: Boolean,
    val available: Boolean,
)

@Entity(
    tableName = "backup_media",
    indices = [
        Index(value = ["bucketId"]),
        Index(value = ["remoteGeneration", "state", "present", "modifiedAtSeconds"]),
    ],
)
data class BackupMediaEntity(
    @PrimaryKey val uri: String,
    val bucketId: String,
    val displayName: String,
    val mimeType: String,
    val sizeBytes: Long,
    val modifiedAtSeconds: Long,
    val generationModified: Long,
    val remoteGeneration: Long,
    val present: Boolean,
    val state: String,
    val clientUploadKey: String,
    val uploadId: String?,
    val blake3: String?,
    val assetId: String?,
    val lastError: String?,
)

@Entity(tableName = "backup_settings")
data class BackupSettingsEntity(
    @PrimaryKey val id: Int = SINGLETON_ID,
    val wifiOnly: Boolean = true,
    val folderSelectionInitialized: Boolean = false,
    val remoteScope: String? = null,
    val remoteGeneration: Long = 0,
) {
    companion object {
        const val SINGLETON_ID = 1
    }
}

data class BackupCountsRow(
    val pending: Long,
    val uploading: Long,
    val verified: Long,
    val failed: Long,
)

data class RemoteStateRow(
    val remoteScope: String?,
    val remoteGeneration: Long,
)
