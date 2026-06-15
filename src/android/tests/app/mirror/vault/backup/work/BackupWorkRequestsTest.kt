package app.mirror.vault.backup.work

import androidx.work.NetworkType
import org.junit.Assert.assertEquals
import org.junit.Test

class BackupWorkRequestsTest {
    @Test
    fun wifi_default_uses_unmetered_network_constraint() {
        val request = BackupWorkRequests.immediate(wifiOnly = true)

        assertEquals(
            NetworkType.UNMETERED,
            request.workSpec.constraints.requiredNetworkType,
        )
    }
}
