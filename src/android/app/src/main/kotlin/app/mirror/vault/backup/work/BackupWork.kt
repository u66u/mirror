package app.mirror.vault.backup.work

import android.content.Context
import androidx.work.Configuration
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.ListenableWorker
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequest
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerFactory
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import app.mirror.vault.backup.BackupRunResult
import app.mirror.vault.backup.BackupRunner
import java.util.concurrent.TimeUnit

class BackupWorker(
    appContext: Context,
    workerParameters: WorkerParameters,
    private val runner: BackupRunner,
) : CoroutineWorker(appContext, workerParameters) {
    override suspend fun doWork(): Result =
        when (runner.run()) {
            BackupRunResult.COMPLETE -> Result.success()
            BackupRunResult.MORE_WORK -> {
                enqueueContinuation()
                Result.success()
            }
            BackupRunResult.RETRY -> Result.retry()
            BackupRunResult.AUTHENTICATION_REQUIRED ->
                Result.failure(workDataOf(OUTPUT_REASON to REASON_AUTHENTICATION))
            BackupRunResult.PERMISSION_REQUIRED ->
                Result.failure(workDataOf(OUTPUT_REASON to REASON_PERMISSION))
        }

    private fun enqueueContinuation() {
        val wifiOnly = inputData.getBoolean(INPUT_WIFI_ONLY, true)
        WorkManager.getInstance(applicationContext).enqueueUniqueWork(
            BackupWorkRequests.IMMEDIATE_NAME,
            ExistingWorkPolicy.APPEND_OR_REPLACE,
            BackupWorkRequests.immediate(wifiOnly),
        )
    }

    companion object {
        const val INPUT_WIFI_ONLY = "wifi_only"
        const val OUTPUT_REASON = "reason"
        const val REASON_AUTHENTICATION = "authentication_required"
        const val REASON_PERMISSION = "permission_required"
    }
}

class MirrorWorkerFactory(
    private val runner: Lazy<BackupRunner>,
) : WorkerFactory() {
    override fun createWorker(
        appContext: Context,
        workerClassName: String,
        workerParameters: WorkerParameters,
    ): ListenableWorker? =
        if (workerClassName == BackupWorker::class.java.name) {
            BackupWorker(appContext, workerParameters, runner.value)
        } else {
            null
        }
}

object BackupWorkRequests {
    fun immediate(wifiOnly: Boolean): OneTimeWorkRequest =
        OneTimeWorkRequestBuilder<BackupWorker>()
            .setConstraints(constraints(wifiOnly))
            .setInputData(workDataOf(BackupWorker.INPUT_WIFI_ONLY to wifiOnly))
            .addTag(TAG)
            .build()

    fun periodic(wifiOnly: Boolean): PeriodicWorkRequest =
        PeriodicWorkRequestBuilder<BackupWorker>(PERIODIC_HOURS, TimeUnit.HOURS)
            .setConstraints(constraints(wifiOnly))
            .setInputData(workDataOf(BackupWorker.INPUT_WIFI_ONLY to wifiOnly))
            .addTag(TAG)
            .build()

    private fun constraints(wifiOnly: Boolean): Constraints =
        Constraints
            .Builder()
            .setRequiredNetworkType(
                if (wifiOnly) NetworkType.UNMETERED else NetworkType.CONNECTED,
            ).build()

    const val TAG = "mirror-backup"
    const val IMMEDIATE_NAME = "mirror-backup-now"
    const val PERIODIC_NAME = "mirror-backup-periodic"
    private const val PERIODIC_HOURS = 6L
}

class BackupScheduler(
    context: Context,
) {
    private val workManager = WorkManager.getInstance(context.applicationContext)

    fun enqueueNow(wifiOnly: Boolean) {
        workManager.enqueueUniqueWork(
            BackupWorkRequests.IMMEDIATE_NAME,
            ExistingWorkPolicy.KEEP,
            BackupWorkRequests.immediate(wifiOnly),
        )
    }

    fun schedulePeriodic(wifiOnly: Boolean) {
        workManager.enqueueUniquePeriodicWork(
            BackupWorkRequests.PERIODIC_NAME,
            ExistingPeriodicWorkPolicy.UPDATE,
            BackupWorkRequests.periodic(wifiOnly),
        )
    }

    fun cancel() {
        workManager.cancelUniqueWork(BackupWorkRequests.IMMEDIATE_NAME)
        workManager.cancelUniqueWork(BackupWorkRequests.PERIODIC_NAME)
    }
}

fun workManagerConfiguration(factory: WorkerFactory): Configuration =
    Configuration
        .Builder()
        .setWorkerFactory(factory)
        .build()
