package app.mirror.vault.ui

import androidx.activity.compose.BackHandler
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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import app.mirror.vault.library.TrashUiState
import app.mirror.vault.ui.design.ButtonTone
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.LocalToaster
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Sheet
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.StatusBarScrim
import app.mirror.vault.ui.design.Txt

@Composable
@Suppress("LongMethod")
fun TrashScreen(
    state: TrashUiState,
    callbacks: TrashCallbacks,
) {
    val colors = Mirror.colors
    val toaster = LocalToaster.current
    var selection by remember { mutableStateOf(emptySet<String>()) }
    var columns by rememberSaveable { mutableStateOf(4) }
    var confirmPurge by remember { mutableStateOf(false) }
    val grid = rememberLazyGridState()
    LaunchedEffect(Unit) { callbacks.onRefresh() }
    BackHandler {
        if (selection.isNotEmpty()) selection = emptySet() else callbacks.onClose()
    }
    val targets = selection.ifEmpty { state.items.mapTo(mutableSetOf()) { it.assetId } }

    Box(Modifier.fillMaxSize().background(colors.canvas)) {
        PhotoGrid(
            entries = remember(state.items) { state.items.asTiles() },
            columns = columns,
            onColumnsChange = { columns = it },
            state = grid,
            selection = selection,
            callbacks =
                GridCallbacks(
                    thumbUrl = callbacks.thumbUrl,
                    onOpen = { selection = selection + it.assetId },
                    onLongPress = { selection = selection + it.assetId },
                    onToggle = { item ->
                        selection =
                            if (item.assetId in selection) selection - item.assetId else selection + item.assetId
                    },
                ),
            contentPadding = PaddingValues(bottom = LocalBottomInset.current + 8.dp),
            header = {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Column(Modifier.statusBarsPadding().padding(14.dp)) {
                        RoundGlyphButton(Glyphs.Back, callbacks.onClose, contentDescription = "Back")
                        Spacer(Modifier.height(18.dp))
                        Txt("Trash", style = Mirror.type.display, modifier = Modifier.padding(start = 6.dp))
                        Txt(
                            "Kept until you delete them. Tap photos to select.",
                            color = colors.inkMuted,
                            modifier = Modifier.padding(start = 6.dp, bottom = 12.dp),
                        )
                    }
                }
                if (state.loaded && state.items.isEmpty()) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        EmptyState("Trash is empty", "Photos you remove will wait here.", glyph = Glyphs.Trash)
                    }
                }
                if (state.loading && state.items.isEmpty()) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Box(Modifier.fillMaxWidth().padding(40.dp), contentAlignment = Alignment.Center) { Spinner() }
                    }
                }
            },
            footer = {
                if (state.nextCursor != null) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        LaunchedEffect(state.items.size) { callbacks.onLoadNext() }
                        Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { Spinner() }
                    }
                }
            },
        )

        AnimatedVisibility(
            visible = state.items.isNotEmpty(),
            enter = fadeIn() + slideInVertically { it },
            exit = fadeOut() + slideOutVertically { it },
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            Row(
                Modifier
                    .navigationBarsPadding()
                    .padding(horizontal = 20.dp, vertical = 18.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                MirrorButton(
                    if (selection.isEmpty()) "Restore all" else "Restore ${selection.size}",
                    {
                        callbacks.onRestore(targets) { done ->
                            toaster.show("Restored $done to your library")
                            selection = emptySet()
                        }
                    },
                    glyph = Glyphs.Restore,
                    loading = state.busy,
                    modifier = Modifier.weight(1f),
                )
                MirrorButton(
                    "Delete",
                    { confirmPurge = true },
                    tone = ButtonTone.DANGER,
                    glyph = Glyphs.Trash,
                    enabled = !state.busy,
                )
            }
        }
        StatusBarScrim(Modifier.align(Alignment.TopCenter))
    }

    Sheet(visible = confirmPurge, onDismiss = { confirmPurge = false }) {
        val count = targets.size
        Txt(if (count == 1) "Delete 1 photo forever?" else "Delete $count photos forever?", style = Mirror.type.title)
        Spacer(Modifier.height(8.dp))
        Txt(
            "The originals are removed from your server. This can't be undone.",
            color = colors.inkMuted,
        )
        Spacer(Modifier.height(22.dp))
        MirrorButton(
            "Delete forever",
            {
                confirmPurge = false
                callbacks.onPurge(targets) { done ->
                    toaster.show("Deleted $done permanently")
                    selection = emptySet()
                }
            },
            tone = ButtonTone.DANGER,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        MirrorButton("Keep", { confirmPurge = false }, tone = ButtonTone.GHOST, modifier = Modifier.fillMaxWidth())
    }
}
