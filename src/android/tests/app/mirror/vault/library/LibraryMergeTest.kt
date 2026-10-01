package app.mirror.vault.library

import app.mirror.vault.backup.data.LocalLibraryRow
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.BackupState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LibraryMergeTest {
    private fun remote(
        id: String,
        createdAt: String,
    ) = AssetTimelineItem(
        assetId = id,
        createdAt = createdAt,
        favoriteAt = null,
        originalBlake3 = "hash-$id",
        mediaType = "image/jpeg",
        sizeBytes = 10,
        originalFilename = "$id.jpg",
        thumbnail = null,
        preview = null,
    )

    private fun local(
        uri: String,
        modified: Long,
        state: String,
        assetId: String? = null,
        mime: String = "image/jpeg",
    ) = LocalLibraryRow(uri, "$uri.jpg", mime, 10, modified, state, assetId)

    @Test
    fun deviceCopyOfVaultItemCollapsesIntoOneEntryWithLocalUri() {
        val merged =
            mergeLibrary(
                remote = listOf(remote("01a-b", "2026-10-01T10:00:00Z")),
                local = listOf(local("content://1", 1_700_000_000, "verified", "01a-b")),
                remoteConfirmed = true,
                remoteComplete = true,
            )

        assertEquals(1, merged.size)
        assertEquals("01a-b", merged.single().assetId)
        assertEquals("content://1", merged.single().localUri)
        assertEquals(BackupState.IN_VAULT, merged.single().backupState)
    }

    @Test
    fun notYetBackedUpDevicePhotosAreTaggedWithTheirState() {
        val merged =
            mergeLibrary(
                remote = emptyList(),
                local =
                    listOf(
                        local("content://w", 300, "pending"),
                        local("content://u", 200, "uploading"),
                        local("content://f", 100, "failed"),
                    ),
                remoteConfirmed = true,
                remoteComplete = true,
            )

        assertEquals(
            listOf(BackupState.WAITING, BackupState.UPLOADING, BackupState.FAILED),
            merged.map { it.backupState },
        )
        assertTrue(merged.all { it.isDeviceOnly && it.assetId.startsWith(LOCAL_ID_PREFIX) })
    }

    @Test
    fun trashedItemDoesNotResurfaceAsALocalDuplicate() {
        val merged =
            mergeLibrary(
                remote = listOf(remote("01a-9", "2026-10-01T10:00:00Z")),
                local = listOf(local("content://gone", 1_700_000_000, "verified", "01a-5")),
                remoteConfirmed = true,
                remoteComplete = true,
            )

        assertNull(merged.firstOrNull { it.localUri == "content://gone" })
    }

    @Test
    fun withMorePagesPendingOnlyItemsNewerThanTheLoadedRangeAreHidden() {
        val remoteItems = listOf(remote("01a-9", "2026-10-01T10:00:00Z"), remote("01a-5", "2026-10-01T09:00:00Z"))
        val merged =
            mergeLibrary(
                remote = remoteItems,
                local =
                    listOf(
                        local("content://newer", 3, "verified", "01a-7"),
                        local("content://older", 2, "verified", "01a-1"),
                    ),
                remoteConfirmed = true,
                remoteComplete = false,
            )

        // 01a-7 lies inside the loaded range yet is absent, so it was removed; 01a-1 may be on a later page.
        assertNull(merged.firstOrNull { it.localUri == "content://newer" })
        assertNotNull(merged.firstOrNull { it.localUri == "content://older" })
    }

    @Test
    fun offlineNeverHidesAnythingBecauseAbsenceProvesNothing() {
        val merged =
            mergeLibrary(
                remote = listOf(remote("01a-9", "2026-10-01T10:00:00Z")),
                local = listOf(local("content://x", 1, "verified", "01a-5")),
                remoteConfirmed = false,
                remoteComplete = false,
            )

        assertEquals(2, merged.size)
    }

    @Test
    fun entriesSortNewestFirstUsingDeviceTimeWhereKnown() {
        val merged =
            mergeLibrary(
                remote =
                    listOf(
                        remote("01a-9", "2026-10-01T10:00:00Z"),
                        remote("01a-8", "2026-10-01T09:00:00Z"),
                    ),
                local =
                    listOf(
                        // Taken last week, uploaded today: sorts by when it was taken.
                        local("content://old", 1_759_000_000, "verified", "01a-9"),
                        local("content://new", 1_800_000_000, "pending"),
                    ),
                remoteConfirmed = true,
                remoteComplete = true,
            )

        assertEquals(listOf("${LOCAL_ID_PREFIX}content://new", "01a-8", "01a-9"), merged.map { it.assetId })
    }
}
