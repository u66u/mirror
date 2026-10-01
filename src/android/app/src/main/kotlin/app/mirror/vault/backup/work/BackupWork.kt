package app.mirror.vault.backup.work

import android.content.Context
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
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
            BackupWorkRequests.immediate(wifiOnly, inputData.getBoolean(INPUT_CHARGING_ONLY, false)),
        )
    }

    companion object {
        const val INPUT_WIFI_ONLY = "wifi_only"
        const val INPUT_CHARGING_ONLY = "charging_only"
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
    fun immediate(
        wifiOnly: Boolean,
        chargingOnly: Boolean = false,
    ): OneTimeWorkRequest =
        OneTimeWorkRequestBuilder<BackupWorker>()
            .setConstraints(constraints(wifiOnly, chargingOnly))
            .setInputData(
                workDataOf(
                    BackupWorker.INPUT_WIFI_ONLY to wifiOnly,
                    BackupWorker.INPUT_CHARGING_ONLY to chargingOnly,
                ),
            ).addTag(TAG)
            .build()

    fun periodic(
        wifiOnly: Boolean,
        chargingOnly: Boolean = false,
    ): PeriodicWorkRequest =
        PeriodicWorkRequestBuilder<BackupWorker>(PERIODIC_HOURS, TimeUnit.HOURS)
            .setConstraints(constraints(wifiOnly, chargingOnly))
            .setInputData(
                workDataOf(
                    BackupWorker.INPUT_WIFI_ONLY to wifiOnly,
                    BackupWorker.INPUT_CHARGING_ONLY to chargingOnly,
                ),
            ).addTag(TAG)
            .build()

    /**
     * The vault is often LAN-only, so backup must not wait for Android's
     * internet validation (captive-portal probe). The plain network type stays
     * as the fallback for WorkManager's pre-API-28 path and for JVM tests.
     */
    private fun constraints(
        wifiOnly: Boolean,
        chargingOnly: Boolean,
    ): Constraints {
        val type = if (wifiOnly) NetworkType.UNMETERED else NetworkType.CONNECTED
        val builder =
            Constraints
                .Builder()
                .setRequiredNetworkType(type)
                .setRequiresCharging(chargingOnly)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val request =
                NetworkRequest
                    .Builder()
                    .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                    .removeCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
                    .apply {
                        if (wifiOnly) addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED)
                    }.build()
            builder.setRequiredNetworkRequest(request, type)
        }
        return builder.build()
    }

    const val TAG = "mirror-backup"
    const val IMMEDIATE_NAME = "mirror-backup-now"
    const val PERIODIC_NAME = "mirror-backup-periodic"
    private const val PERIODIC_HOURS = 6L
}

class BackupScheduler(
    context: Context,
    private val chargingOnly: () -> Boolean = { false },
) {
    private val workManager = WorkManager.getInstance(context.applicationContext)

    /**
     * [replace] swaps any queued run for one with fresh constraints. Needed when
     * the network preference changes: a kept request would stay blocked on the
     * old constraint, and continuations append behind it.
     */
    fun enqueueNow(
        wifiOnly: Boolean,
        replace: Boolean = false,
    ) {
        workManager.enqueueUniqueWork(
            BackupWorkRequests.IMMEDIATE_NAME,
            if (replace) ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.KEEP,
            BackupWorkRequests.immediate(wifiOnly, chargingOnly()),
        )
    }

    fun schedulePeriodic(wifiOnly: Boolean) {
        workManager.enqueueUniquePeriodicWork(
            BackupWorkRequests.PERIODIC_NAME,
            ExistingPeriodicWorkPolicy.UPDATE,
            BackupWorkRequests.periodic(wifiOnly, chargingOnly()),
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
