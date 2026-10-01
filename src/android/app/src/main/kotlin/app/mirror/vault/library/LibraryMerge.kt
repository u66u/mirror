package app.mirror.vault.library

import app.mirror.vault.backup.data.LocalLibraryRow
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.BackupState
import java.time.Instant
import java.time.OffsetDateTime

private const val STATE_VERIFIED = "verified"
private const val STATE_UPLOADING = "uploading"
private const val STATE_FAILED = "failed"

/** Prefix for the synthetic ID of an item that exists only on this device. */
const val LOCAL_ID_PREFIX = "local:"

val AssetTimelineItem.isDeviceOnly: Boolean get() = backupState != BackupState.IN_VAULT

/**
 * Builds the one timeline users see: vault items plus whatever lives on this
 * device, de-duplicated.
 *
 * - Device photos already in the vault collapse into their vault item, which
 *   keeps the device URI so full-size viewing needs no network.
 * - Device photos not yet uploaded appear tagged with their backup state.
 * - A device photo whose vault copy is gone (trashed in the app or on the web)
 *   stays hidden, rather than resurfacing as a "local" duplicate.
 *
 * [remoteConfirmed] means the vault list is fresh from the server. When it is
 * not (offline, or still showing cached data) nothing is hidden, since absence
 * proves nothing.
 *
 * @param remoteComplete every vault page has been loaded
 */
fun mergeLibrary(
    remote: List<AssetTimelineItem>,
    local: List<LocalLibraryRow>,
    remoteConfirmed: Boolean,
    remoteComplete: Boolean,
): List<AssetTimelineItem> {
    val remoteIds = remote.mapTo(HashSet(remote.size * 2)) { it.assetId }
    // Vault IDs are time-ordered UUIDv7, so anything newer than the oldest loaded
    // vault item would have been listed already if it still existed.
    val oldestLoadedId = remote.lastOrNull()?.assetId
    val matched = HashMap<String, LocalLibraryRow>()
    val stamped = ArrayList<Pair<Long, AssetTimelineItem>>(remote.size + local.size)

    for (row in local) {
        val id = row.assetId
        if (row.state == STATE_VERIFIED && id != null) {
            when {
                id in remoteIds -> matched[id] = row
                !remoteConfirmed -> stamped += row.toItem(id, BackupState.IN_VAULT)
                remoteComplete -> Unit
                oldestLoadedId == null || id <= oldestLoadedId -> stamped += row.toItem(id, BackupState.IN_VAULT)
            }
        } else {
            stamped += row.toItem("$LOCAL_ID_PREFIX${row.uri}", row.backupState())
        }
    }
    for (item in remote) {
        val row = matched[item.assetId]
        stamped +=
            if (row != null) {
                row.modifiedAtSeconds * MILLIS to
                    item.copy(createdAt = Instant.ofEpochSecond(row.modifiedAtSeconds).toString(), localUri = row.uri)
            } else {
                item.epochMillis() to item
            }
    }
    return stamped
        .sortedWith(compareByDescending<Pair<Long, AssetTimelineItem>> { it.first }.thenBy { it.second.assetId })
        .map { it.second }
}

private const val MILLIS = 1000L

private fun LocalLibraryRow.backupState(): BackupState =
    when (state) {
        STATE_UPLOADING -> BackupState.UPLOADING
        STATE_FAILED -> BackupState.FAILED
        else -> BackupState.WAITING
    }

private fun LocalLibraryRow.toItem(
    assetId: String,
    backupState: BackupState,
): Pair<Long, AssetTimelineItem> =
    modifiedAtSeconds * MILLIS to
        AssetTimelineItem(
            assetId = assetId,
            createdAt = Instant.ofEpochSecond(modifiedAtSeconds).toString(),
            favoriteAt = null,
            originalBlake3 = "",
            mediaType = mimeType,
            sizeBytes = sizeBytes,
            originalFilename = displayName,
            thumbnail = null,
            preview = null,
            localUri = uri,
            backupState = backupState,
        )

private fun AssetTimelineItem.epochMillis(): Long =
    runCatching { OffsetDateTime.parse(createdAt).toInstant().toEpochMilli() }.getOrDefault(0L)
