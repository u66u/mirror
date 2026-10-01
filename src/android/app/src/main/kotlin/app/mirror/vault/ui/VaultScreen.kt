package app.mirror.vault.ui

import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.mirror.vault.backup.BackupCounts
import app.mirror.vault.backup.BackupFolder
import app.mirror.vault.backup.MediaPermissionState
import app.mirror.vault.settings.AppSettings
import app.mirror.vault.ui.design.ButtonTone
import app.mirror.vault.ui.design.Card
import app.mirror.vault.ui.design.CardRow
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Hairline
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.ProgressRing
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Sheet
import app.mirror.vault.ui.design.Toggle
import app.mirror.vault.ui.design.Txt

data class BackupActions(
    val requestPermission: () -> Unit,
    val selectFolder: (String, Boolean) -> Unit,
    val setWifiOnly: (Boolean) -> Unit,
    val runNow: () -> Unit,
)

data class VaultCallbacks(
    val backup: BackupActions,
    val onChargingOnly: (Boolean) -> Unit,
    val onBackupVideos: (Boolean) -> Unit,
    val onOpenTrash: () -> Unit,
    val onOpenSettings: () -> Unit,
    val onDisconnect: () -> Unit,
)

@Composable
@Suppress("LongMethod")
fun VaultScreen(
    backup: BackupUiState,
    settings: AppSettings,
    serverUrl: String,
    listState: LazyListState,
    callbacks: VaultCallbacks,
) {
    val colors = Mirror.colors
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    var confirmDisconnect by remember { mutableStateOf(false) }
    LazyColumn(
        state = listState,
        contentPadding =
            PaddingValues(start = 16.dp, end = 16.dp, top = top + 18.dp, bottom = LocalBottomInset.current),
        verticalArrangement = Arrangement.spacedBy(14.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        item {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, bottom = 6.dp), verticalAlignment = Alignment.Bottom) {
                Column(Modifier.weight(1f)) {
                    Txt("Vault", style = Mirror.type.display)
                    Txt("Backup and this device", color = colors.inkMuted)
                }
                RoundGlyphButton(
                    Glyphs.Settings,
                    callbacks.onOpenSettings,
                    contentDescription = "Settings",
                    background = colors.surface,
                )
            }
        }
        item {
            AnimatedContent(
                targetState = backup.permission == MediaPermissionState.DENIED,
                transitionSpec = { fadeIn() togetherWith fadeOut() },
                label = "hero",
            ) { denied ->
                if (denied) {
                    PermissionHero(callbacks.backup.requestPermission)
                } else {
                    BackupHero(backup, callbacks.backup.runNow)
                }
            }
        }
        if (backup.permission == MediaPermissionState.PARTIAL) {
            item {
                Card {
                    CardRow(
                        title = "Limited photo access",
                        subtitle = "Mirror can only see photos you picked. Tap to choose more or allow all.",
                        glyph = Glyphs.Alert,
                        glyphTint = colors.accent,
                        onClick = callbacks.backup.requestPermission,
                    ) { Glyph(Glyphs.Forward, tint = colors.inkFaint, size = 18.dp) }
                }
            }
        }
        if (backup.permission != MediaPermissionState.DENIED && settings.backupVideos && backup.videoAccessMissing) {
            item {
                Card {
                    CardRow(
                        title = "Allow video access",
                        subtitle = "Photos are backing up, but Mirror can't see your videos yet. Tap to grant access.",
                        glyph = Glyphs.Play,
                        glyphTint = colors.accent,
                        onClick = callbacks.backup.requestPermission,
                    ) { Glyph(Glyphs.Forward, tint = colors.inkFaint, size = 18.dp) }
                }
            }
        }
        if (backup.permission != MediaPermissionState.DENIED) {
            item { SectionLabel("Folders") }
            item {
                Card {
                    if (backup.folders.isEmpty()) {
                        CardRow(
                            title = "No photo folders found",
                            subtitle = "Take or save a photo and it will appear here.",
                            glyph = Glyphs.Folder,
                        )
                    }
                    backup.folders.forEachIndexed { index, folder ->
                        if (index > 0) Hairline()
                        FolderRow(folder, callbacks.backup.selectFolder)
                    }
                }
            }
        }
        item { SectionLabel("Backup") }
        item {
            Card {
                CardRow(
                    title = "Wi-Fi only",
                    subtitle = "Save mobile data — uploads wait for Wi-Fi.",
                    glyph = Glyphs.Wifi,
                ) { Toggle(backup.wifiOnly, callbacks.backup.setWifiOnly) }
                Hairline()
                CardRow(
                    title = "Only while charging",
                    subtitle = "Spare your battery — uploads wait until you plug in.",
                    glyph = Glyphs.Battery,
                ) { Toggle(settings.backupOnlyWhileCharging, callbacks.onChargingOnly) }
                Hairline()
                CardRow(
                    title = "Include videos",
                    subtitle = "MP4 videos back up alongside photos. Large files use more data.",
                    glyph = Glyphs.Play,
                ) { Toggle(settings.backupVideos, callbacks.onBackupVideos) }
            }
        }
        item { SectionLabel("Library") }
        item {
            Card {
                CardRow(
                    title = "Trash",
                    subtitle = "Restore or permanently delete",
                    glyph = Glyphs.Trash,
                    onClick = callbacks.onOpenTrash,
                ) { Glyph(Glyphs.Forward, tint = colors.inkFaint, size = 18.dp) }
                Hairline()
                CardRow(
                    title = "Settings",
                    subtitle = "Theme, sharing, privacy and storage",
                    glyph = Glyphs.Settings,
                    onClick = callbacks.onOpenSettings,
                ) { Glyph(Glyphs.Forward, tint = colors.inkFaint, size = 18.dp) }
            }
        }
        item { SectionLabel("Server") }
        item {
            Card {
                CardRow(
                    title = serverUrl.removePrefix("https://").removePrefix("http://"),
                    subtitle =
                        if (serverUrl.startsWith("http://")) {
                            "Local network · unencrypted"
                        } else {
                            "Encrypted connection"
                        },
                    glyph = if (serverUrl.startsWith("http://")) Glyphs.Server else Glyphs.Lock,
                )
                Hairline()
                CardRow(
                    title = "Disconnect this device",
                    glyph = Glyphs.Leave,
                    glyphTint = colors.danger,
                    onClick = { confirmDisconnect = true },
                )
            }
        }
        item {
            Txt(
                "Mirror keeps your originals on hardware you own.\nNothing is sent anywhere else.",
                style = Mirror.type.caption.copy(textAlign = TextAlign.Center),
                color = colors.inkFaint,
                modifier = Modifier.fillMaxWidth().padding(top = 18.dp),
            )
        }
    }

    Sheet(visible = confirmDisconnect, onDismiss = { confirmDisconnect = false }) {
        Txt("Disconnect?", style = Mirror.type.title)
        Spacer(Modifier.height(8.dp))
        Txt(
            "Backup stops and this device forgets the vault. Photos already backed up stay safe on your server.",
            color = colors.inkMuted,
        )
        Spacer(Modifier.height(22.dp))
        MirrorButton(
            "Disconnect",
            {
                confirmDisconnect = false
                callbacks.onDisconnect()
            },
            tone = ButtonTone.DANGER,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        MirrorButton(
            "Stay connected",
            { confirmDisconnect = false },
            tone = ButtonTone.GHOST,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun SectionLabel(text: String) {
    Txt(
        text.uppercase(),
        style = Mirror.type.overline,
        color = Mirror.colors.inkMuted,
        modifier = Modifier.padding(start = 8.dp, top = 12.dp),
    )
}

/** Tracks whether the default network is unmetered, for the Wi-Fi-only hint. */
@Composable
private fun rememberUnmetered(): Boolean {
    val context = LocalContext.current
    var unmetered by remember { mutableStateOf(true) }
    DisposableEffect(context) {
        val manager = context.getSystemService(ConnectivityManager::class.java)

        fun update() {
            unmetered =
                manager.activeNetwork
                    ?.let(manager::getNetworkCapabilities)
                    ?.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED) == true
        }
        val callback =
            object : ConnectivityManager.NetworkCallback() {
                override fun onCapabilitiesChanged(
                    network: Network,
                    capabilities: NetworkCapabilities,
                ) = update()

                override fun onLost(network: Network) = update()
            }
        update()
        manager.registerDefaultNetworkCallback(callback)
        onDispose { manager.unregisterNetworkCallback(callback) }
    }
    return unmetered
}

@Composable
private fun BackupHero(
    state: BackupUiState,
    onRunNow: () -> Unit,
) {
    val colors = Mirror.colors
    val waitingForWifi = state.wifiOnly && !rememberUnmetered()
    val counts = state.counts
    val total = counts.pending + counts.uploading + counts.verified + counts.failed
    val progress = if (total == 0L) 0f else counts.verified.toFloat() / total
    val anySelected = state.folders.any { it.selected && it.available }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(colors.surface)
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(contentAlignment = Alignment.Center) {
            ProgressRing(
                progress = progress,
                size = 176.dp,
                stroke = 12.dp,
                color = if (counts.failed > 0) colors.danger else colors.accent,
                spinning = counts.uploading > 0 || state.refreshing,
            )
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Txt(
                    if (total == 0L) "—" else "${(progress * 100).toInt()}%",
                    style = Mirror.type.display,
                )
                Txt("backed up", style = Mirror.type.caption, color = colors.inkMuted)
            }
        }
        Spacer(Modifier.height(20.dp))
        val paused = waitingForWifi && anySelected
        Txt(if (paused) "Waiting for Wi-Fi" else heroTitle(counts, anySelected), style = Mirror.type.heading)
        Spacer(Modifier.height(4.dp))
        Txt(
            if (paused) {
                "Backup resumes on Wi-Fi. Turn off “Wi-Fi only” below to use mobile data."
            } else {
                heroSubtitle(counts, anySelected, state.wifiOnly)
            },
            style = Mirror.type.body.copy(textAlign = TextAlign.Center),
            color = colors.inkMuted,
        )
        Spacer(Modifier.height(20.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Stat("${counts.verified}", "safe")
            Stat("${counts.pending + counts.uploading}", "waiting")
            if (counts.failed > 0) Stat("${counts.failed}", "failed", danger = true)
        }
        Spacer(Modifier.height(22.dp))
        MirrorButton(
            text = if (counts.failed > 0) "Retry backup" else "Back up now",
            onClick = onRunNow,
            enabled = anySelected,
            glyph = Glyphs.Vault,
            modifier = Modifier.fillMaxWidth(),
        )
        state.error?.let {
            Spacer(Modifier.height(10.dp))
            Txt(it, style = Mirror.type.caption, color = colors.danger)
        }
    }
}

private fun heroTitle(
    counts: BackupCounts,
    anySelected: Boolean,
): String =
    when {
        !anySelected -> "Choose what to protect"
        counts.failed > 0 -> "Some photos need another try"
        counts.uploading > 0 -> "Backing up…"
        counts.pending > 0 -> "Ready to back up"
        counts.verified > 0 -> "Everything is safe"
        else -> "Waiting for photos"
    }

private fun heroSubtitle(
    counts: BackupCounts,
    anySelected: Boolean,
    wifiOnly: Boolean,
): String =
    when {
        !anySelected -> "Turn on the folders below and Mirror will copy them to your vault."
        counts.uploading > 0 || counts.pending > 0 ->
            "${counts.pending + counts.uploading} left" + if (wifiOnly) " · uploads on Wi-Fi" else ""
        counts.verified > 0 -> "Every original is verified on your server."
        else -> "New photos in your folders will back up automatically."
    }

@Composable
private fun Stat(
    value: String,
    label: String,
    danger: Boolean = false,
) {
    val colors = Mirror.colors
    Column(
        Modifier
            .clip(RoundedCornerShape(16.dp))
            .background(colors.raised)
            .padding(horizontal = 18.dp, vertical = 10.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Txt(value, style = Mirror.type.heading, color = if (danger) colors.danger else colors.ink)
        Txt(label, style = Mirror.type.caption, color = colors.inkMuted)
    }
}

@Composable
private fun PermissionHero(onAllow: () -> Unit) {
    val colors = Mirror.colors
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(28.dp))
            .background(colors.surface)
            .padding(26.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .size(76.dp)
                .clip(RoundedCornerShape(24.dp))
                .background(colors.accent.copy(alpha = 0.16f)),
            contentAlignment = Alignment.Center,
        ) {
            Glyph(Glyphs.Photos, tint = colors.accent, size = 34.dp)
        }
        Spacer(Modifier.height(18.dp))
        Txt("Let Mirror see your photos", style = Mirror.type.title.copy(textAlign = TextAlign.Center))
        Spacer(Modifier.height(8.dp))
        Txt(
            "Mirror only reads them to copy originals to your own server. Nothing leaves your control.",
            style = Mirror.type.body.copy(textAlign = TextAlign.Center),
            color = colors.inkMuted,
        )
        Spacer(Modifier.height(22.dp))
        MirrorButton("Allow access", onAllow, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun FolderRow(
    folder: BackupFolder,
    onSelect: (String, Boolean) -> Unit,
) {
    val colors = Mirror.colors
    CardRow(
        title = folder.displayName,
        subtitle = if (folder.available) folder.relativePath?.trimEnd('/') else "No longer on this device",
        glyph = Glyphs.Folder,
        glyphTint = if (folder.selected) colors.accent else colors.inkMuted,
        onClick = if (folder.available) ({ onSelect(folder.bucketId, !folder.selected) }) else null,
    ) {
        Spacer(Modifier.width(12.dp))
        Toggle(folder.selected, { onSelect(folder.bucketId, it) }, enabled = folder.available)
    }
}
