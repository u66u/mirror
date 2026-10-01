package app.mirror.vault

import android.app.Application
import androidx.work.Configuration
import app.mirror.vault.auth.AndroidKeystoreTokenStore
import app.mirror.vault.auth.AuthRepository
import app.mirror.vault.backup.BackupRepository
import app.mirror.vault.backup.BackupRunner
import app.mirror.vault.backup.DefaultBackupRunner
import app.mirror.vault.backup.UploadCoordinator
import app.mirror.vault.backup.data.MediaStoreSource
import app.mirror.vault.backup.data.MirrorDatabase
import app.mirror.vault.backup.data.RoomUploadQueue
import app.mirror.vault.backup.work.BackupScheduler
import app.mirror.vault.backup.work.MirrorWorkerFactory
import app.mirror.vault.backup.work.workManagerConfiguration
import app.mirror.vault.library.LibraryRepository
import app.mirror.vault.network.KtorMirrorApi
import app.mirror.vault.settings.AppPreferences
import app.mirror.vault.timeline.TimelineCache
import app.mirror.vault.timeline.TimelineRepository

class MirrorApplication :
    Application(),
    Configuration.Provider {
    private val api by lazy { KtorMirrorApi() }
    private val tokenStore by lazy { AndroidKeystoreTokenStore(this) }
    private val database by lazy { MirrorDatabase.create(this) }
    val appPreferences: AppPreferences by lazy { AppPreferences(this) }
    private val mediaStore by lazy { MediaStoreSource(this, includeVideos = { appPreferences.current.backupVideos }) }
    private val uploadQueue by lazy { RoomUploadQueue(database.backupDao()) }
    private val backupScheduler by lazy {
        BackupScheduler(this, chargingOnly = { appPreferences.current.backupOnlyWhileCharging })
    }

    val authRepository: AuthRepository by lazy {
        AuthRepository(
            api = api,
            tokenStore = tokenStore,
        )
    }

    val backupRepository: BackupRepository by lazy {
        BackupRepository(
            dao = database.backupDao(),
            mediaStore = mediaStore,
            scheduler = backupScheduler,
        )
    }

    val timelineRepository: TimelineRepository by lazy {
        TimelineRepository(api = api)
    }

    val timelineCache: TimelineCache by lazy { TimelineCache(this) }

    val libraryRepository: LibraryRepository by lazy {
        LibraryRepository(api = api, shareLifetimeSeconds = { appPreferences.current.shareExpiry.seconds })
    }

    private val backupRunner: BackupRunner by lazy {
        DefaultBackupRunner(
            repository = backupRepository,
            tokenStore = tokenStore,
            queue = uploadQueue,
            coordinator =
                UploadCoordinator(
                    api = api,
                    source = mediaStore,
                    queue = uploadQueue,
                ),
        )
    }
    private val workerFactory by lazy {
        MirrorWorkerFactory(lazy { backupRunner })
    }

    override val workManagerConfiguration: Configuration
        get() = workManagerConfiguration(workerFactory)
}
