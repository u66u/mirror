package app.mirror.vault.backup

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.backup.data.BackupDao
import app.mirror.vault.backup.data.BackupFolderEntity
import app.mirror.vault.backup.data.BackupSettingsEntity
import app.mirror.vault.backup.data.LocalLibraryRow
import app.mirror.vault.backup.data.MediaStoreSource
import app.mirror.vault.backup.work.BackupScheduler
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

@Suppress("TooManyFunctions") // T206: one feature boundary coordinates Room, MediaStore, and WorkManager.
class BackupRepository(
    private val dao: BackupDao,
    private val mediaStore: MediaStoreSource,
    private val scheduler: BackupScheduler,
) : BackupScanService {
    val folders: Flow<List<BackupFolder>> =
        dao.observeFolders().map { folders ->
            folders.map { folder ->
                BackupFolder(
                    bucketId = folder.bucketId,
                    displayName = folder.displayName,
                    relativePath = folder.relativePath,
                    selected = folder.selected,
                    available = folder.available,
                )
            }
        }
    val counts: Flow<BackupCounts> =
        dao.observeCounts().map { counts ->
            BackupCounts(
                pending = counts.pending,
                uploading = counts.uploading,
                verified = counts.verified,
                failed = counts.failed,
            )
        }
    val wifiOnly: Flow<Boolean> = dao.observeWifiOnly()
    val localLibrary: Flow<List<LocalLibraryRow>> = dao.observeLocalLibrary()

    override fun permissionState(): MediaPermissionState = mediaStore.permissionState()

    fun requiredPermissions(): Array<String> = mediaStore.requiredPermissions()

    suspend fun initializeAuthenticated(credential: DeviceCredential) {
        scheduler.cancel()
        activateRemote(credential)
        refreshFolders()
        val wifiOnly = dao.wifiOnly()
        scheduler.schedulePeriodic(wifiOnly)
        if (permissionState() != MediaPermissionState.DENIED) {
            scheduler.enqueueNow(wifiOnly)
        }
    }

    fun signedOut() {
        scheduler.cancel()
    }

    override suspend fun activateRemote(credential: DeviceCredential) {
        ensureSettings()
        dao.activateRemote(credential.serverUrl)
    }

    suspend fun refreshFolders() {
        ensureSettings()
        if (permissionState() == MediaPermissionState.DENIED) {
            return
        }

        val previous = dao.folders().associateBy(BackupFolderEntity::bucketId)
        val selectionInitialized = dao.folderSelectionInitialized()
        val discovered = mediaStore.discoverFolders()
        val defaultBucket =
            if (selectionInitialized) {
                null
            } else {
                discovered
                    .firstOrNull { folder ->
                        folder.displayName.equals("Camera", ignoreCase = true) ||
                            folder.relativePath
                                ?.trimEnd('/')
                                ?.endsWith("DCIM/Camera", ignoreCase = true) == true
                    }?.bucketId
            }

        dao.markFoldersUnavailable()
        dao.upsertFolders(
            discovered.map { folder ->
                BackupFolderEntity(
                    bucketId = folder.bucketId,
                    displayName = folder.displayName,
                    relativePath = folder.relativePath,
                    selected = previous[folder.bucketId]?.selected ?: (folder.bucketId == defaultBucket),
                    available = true,
                )
            },
        )
        if (!selectionInitialized && discovered.isNotEmpty()) {
            dao.markFolderSelectionInitialized()
        }
    }

    override suspend fun scanSelectedFolders() {
        refreshFolders()
        val remoteGeneration = dao.remoteGeneration()
        dao.selectedAvailableFolders().forEach { folder ->
            dao.reconcileFolder(
                bucketId = folder.bucketId,
                scanned = mediaStore.scanFolder(folder.bucketId),
                remoteGeneration = remoteGeneration,
            )
        }
    }

    suspend fun setFolderSelected(
        bucketId: String,
        selected: Boolean,
    ) {
        ensureSettings()
        dao.markFolderSelectionInitialized()
        if (!selected) {
            scheduler.cancel()
        }
        dao.setFolderSelected(bucketId, selected)
        if (selected) {
            scheduler.enqueueNow(dao.wifiOnly())
        } else {
            scheduler.schedulePeriodic(dao.wifiOnly())
        }
    }

    /** Re-applies schedule constraints (charging) and rescans after a preference changed. */
    suspend fun preferencesChanged() {
        ensureSettings()
        val wifiOnly = dao.wifiOnly()
        scheduler.schedulePeriodic(wifiOnly)
        if (permissionState() != MediaPermissionState.DENIED) {
            refreshFolders()
            scheduler.enqueueNow(wifiOnly, replace = true)
        }
    }

    fun videoPermissionGranted(): Boolean = mediaStore.videoPermissionGranted()

    suspend fun setWifiOnly(wifiOnly: Boolean) {
        ensureSettings()
        dao.setWifiOnly(wifiOnly)
        scheduler.schedulePeriodic(wifiOnly)
        scheduler.enqueueNow(wifiOnly, replace = true)
    }

    suspend fun runNow() {
        ensureSettings()
        dao.retryFailed()
        scheduler.enqueueNow(dao.wifiOnly())
    }

    private suspend fun ensureSettings() {
        dao.insertDefaultSettings(BackupSettingsEntity())
    }
}
