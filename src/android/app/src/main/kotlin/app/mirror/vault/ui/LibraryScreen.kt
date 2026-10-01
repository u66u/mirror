package app.mirror.vault.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import app.mirror.vault.backup.BackupCounts
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.timeline.TimelineUiState
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.Pill
import app.mirror.vault.ui.design.ProgressRing
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Segmented
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.shimmer
import app.mirror.vault.ui.design.tappable

enum class LibraryFilter(
    val label: String,
) {
    ALL("All"),
    FAVORITES("Favorites"),
    VIDEOS("Videos"),
}

data class LibraryCallbacks(
    val grid: GridCallbacks,
    val onColumnsChange: (Int) -> Unit,
    val onFilterChange: (LibraryFilter) -> Unit,
    val onLoadNext: () -> Unit,
    val onRefresh: () -> Unit,
    val onOpenVault: () -> Unit,
    val onClearSelection: () -> Unit,
)

@Composable
@Suppress("LongParameterList", "LongMethod")
fun LibraryScreen(
    timeline: TimelineUiState,
    hasItems: Boolean,
    visible: List<AssetTimelineItem>,
    entries: List<GridEntry>,
    backup: BackupCounts,
    filter: LibraryFilter,
    columns: Int,
    selection: Set<String>,
    gridState: LazyGridState,
    heroBounds: HeroBounds,
    callbacks: LibraryCallbacks,
) {
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val scrolled by remember { derivedStateOf { gridState.firstVisibleItemIndex > 0 } }
    val currentSection by remember(entries) {
        // The grid has one header item before the entries, hence the -1.
        derivedStateOf { sectionTitleAt(entries, gridState.firstVisibleItemIndex - 1) }
    }

    LaunchedEffect(gridState, entries.size, timeline.nextCursor) {
        snapshotFlow {
            gridState.layoutInfo.visibleItemsInfo
                .lastOrNull()
                ?.index ?: 0
        }.collect { last ->
            if (timeline.nextCursor != null && last >= entries.size - 24) callbacks.onLoadNext()
        }
    }

    Box(Modifier.fillMaxSize()) {
        when {
            timeline.loadingInitial && !hasItems -> SkeletonGrid(columns, top)
            !hasItems ->
                Column(Modifier.fillMaxSize().statusBarsPadding()) {
                    LibraryHeader(timeline, visible.size, backup, filter, callbacks, showFilters = false)
                    if (timeline.error != null) {
                        EmptyState(
                            title = "Can't reach your vault",
                            body = friendlyError(timeline.error),
                            glyph = Glyphs.Server,
                            action = { MirrorButton("Try again", callbacks.onRefresh, glyph = Glyphs.Refresh) },
                        )
                    } else {
                        EmptyState(
                            title = "Nothing here yet",
                            body = "Turn on backup and your photos will gather here, safe on your own server.",
                            glyph = Glyphs.Vault,
                            action = { MirrorButton("Set up backup", callbacks.onOpenVault, glyph = Glyphs.Vault) },
                        )
                    }
                }
            else ->
                PhotoGrid(
                    entries = entries,
                    columns = columns,
                    onColumnsChange = callbacks.onColumnsChange,
                    state = gridState,
                    callbacks = callbacks.grid,
                    selection = selection,
                    heroBounds = heroBounds,
                    contentPadding = PaddingValues(top = top, bottom = LocalBottomInset.current),
                    header = {
                        item(key = "header", span = { GridItemSpan(maxLineSpan) }) {
                            LibraryHeader(timeline, visible.size, backup, filter, callbacks, showFilters = true)
                        }
                        if (visible.isEmpty()) {
                            item(key = "filtered-empty", span = { GridItemSpan(maxLineSpan) }) {
                                EmptyState(
                                    title =
                                        if (filter == LibraryFilter.FAVORITES) {
                                            "No favorites yet"
                                        } else {
                                            "No videos yet"
                                        },
                                    body =
                                        if (filter == LibraryFilter.FAVORITES) {
                                            "Tap the heart on any photo to keep it close."
                                        } else {
                                            "Videos you back up will show up here."
                                        },
                                    glyph = if (filter == LibraryFilter.FAVORITES) Glyphs.Heart else Glyphs.Play,
                                )
                            }
                        }
                    },
                    footer = {
                        if (timeline.loadingNext) {
                            item(key = "more", span = { GridItemSpan(maxLineSpan) }) {
                                Box(Modifier.fillMaxWidth().padding(28.dp), contentAlignment = Alignment.Center) {
                                    Spinner(color = Mirror.colors.inkMuted)
                                }
                            }
                        }
                    },
                )
        }

        AnimatedVisibility(
            visible = scrolled && selection.isEmpty(),
            enter = fadeIn() + slideInVertically { -it / 2 },
            exit = fadeOut() + slideOutVertically { -it / 2 },
        ) {
            CompactBar(title = currentSection ?: "Library", backup = backup, onOpenVault = callbacks.onOpenVault)
        }
        AnimatedVisibility(
            visible = selection.isNotEmpty(),
            enter = fadeIn() + slideInVertically { -it },
            exit = fadeOut() + slideOutVertically { -it },
        ) {
            SelectionBar(count = selection.size, onClear = callbacks.onClearSelection)
        }
    }
}

@Composable
@Suppress("LongParameterList")
private fun LibraryHeader(
    timeline: TimelineUiState,
    count: Int,
    backup: BackupCounts,
    filter: LibraryFilter,
    callbacks: LibraryCallbacks,
    showFilters: Boolean,
) {
    val colors = Mirror.colors
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Txt("Library", style = Mirror.type.display)
                val more = if (timeline.nextCursor != null) "+" else ""
                if (count > 0) {
                    Txt(
                        if (count == 1) "1 item" else "$count$more items",
                        style = Mirror.type.body,
                        color = colors.inkMuted,
                    )
                }
            }
            BackupBadge(backup, callbacks.onOpenVault)
        }
        if (showFilters) {
            Spacer(Modifier.height(18.dp))
            Segmented(
                options = LibraryFilter.entries.map { it to it.label },
                selected = filter,
                onSelect = callbacks.onFilterChange,
            )
            if (timeline.offline) {
                OfflineNotice(onRetry = callbacks.onRefresh)
            }
        }
    }
}

@Composable
fun BackupBadge(
    counts: BackupCounts,
    onClick: () -> Unit,
) {
    val colors = Mirror.colors
    val total = counts.pending + counts.uploading + counts.verified + counts.failed
    val remaining = counts.pending + counts.uploading
    val active = counts.uploading > 0
    val label =
        when {
            counts.failed > 0 -> "${counts.failed} need attention"
            remaining > 0 -> "$remaining to go"
            total > 0 -> "Backed up"
            // Not a status claim: the vault may be full while this device just isn't uploading yet.
            else -> "Set up backup"
        }
    Row(
        Modifier
            .clip(Pill)
            .background(colors.surface)
            .tappable(onClick = onClick)
            .padding(start = 8.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ProgressRing(
            progress = if (total == 0L) 0f else counts.verified.toFloat() / total,
            size = 20.dp,
            stroke = 3.dp,
            color = if (counts.failed > 0) colors.danger else colors.accent,
            spinning = active,
        )
        Spacer(Modifier.width(8.dp))
        Txt(label, style = Mirror.type.label, color = colors.ink)
    }
}

@Composable
private fun CompactBar(
    title: String,
    backup: BackupCounts,
    onOpenVault: () -> Unit,
) {
    val colors = Mirror.colors
    Row(
        Modifier
            .fillMaxWidth()
            .background(colors.canvas)
            .statusBarsPadding()
            .padding(horizontal = 20.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Txt(title, style = Mirror.type.heading, modifier = Modifier.weight(1f))
        BackupBadge(backup, onOpenVault)
    }
}

@Composable
fun SelectionBar(
    count: Int,
    onClear: () -> Unit,
) {
    val colors = Mirror.colors
    Row(
        Modifier
            .fillMaxWidth()
            .background(colors.canvas)
            .statusBarsPadding()
            .padding(horizontal = 14.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RoundGlyphButton(Glyphs.Close, onClear, contentDescription = "Clear selection")
        Spacer(Modifier.width(14.dp))
        Txt(if (count == 1) "1 selected" else "$count selected", style = Mirror.type.heading)
    }
}

@Composable
fun SelectionActions(
    allFavorite: Boolean,
    onFavorite: () -> Unit,
    onShare: (() -> Unit)?,
    onTrash: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = Mirror.colors
    Row(
        modifier
            .clip(Pill)
            .background(colors.ink)
            .padding(6.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        RoundGlyphButton(
            if (allFavorite) Glyphs.HeartFilled else Glyphs.Heart,
            onFavorite,
            tint = if (allFavorite) colors.accent else colors.canvas,
            background = colors.canvas.copy(alpha = 0.08f),
            size = 52.dp,
            contentDescription = "Favorite",
        )
        if (onShare != null) {
            RoundGlyphButton(
                Glyphs.Share,
                onShare,
                tint = colors.canvas,
                background = colors.canvas.copy(alpha = 0.08f),
                size = 52.dp,
                contentDescription = "Share",
            )
        }
        RoundGlyphButton(
            Glyphs.Trash,
            onTrash,
            tint = colors.danger,
            background = colors.canvas.copy(alpha = 0.08f),
            size = 52.dp,
            contentDescription = "Move to trash",
        )
    }
}

@Composable
fun SkeletonGrid(
    columns: Int,
    top: androidx.compose.ui.unit.Dp,
) {
    val colors = Mirror.colors
    LazyVerticalGrid(
        columns = GridCells.Fixed(columns),
        contentPadding = PaddingValues(top = top),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
        userScrollEnabled = false,
    ) {
        item(span = { GridItemSpan(maxLineSpan) }) {
            Column(Modifier.padding(start = 20.dp, top = 18.dp, bottom = 24.dp)) {
                Box(Modifier.size(170.dp, 40.dp).clip(Pill).shimmer(colors.raised))
                Spacer(Modifier.height(10.dp))
                Box(Modifier.size(90.dp, 14.dp).clip(Pill).shimmer(colors.raised))
            }
        }
        items(columns * 7) {
            Box(Modifier.aspectRatio(1f).shimmer(colors.tile))
        }
    }
}

/** Shown when the vault can't be reached but device photos are still browsable. */
@Composable
private fun OfflineNotice(onRetry: () -> Unit) {
    val colors = Mirror.colors
    Row(
        Modifier
            .padding(top = 14.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(colors.accent.copy(alpha = 0.12f))
            .padding(start = 14.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Glyph(Glyphs.Server, tint = colors.accent, size = 19.dp)
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Txt("Can't reach your vault", style = Mirror.type.label)
            Txt("Showing what's on this device", style = Mirror.type.caption, color = colors.inkMuted)
        }
        Box(
            Modifier
                .clip(Pill)
                .background(colors.accent)
                .tappable(onClick = onRetry)
                .padding(horizontal = 14.dp, vertical = 8.dp),
        ) {
            Txt("Retry", style = Mirror.type.label, color = colors.onAccent)
        }
    }
}
