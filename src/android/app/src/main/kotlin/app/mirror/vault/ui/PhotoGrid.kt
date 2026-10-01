package app.mirror.vault.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.dp
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.BackupState
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.Txt

const val MIN_COLUMNS = 2
const val MAX_COLUMNS = 6

data class GridCallbacks(
    val thumbUrl: (String) -> String?,
    val onOpen: (AssetTimelineItem) -> Unit,
    val onLongPress: (AssetTimelineItem) -> Unit,
    val onToggle: (AssetTimelineItem) -> Unit,
)

/**
 * Timeline-style grid. Month banners and day captions span the full width;
 * pinch with two fingers to change density.
 */
@Composable
@Suppress("LongParameterList") // Grid wiring is explicit on purpose.
fun PhotoGrid(
    entries: List<GridEntry>,
    columns: Int,
    onColumnsChange: (Int) -> Unit,
    state: LazyGridState,
    callbacks: GridCallbacks,
    modifier: Modifier = Modifier,
    selection: Set<String> = emptySet(),
    heroBounds: HeroBounds? = null,
    contentPadding: PaddingValues = PaddingValues(),
    header: LazyGridScope.() -> Unit = {},
    footer: LazyGridScope.() -> Unit = {},
) {
    val selecting = selection.isNotEmpty()
    LazyVerticalGrid(
        columns = GridCells.Fixed(columns),
        state = state,
        contentPadding = contentPadding,
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
        modifier = modifier.fillMaxSize().pinchColumns(columns, onColumnsChange),
    ) {
        header()
        items(
            items = entries,
            key = { it.key },
            span = { entry -> if (entry is GridEntry.Tile) GridItemSpan(1) else GridItemSpan(maxLineSpan) },
            contentType = { it::class },
        ) { entry ->
            when (entry) {
                is GridEntry.Month -> MonthBanner(entry.label, Modifier.animateItem())
                is GridEntry.Day -> DayCaption(entry, Modifier.animateItem())
                is GridEntry.Tile ->
                    PhotoTile(
                        item = entry.item,
                        url = callbacks.thumbUrl(entry.item.assetId),
                        selecting = selecting,
                        selected = entry.item.assetId in selection,
                        heroBounds = heroBounds,
                        onTap = {
                            if (selecting) callbacks.onToggle(entry.item) else callbacks.onOpen(entry.item)
                        },
                        onLongPress = { callbacks.onLongPress(entry.item) },
                        modifier = Modifier.animateItem(fadeInSpec = null, fadeOutSpec = null),
                    )
            }
        }
        footer()
    }
}

/** Plain list of tiles without date sections, e.g. search results. */
fun List<AssetTimelineItem>.asTiles(): List<GridEntry> = mapIndexed { index, item -> GridEntry.Tile(item, index) }

@Composable
private fun MonthBanner(
    label: String,
    modifier: Modifier,
) {
    Txt(
        label,
        style = Mirror.type.title,
        modifier = modifier.padding(start = 18.dp, end = 18.dp, top = 30.dp, bottom = 2.dp),
    )
}

@Composable
private fun DayCaption(
    entry: GridEntry.Day,
    modifier: Modifier,
) {
    Row(
        modifier = modifier.padding(start = 18.dp, end = 18.dp, top = 18.dp, bottom = 8.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        Txt(entry.label, style = Mirror.type.heading)
        Spacer(Modifier.width(8.dp))
        Txt("${entry.count}", style = Mirror.type.caption, color = Mirror.colors.inkFaint)
    }
}

@Composable
@Suppress("LongParameterList")
fun PhotoTile(
    item: AssetTimelineItem,
    url: String?,
    selecting: Boolean,
    selected: Boolean,
    heroBounds: HeroBounds?,
    onTap: () -> Unit,
    onLongPress: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = Mirror.colors
    val haptics = LocalHapticFeedback.current
    val inset by animateDpAsState(
        if (selected) 10.dp else 0.dp,
        spring(dampingRatio = 0.6f, stiffness = Spring.StiffnessMedium),
        label = "inset",
    )
    val corner by animateDpAsState(if (selected) 14.dp else 0.dp, label = "corner")
    if (heroBounds != null) {
        DisposableEffect(item.assetId) { onDispose { heroBounds.remove(item.assetId) } }
    }
    Box(
        modifier =
            modifier
                .aspectRatio(1f)
                .background(if (selected) colors.accent.copy(alpha = 0.16f) else Color.Transparent)
                .pointerInput(item.assetId) {
                    detectTapGestures(
                        onTap = { onTap() },
                        onLongPress = {
                            haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                            onLongPress()
                        },
                    )
                },
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .padding(inset.coerceAtLeast(0.dp))
                .clip(RoundedCornerShape(corner))
                .background(colors.tile)
                .then(
                    if (heroBounds != null) {
                        Modifier.onGloballyPositioned { heroBounds.put(item.assetId, it.boundsInWindow()) }
                    } else {
                        Modifier
                    },
                ),
        ) {
            if (item.localUri != null) {
                LocalThumbnail(item.localUri, Modifier.fillMaxSize())
            } else {
                RemoteImage(
                    url,
                    Modifier.fillMaxSize(),
                    contentDescription = item.originalFilename,
                    fallbackLabel = item.originalFilename ?: item.mediaType,
                )
            }
            if (item.backupState != BackupState.IN_VAULT) {
                BackupStateBadge(item.backupState, Modifier.align(Alignment.TopStart).padding(6.dp))
            }
            if (item.isVideo || item.isFavorite) {
                Box(
                    Modifier
                        .fillMaxSize()
                        .background(
                            Brush.verticalGradient(
                                0.6f to Color.Transparent,
                                1f to Color.Black.copy(alpha = 0.45f),
                            ),
                        ),
                )
            }
            if (item.isVideo) {
                Glyph(
                    Glyphs.Play,
                    tint = Color.White,
                    size = 14.dp,
                    modifier = Modifier.align(Alignment.BottomEnd).padding(6.dp),
                )
            }
            if (item.isFavorite) {
                Glyph(
                    Glyphs.HeartFilled,
                    tint = Color.White,
                    size = 13.dp,
                    modifier = Modifier.align(Alignment.BottomStart).padding(6.dp),
                )
            }
        }
        SelectionMark(selecting, selected)
    }
}

@Composable
private fun BoxScope.SelectionMark(
    selecting: Boolean,
    selected: Boolean,
) {
    val colors = Mirror.colors
    AnimatedVisibility(
        visible = selecting,
        enter = fadeIn() + scaleIn(initialScale = 0.6f),
        exit = fadeOut() + scaleOut(targetScale = 0.6f),
        modifier = Modifier.align(Alignment.TopEnd).padding(6.dp),
    ) {
        val scale by animateFloatAsState(if (selected) 1f else 0.9f, label = "mark")
        Box(
            Modifier
                .size(22.dp)
                .graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                }.clip(CircleShape)
                .background(if (selected) colors.accent else Color.Black.copy(alpha = 0.25f))
                .border(1.5.dp, if (selected) colors.accent else Color.White, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) Glyph(Glyphs.Check, tint = colors.onAccent, size = 14.dp)
        }
    }
}

/** Two-finger pinch steps the column count; single-finger scrolling is untouched. */
private fun Modifier.pinchColumns(
    columns: Int,
    onColumnsChange: (Int) -> Unit,
): Modifier =
    pointerInput(columns) {
        awaitEachGesture {
            awaitFirstDown(requireUnconsumed = false)
            var accumulated = 1f
            var changed = false
            do {
                val event = awaitPointerEvent(PointerEventPass.Initial)
                if (event.changes.count { it.pressed } >= 2) {
                    accumulated *= event.calculateZoom()
                    if (!changed && accumulated > 1.22f && columns > MIN_COLUMNS) {
                        onColumnsChange(columns - 1)
                        changed = true
                    } else if (!changed && accumulated < 0.82f && columns < MAX_COLUMNS) {
                        onColumnsChange(columns + 1)
                        changed = true
                    }
                    event.changes.forEach { if (it.positionChanged()) it.consume() }
                }
            } while (event.changes.any { it.pressed })
        }
    }

@Composable
fun EmptyState(
    title: String,
    body: String,
    modifier: Modifier = Modifier,
    glyph: androidx.compose.ui.graphics.vector.ImageVector = Glyphs.Photos,
    action: @Composable (() -> Unit)? = null,
) {
    val colors = Mirror.colors
    androidx.compose.foundation.layout.Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = 36.dp, vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .size(84.dp)
                .clip(CircleShape)
                .background(colors.raised),
            contentAlignment = Alignment.Center,
        ) {
            Glyph(glyph, tint = colors.inkMuted, size = 34.dp)
        }
        Spacer(Modifier.padding(top = 20.dp))
        Txt(title, style = Mirror.type.title, modifier = Modifier.padding(bottom = 8.dp))
        Txt(
            body,
            style = Mirror.type.body.copy(textAlign = androidx.compose.ui.text.style.TextAlign.Center),
            color = colors.inkMuted,
        )
        action?.let {
            Spacer(Modifier.padding(top = 22.dp))
            it()
        }
    }
}

/** Small corner marker for photos that exist only on this device. */
@Composable
fun BackupStateBadge(
    state: BackupState,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier
            .size(22.dp)
            .clip(CircleShape)
            .background(Color.Black.copy(alpha = 0.5f)),
        contentAlignment = Alignment.Center,
    ) {
        when (state) {
            BackupState.UPLOADING -> Spinner(color = Color.White, size = 12.dp, stroke = 1.6.dp)
            BackupState.FAILED -> Glyph(Glyphs.Alert, tint = Color(0xFFF0806F), size = 13.dp)
            else -> Glyph(Glyphs.Vault, tint = Color.White, size = 13.dp)
        }
    }
}

/** Short wording for where a photo stands, used in the viewer and details. */
fun BackupState.statusLabel(): String? =
    when (this) {
        BackupState.IN_VAULT -> null
        BackupState.WAITING -> "Not backed up yet"
        BackupState.UPLOADING -> "Backing up…"
        BackupState.FAILED -> "Backup failed"
    }
