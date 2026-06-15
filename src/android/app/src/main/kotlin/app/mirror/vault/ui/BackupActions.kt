package app.mirror.vault.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.mirror.vault.backup.BackupCounts
import app.mirror.vault.backup.BackupFolder
import app.mirror.vault.backup.MediaPermissionState

data class BackupActions(
    val requestPermission: () -> Unit,
    val selectFolder: (String, Boolean) -> Unit,
    val setWifiOnly: (Boolean) -> Unit,
    val runNow: () -> Unit,
)

@Composable
fun BackupContent(
    state: BackupUiState,
    actions: BackupActions,
    modifier: Modifier = Modifier,
) {
    when (state.permission) {
        MediaPermissionState.DENIED -> PermissionRequired(actions.requestPermission, modifier)
        MediaPermissionState.FULL,
        MediaPermissionState.PARTIAL,
        -> BackupFolderList(state, actions, modifier)
    }
}

@Composable
private fun PermissionRequired(
    onRequestPermission: () -> Unit,
    modifier: Modifier,
) {
    Box(modifier = modifier.fillMaxWidth()) {
        Column(
            modifier =
                Modifier
                    .align(Alignment.Center)
                    .padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("Photo access required", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(16.dp))
            Button(onClick = onRequestPermission) {
                Text("Allow access")
            }
        }
    }
}

@Composable
private fun BackupFolderList(
    state: BackupUiState,
    actions: BackupActions,
    modifier: Modifier,
) {
    LazyColumn(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = 20.dp, vertical = 16.dp),
    ) {
        item {
            BackupStatus(state, actions)
        }
        if (state.permission == MediaPermissionState.PARTIAL) {
            item {
                PartialAccessWarning(actions.requestPermission)
            }
        }
        item {
            Text(
                "Folders",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(top = 20.dp, bottom = 8.dp),
            )
        }
        if (state.folders.isEmpty()) {
            item {
                Text(
                    "No supported photo folders",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(vertical = 16.dp),
                )
            }
        }
        items(
            items = state.folders,
            key = BackupFolder::bucketId,
        ) { folder ->
            FolderRow(folder, actions.selectFolder)
        }
        state.error?.let { message ->
            item {
                Text(
                    message,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 16.dp),
                )
            }
        }
    }
}

@Composable
private fun PartialAccessWarning(onRequestPermission: () -> Unit) {
    Text(
        "Limited photo access",
        color = MaterialTheme.colorScheme.error,
        modifier =
            Modifier
                .fillMaxWidth()
                .clickable(onClick = onRequestPermission)
                .padding(vertical = 12.dp),
    )
}

@Composable
private fun BackupStatus(
    state: BackupUiState,
    actions: BackupActions,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column {
            Text("Backup", style = MaterialTheme.typography.titleLarge)
            Text(
                backupStatusText(state.counts),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (state.refreshing || state.counts.uploading > 0) {
            CircularProgressIndicator(modifier = Modifier.size(24.dp), strokeWidth = 2.dp)
        }
    }
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(top = 16.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text("Wi-Fi only")
        Switch(
            checked = state.wifiOnly,
            onCheckedChange = actions.setWifiOnly,
        )
    }
    Button(
        onClick = actions.runNow,
        enabled = state.folders.any { it.selected && it.available },
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(top = 12.dp),
    ) {
        Text(if (state.counts.failed > 0) "Retry backup" else "Back up now")
    }
}

@Composable
private fun FolderRow(
    folder: BackupFolder,
    onFolderSelected: (String, Boolean) -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .clickable(enabled = folder.available) {
                    onFolderSelected(folder.bucketId, !folder.selected)
                }.padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(
            checked = folder.selected,
            onCheckedChange = { selected ->
                onFolderSelected(folder.bucketId, selected)
            },
            enabled = folder.available,
        )
        Column(modifier = Modifier.padding(start = 8.dp)) {
            Text(folder.displayName)
            Text(
                folder.relativePath ?: if (folder.available) "" else "Unavailable",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

private fun backupStatusText(counts: BackupCounts): String =
    when {
        counts.uploading > 0 -> "Uploading ${counts.uploading}"
        counts.pending > 0 -> "${counts.pending} waiting"
        counts.failed > 0 -> "${counts.failed} failed"
        counts.verified > 0 -> "${counts.verified} backed up"
        else -> "No photos queued"
    }
