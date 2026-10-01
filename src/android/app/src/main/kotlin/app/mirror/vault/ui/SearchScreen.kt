package app.mirror.vault.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import app.mirror.vault.library.SearchUiState
import app.mirror.vault.library.minQueryLength
import app.mirror.vault.network.SearchMode
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.Pill
import app.mirror.vault.ui.design.Segmented
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.tappable

private val Suggestions =
    listOf(
        "sunset over water",
        "mountains",
        "dogs",
        "city at night",
        "food",
        "snow",
        "beach",
        "people laughing",
    )

@Composable
fun SearchScreen(
    state: SearchUiState,
    columns: Int,
    gridState: LazyGridState,
    heroBounds: HeroBounds,
    callbacks: SearchCallbacks,
) {
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    PhotoGrid(
        entries = remember(state.results) { state.results.asTiles() },
        columns = columns,
        onColumnsChange = callbacks.onColumnsChange,
        state = gridState,
        callbacks = callbacks.grid,
        heroBounds = heroBounds,
        contentPadding = PaddingValues(top = top, bottom = 132.dp),
        header = {
            item(key = "search-header", span = { GridItemSpan(maxLineSpan) }) {
                SearchHeader(state, callbacks)
            }
            if (!state.searching && state.searched && state.results.isEmpty() && state.error == null) {
                item(key = "search-empty", span = { GridItemSpan(maxLineSpan) }) {
                    EmptyState(
                        title = "No matches",
                        body =
                            if (state.mode == SearchMode.SEMANTIC) {
                                "Try describing what's in the photo — a place, a colour, a moment."
                            } else {
                                "No file names contain “${state.query.trim()}”."
                            },
                        glyph = Glyphs.Search,
                    )
                }
            }
            if (!state.searched && state.query.isBlank()) {
                item(key = "search-ideas", span = { GridItemSpan(maxLineSpan) }) {
                    Ideas(state, callbacks)
                }
            }
        },
    )
}

@Composable
private fun SearchHeader(
    state: SearchUiState,
    callbacks: SearchCallbacks,
) {
    val colors = Mirror.colors
    val focus = LocalFocusManager.current
    val requester = remember { FocusRequester() }
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 14.dp)) {
        Txt("Search", style = Mirror.type.display)
        Spacer(Modifier.height(18.dp))
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(20.dp))
                .background(colors.surface)
                .border(1.dp, colors.line, RoundedCornerShape(20.dp))
                .tappable(pressedScale = 0.99f, haptic = false) { requester.requestFocus() }
                .padding(horizontal = 16.dp, vertical = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Glyph(
                if (state.mode == SearchMode.SEMANTIC) Glyphs.Sparkle else Glyphs.Search,
                tint = if (state.mode == SearchMode.SEMANTIC) colors.accent else colors.inkMuted,
            )
            Spacer(Modifier.width(12.dp))
            Box(Modifier.weight(1f)) {
                if (state.query.isEmpty()) {
                    Txt(
                        if (state.mode == SearchMode.SEMANTIC) "Describe a moment…" else "Search file names…",
                        color = colors.inkFaint,
                    )
                }
                BasicTextField(
                    value = state.query,
                    onValueChange = callbacks.onQuery,
                    singleLine = true,
                    textStyle = Mirror.type.body.copy(color = colors.ink),
                    cursorBrush = SolidColor(colors.accent),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions =
                        KeyboardActions(onSearch = {
                            callbacks.onSubmit()
                            focus.clearFocus()
                        }),
                    modifier = Modifier.fillMaxWidth().focusRequester(requester),
                )
            }
            when {
                state.searching -> Spinner(size = 18.dp, color = colors.inkMuted)
                state.query.isNotEmpty() ->
                    Glyph(
                        Glyphs.Close,
                        tint = colors.inkMuted,
                        size = 18.dp,
                        modifier = Modifier.tappable { callbacks.onQuery("") },
                    )
            }
        }
        Spacer(Modifier.height(12.dp))
        Segmented(
            options = listOf(SearchMode.SEMANTIC to "By meaning", SearchMode.FILENAME to "By file name"),
            selected = state.mode,
            onSelect = callbacks.onMode,
        )
        AnimatedVisibility(
            visible = state.semanticUnavailable && state.mode == SearchMode.SEMANTIC,
            enter = fadeIn() + expandVertically(),
            exit = fadeOut() + shrinkVertically(),
        ) {
            Notice(
                Glyphs.Alert,
                "Smart search isn't switched on for this vault yet, so you're seeing file-name matches. " +
                    "Install an image model from the web app to search by meaning.",
            )
        }
        state.error?.let { Notice(Glyphs.Alert, it) }
        if (state.query.isNotBlank() && state.query.trim().length < minQueryLength(state.mode)) {
            Txt(
                "Keep typing — meaning search needs at least ${minQueryLength(state.mode)} characters.",
                style = Mirror.type.caption,
                color = colors.inkMuted,
                modifier = Modifier.padding(top = 14.dp),
            )
        }
        if (state.searched && state.results.isNotEmpty()) {
            Spacer(Modifier.height(20.dp))
            Txt(
                if (state.results.size == 1) "1 match" else "${state.results.size} matches",
                style = Mirror.type.overline,
                color = colors.inkMuted,
            )
        }
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun Ideas(
    state: SearchUiState,
    callbacks: SearchCallbacks,
) {
    val colors = Mirror.colors
    Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp)) {
        if (state.recent.isNotEmpty()) {
            Txt("RECENT", style = Mirror.type.overline, color = colors.inkMuted)
            Spacer(Modifier.height(12.dp))
            ChipFlow(state.recent, Glyphs.Restore, callbacks.onRecent)
            Spacer(Modifier.height(26.dp))
        }
        Txt("TRY", style = Mirror.type.overline, color = colors.inkMuted)
        Spacer(Modifier.height(12.dp))
        ChipFlow(Suggestions, Glyphs.Sparkle, callbacks.onRecent)
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ChipFlow(
    values: List<String>,
    glyph: ImageVector,
    onPick: (String) -> Unit,
) {
    val colors = Mirror.colors
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        values.forEach { value ->
            Row(
                Modifier
                    .clip(Pill)
                    .background(colors.surface)
                    .border(1.dp, colors.line, Pill)
                    .tappable { onPick(value) }
                    .padding(horizontal = 14.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Glyph(glyph, tint = colors.inkFaint, size = 15.dp)
                Spacer(Modifier.width(7.dp))
                Txt(value, style = Mirror.type.label)
            }
        }
    }
}

@Composable
private fun Notice(
    glyph: ImageVector,
    text: String,
) {
    val colors = Mirror.colors
    Row(
        Modifier
            .padding(top = 14.dp)
            .fillMaxWidth()
            .clip(RoundedCornerShape(16.dp))
            .background(colors.accent.copy(alpha = 0.12f))
            .padding(14.dp),
    ) {
        Glyph(glyph, tint = colors.accent, size = 19.dp)
        Spacer(Modifier.width(10.dp))
        Txt(text, style = Mirror.type.caption, color = colors.ink)
    }
}
