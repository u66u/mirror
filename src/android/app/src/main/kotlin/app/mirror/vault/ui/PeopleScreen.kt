package app.mirror.vault.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.mirror.vault.library.PeopleUiState
import app.mirror.vault.library.PersonCard
import app.mirror.vault.ui.design.ButtonTone
import app.mirror.vault.ui.design.Field
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Sheet
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.shimmer
import app.mirror.vault.ui.design.tappable

@Composable
fun PeopleScreen(
    state: PeopleUiState,
    gridState: LazyGridState,
    callbacks: PeopleCallbacks,
) {
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    LazyVerticalGrid(
        columns = GridCells.Fixed(3),
        state = gridState,
        contentPadding = PaddingValues(start = 14.dp, end = 14.dp, top = top, bottom = LocalBottomInset.current),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalArrangement = Arrangement.spacedBy(22.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        item(span = { GridItemSpan(maxLineSpan) }) {
            Column(Modifier.padding(start = 6.dp, top = 18.dp, bottom = 4.dp)) {
                Txt("People", style = Mirror.type.display)
                Txt(
                    when {
                        state.people.isEmpty() -> "Faces, grouped privately on your server"
                        state.people.size == 1 -> "1 person"
                        else -> "${state.people.size} people"
                    },
                    color = Mirror.colors.inkMuted,
                )
            }
        }
        when {
            state.loading && state.people.isEmpty() ->
                items(9) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(
                            Modifier
                                .fillMaxWidth()
                                .aspectRatio(1f)
                                .clip(CircleShape)
                                .shimmer(Mirror.colors.raised),
                        )
                    }
                }
            state.people.isEmpty() && state.unassigned.isEmpty() ->
                item(span = { GridItemSpan(maxLineSpan) }) {
                    EmptyState(
                        title = if (state.error != null) "People unavailable" else "No one here yet",
                        body =
                            state.error
                                ?: "When face recognition is enabled on your vault, the people in your photos " +
                                "will gather here. Everything stays on your own server.",
                        glyph = Glyphs.People,
                        action = {
                            MirrorButton(
                                "Check again",
                                callbacks.onRefresh,
                                tone = ButtonTone.SECONDARY,
                                glyph = Glyphs.Refresh,
                            )
                        },
                    )
                }
            else -> {
                items(state.people, key = { it.person.personId }) { card ->
                    PersonBubble(card, callbacks)
                }
                if (state.unassigned.isNotEmpty()) {
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        UnsortedFaces(state, callbacks)
                    }
                }
            }
        }
    }

    val open = state.people.firstOrNull { it.person.personId == state.openPersonId }
    if (open != null) {
        PersonDetail(open, state, callbacks)
    }
}

@Composable
private fun PersonBubble(
    card: PersonCard,
    callbacks: PeopleCallbacks,
) {
    val colors = Mirror.colors
    Column(
        Modifier.tappable { callbacks.onOpenPerson(card.person.personId) },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Avatar(card.coverFaceId?.let(callbacks.chipUrl), Modifier.fillMaxWidth())
        Spacer(Modifier.height(10.dp))
        Txt(
            card.person.displayName ?: "Add a name",
            style = Mirror.type.label,
            color = if (card.person.displayName == null) colors.inkFaint else colors.ink,
            maxLines = 1,
        )
        Txt(
            "${card.person.faceCount} photo${if (card.person.faceCount == 1L) "" else "s"}",
            style = Mirror.type.caption,
            color = colors.inkMuted,
        )
    }
}

@Composable
private fun Avatar(
    url: String?,
    modifier: Modifier = Modifier,
    size: Dp? = null,
) {
    val colors = Mirror.colors
    Box(
        (if (size != null) modifier.size(size) else modifier.aspectRatio(1f))
            .clip(CircleShape)
            .background(colors.raised)
            .border(1.dp, colors.line, CircleShape),
        contentAlignment = Alignment.Center,
    ) {
        if (url == null) {
            Glyph(Glyphs.People, tint = colors.inkFaint, size = 30.dp)
        } else {
            RemoteImage(url, Modifier.fillMaxSize())
        }
    }
}

@Composable
private fun UnsortedFaces(
    state: PeopleUiState,
    callbacks: PeopleCallbacks,
) {
    val faces = state.unassigned
    Column(Modifier.padding(top = 10.dp)) {
        Txt(
            "NOT YET GROUPED",
            style = Mirror.type.overline,
            color = Mirror.colors.inkMuted,
            modifier = Modifier.padding(start = 6.dp),
        )
        Spacer(Modifier.height(12.dp))
        LazyRow(
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            contentPadding = PaddingValues(horizontal = 6.dp),
        ) {
            items(faces, key = { it.faceId }) { face ->
                Box(
                    Modifier.tappable {
                        val assets = faces.distinctBy { it.assetId }.map { it.asAsset() }
                        callbacks.onOpenAsset(assets, face.asAsset())
                    },
                ) {
                    Avatar(callbacks.chipUrl(face.faceId), size = 64.dp)
                }
            }
        }
    }
}

@Composable
@Suppress("LongMethod")
private fun PersonDetail(
    card: PersonCard,
    state: PeopleUiState,
    callbacks: PeopleCallbacks,
) {
    val colors = Mirror.colors
    var renaming by remember { mutableStateOf(false) }
    var confirmHide by remember { mutableStateOf(false) }
    val grid = rememberLazyGridState()
    val assets = remember(state.personFaces) { state.personFaces.map { it.asAsset() } }
    BackHandler(enabled = !renaming && !confirmHide) { callbacks.onOpenPerson(null) }
    Box(Modifier.fillMaxSize().background(colors.canvas)) {
        LazyVerticalGrid(
            columns = GridCells.Fixed(3),
            state = grid,
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
            contentPadding = PaddingValues(bottom = 60.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            item(span = { GridItemSpan(maxLineSpan) }) {
                Column(
                    Modifier.fillMaxWidth().statusBarsPadding().padding(bottom = 24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Row(Modifier.fillMaxWidth().padding(14.dp)) {
                        RoundGlyphButton(Glyphs.Back, { callbacks.onOpenPerson(null) }, contentDescription = "Back")
                    }
                    Avatar(card.coverFaceId?.let(callbacks.chipUrl), size = 128.dp)
                    Spacer(Modifier.height(18.dp))
                    Txt(
                        card.person.displayName ?: "Who is this?",
                        style = Mirror.type.display.copy(textAlign = TextAlign.Center),
                        color = if (card.person.displayName == null) colors.inkMuted else colors.ink,
                        modifier = Modifier.padding(horizontal = 24.dp),
                    )
                    Txt("${card.person.faceCount} photos", color = colors.inkMuted)
                    Spacer(Modifier.height(20.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        MirrorButton(
                            if (card.person.displayName == null) "Add name" else "Rename",
                            { renaming = true },
                            tone = ButtonTone.SECONDARY,
                            glyph = Glyphs.Pencil,
                        )
                        MirrorButton("Hide", { confirmHide = true }, tone = ButtonTone.SECONDARY, glyph = Glyphs.Hide)
                    }
                }
            }
            if (state.loadingFaces) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Box(Modifier.fillMaxWidth().padding(30.dp), contentAlignment = Alignment.Center) { Spinner() }
                }
            }
            items(assets, key = { it.assetId }) { asset ->
                Box(
                    Modifier
                        .aspectRatio(1f)
                        .background(colors.tile)
                        .tappable(pressedScale = 0.97f) { callbacks.onOpenAsset(assets, asset) },
                ) {
                    RemoteImage(callbacks.thumbUrl(asset.assetId), Modifier.fillMaxSize())
                }
            }
        }
    }

    var name by remember(card.person.personId, renaming) { mutableStateOf(card.person.displayName.orEmpty()) }
    Sheet(visible = renaming, onDismiss = { renaming = false }) {
        Txt(if (card.person.displayName == null) "Who is this?" else "Rename", style = Mirror.type.title)
        Spacer(Modifier.height(18.dp))
        Field(
            value = name,
            onValueChange = { name = it },
            label = "Name",
            placeholder = "e.g. Grandma",
            keyboardOptions =
                KeyboardOptions(capitalization = KeyboardCapitalization.Words, imeAction = ImeAction.Done),
            keyboardActions =
                KeyboardActions(onDone = {
                    callbacks.onRename(card.person.personId, name)
                    renaming = false
                }),
        )
        Spacer(Modifier.height(20.dp))
        MirrorButton(
            "Save",
            {
                callbacks.onRename(card.person.personId, name)
                renaming = false
            },
            enabled = name.isNotBlank(),
            modifier = Modifier.fillMaxWidth(),
        )
    }
    Sheet(visible = confirmHide, onDismiss = { confirmHide = false }) {
        Txt("Hide this person?", style = Mirror.type.title)
        Spacer(Modifier.height(8.dp))
        Txt(
            "They'll disappear from People. Photos stay in your library.",
            color = colors.inkMuted,
        )
        Spacer(Modifier.height(22.dp))
        MirrorButton(
            "Hide",
            {
                confirmHide = false
                callbacks.onHide(card.person.personId)
            },
            tone = ButtonTone.DANGER,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(8.dp))
        MirrorButton("Cancel", { confirmHide = false }, tone = ButtonTone.GHOST, modifier = Modifier.fillMaxWidth())
    }
}
