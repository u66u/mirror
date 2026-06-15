package app.mirror.vault.backup.work

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import androidx.work.ListenableWorker
import androidx.work.testing.TestListenableWorkerBuilder
import app.mirror.vault.backup.BackupRunResult
import app.mirror.vault.backup.BackupRunner
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BackupWorkerTest {
    @Test
    fun transient_runner_result_requests_workmanager_retry() =
        runBlocking {
            val worker = workerReturning(BackupRunResult.RETRY)

            assertTrue(worker.doWork() is ListenableWorker.Result.Retry)
        }

    @Test
    fun authentication_failure_is_terminal_until_new_login() =
        runBlocking {
            val worker = workerReturning(BackupRunResult.AUTHENTICATION_REQUIRED)
            val result = worker.doWork()

            assertTrue(result is ListenableWorker.Result.Failure)
            assertEquals(
                BackupWorker.REASON_AUTHENTICATION,
                result.outputData.getString(BackupWorker.OUTPUT_REASON),
            )
        }

    private fun workerReturning(result: BackupRunResult): BackupWorker {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val runner = BackupRunner { result }
        return TestListenableWorkerBuilder<BackupWorker>(context)
            .setWorkerFactory(MirrorWorkerFactory(lazyOf(runner)))
            .build()
    }
}
