package app.mirror.vault.ui

import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.SearchMode
import app.mirror.vault.settings.ShareExpiry
import app.mirror.vault.settings.ThemeMode

data class SearchCallbacks(
    val onQuery: (String) -> Unit,
    val onMode: (SearchMode) -> Unit,
    val onSubmit: () -> Unit,
    val onRecent: (String) -> Unit,
    val grid: GridCallbacks,
    val onColumnsChange: (Int) -> Unit,
)

data class TrashCallbacks(
    val thumbUrl: (String) -> String?,
    val onRefresh: () -> Unit,
    val onLoadNext: () -> Unit,
    val onRestore: (Set<String>, (Int) -> Unit) -> Unit,
    val onPurge: (Set<String>, (Int) -> Unit) -> Unit,
    val onClose: () -> Unit,
)

data class PeopleCallbacks(
    val chipUrl: (String) -> String?,
    val thumbUrl: (String) -> String?,
    val onOpenPerson: (String?) -> Unit,
    val onRename: (String, String) -> Unit,
    val onHide: (String) -> Unit,
    val onOpenAsset: (List<AssetTimelineItem>, AssetTimelineItem) -> Unit,
    val onRefresh: () -> Unit,
)

data class SettingsCallbacks(
    val onTheme: (ThemeMode) -> Unit,
    val onShareExpiry: (ShareExpiry) -> Unit,
    val onHidePreviews: (Boolean) -> Unit,
    val onClose: () -> Unit,
)
