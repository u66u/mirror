package app.mirror.vault.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.timeline.TimelineDerivativeKind
import app.mirror.vault.timeline.TimelineUiState
import coil3.compose.AsyncImage
import coil3.network.NetworkHeaders
import coil3.network.httpHeaders
import coil3.request.ImageRequest
import coil3.request.crossfade

private val TileBackground = Color(0xFFE3E4DF)
private val PreviewBackground = Color(0xFF080A09)

internal data class TimelineActions(
    val refresh: () -> Unit,
    val loadNext: () -> Unit,
    val open: (String) -> Unit,
    val close: () -> Unit,
    val next: () -> Unit,
    val previous: () -> Unit,
    val imageUrl: (AssetTimelineItem, TimelineDerivativeKind) -> String?,
    val authorizationHeader: () -> String?,
)

@Composable
internal fun TimelineContent(
    state: TimelineUiState,
    actions: TimelineActions,
    modifier: Modifier = Modifier,
) {
    Box(modifier = modifier.fillMaxSize()) {
        when {
            state.loadingInitial && state.items.isEmpty() ->
                CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
            state.items.isEmpty() ->
                EmptyTimeline(
                    error = state.error,
                    onRefresh = actions.refresh,
                    modifier = Modifier.align(Alignment.Center),
                )
            else ->
                TimelineGrid(
                    state = state,
                    actions = actions,
                )
        }
        TimelineError(
            error = state.error,
            modifier = Modifier.align(Alignment.BottomCenter),
        )
        TimelinePreview(
            state = state,
            actions = actions,
        )
    }
}

@Composable
private fun EmptyTimeline(
    error: String?,
    onRefresh: () -> Unit,
    modifier: Modifier,
) {
    Column(
        modifier = modifier.padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = error ?: "No photos yet",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(12.dp))
        TextButton(onClick = onRefresh) {
            Text("Refresh")
        }
    }
}

@Composable
private fun TimelineGrid(
    state: TimelineUiState,
    actions: TimelineActions,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 112.dp),
        modifier = Modifier.fillMaxSize(),
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        itemsIndexed(
            items = state.items,
            key = { _, item -> item.assetId },
        ) { index, item ->
            if (index >= state.items.lastIndex - 12 && state.nextCursor != null) {
                LaunchedEffect(state.nextCursor, index) {
                    actions.loadNext()
                }
            }
            TimelineTile(
                item = item,
                imageUrl = actions.imageUrl(item, TimelineDerivativeKind.THUMBNAIL),
                authorizationHeader = actions.authorizationHeader(),
                onOpen = { actions.open(item.assetId) },
            )
        }
        if (state.nextCursor != null) {
            item(span = { GridItemSpan(maxLineSpan) }) {
                Button(
                    onClick = actions.loadNext,
                    enabled = !state.loadingNext,
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .padding(16.dp),
                ) {
                    if (state.loadingNext) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(18.dp),
                            strokeWidth = 2.dp,
                        )
                    } else {
                        Text("Load more")
                    }
                }
            }
        }
    }
}

@Composable
private fun TimelineTile(
    item: AssetTimelineItem,
    imageUrl: String?,
    authorizationHeader: String?,
    onOpen: () -> Unit,
) {
    Box(
        modifier =
            Modifier
                .aspectRatio(1f)
                .background(TileBackground)
                .clickable(onClick = onOpen),
    ) {
        AuthenticatedImage(
            url = imageUrl,
            authorizationHeader = authorizationHeader,
            contentDescription = item.originalFilename,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxSize(),
        )
        if (item.mediaType.startsWith("video/")) {
            VideoMarker()
        }
    }
}

@Composable
private fun BoxScope.VideoMarker() {
    Box(
        modifier =
            Modifier
                .align(Alignment.TopEnd)
                .padding(6.dp)
                .clip(RoundedCornerShape(6.dp))
                .background(Color.Black.copy(alpha = 0.56f))
                .padding(horizontal = 7.dp, vertical = 3.dp),
    ) {
        Text("▶", color = Color.White)
    }
}

@Composable
private fun TimelineError(
    error: String?,
    modifier: Modifier,
) {
    error ?: return
    Text(
        text = error,
        color = MaterialTheme.colorScheme.error,
        modifier =
            modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface.copy(alpha = 0.94f))
                .padding(horizontal = 20.dp, vertical = 10.dp),
    )
}

@Composable
private fun TimelinePreview(
    state: TimelineUiState,
    actions: TimelineActions,
) {
    val item = state.items.firstOrNull { it.assetId == state.selectedAssetId } ?: return
    Dialog(
        onDismissRequest = actions.close,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            modifier =
                Modifier
                    .fillMaxSize()
                    .background(PreviewBackground),
        ) {
            AuthenticatedImage(
                url = actions.imageUrl(item, TimelineDerivativeKind.PREVIEW),
                authorizationHeader = actions.authorizationHeader(),
                contentDescription = item.originalFilename,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize(),
            )
            PreviewControls(
                title = item.originalFilename ?: item.createdAt,
                actions = actions,
            )
        }
    }
}

@Composable
private fun BoxScope.PreviewControls(
    title: String,
    actions: TimelineActions,
) {
    Row(
        modifier =
            Modifier
                .align(Alignment.TopCenter)
                .fillMaxWidth()
                .background(Color.Black.copy(alpha = 0.58f))
                .padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = title,
            color = Color.White,
            maxLines = 1,
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.width(8.dp))
        TextButton(onClick = actions.close) {
            Text("Close", color = Color.White)
        }
    }
    Row(
        modifier =
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Color.Black.copy(alpha = 0.42f))
                .padding(horizontal = 16.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        TextButton(onClick = actions.previous) {
            Text("Previous", color = Color.White)
        }
        TextButton(onClick = actions.next) {
            Text("Next", color = Color.White)
        }
    }
}

@Composable
private fun AuthenticatedImage(
    url: String?,
    authorizationHeader: String?,
    contentDescription: String?,
    contentScale: ContentScale,
    modifier: Modifier,
) {
    if (url == null || authorizationHeader == null) {
        Box(modifier = modifier.background(TileBackground))
        return
    }
    AsyncImage(
        model =
            ImageRequest
                .Builder(LocalContext.current)
                .data(url)
                .httpHeaders(
                    NetworkHeaders
                        .Builder()
                        .set("Authorization", authorizationHeader)
                        .build(),
                ).crossfade(false)
                .build(),
        contentDescription = contentDescription,
        contentScale = contentScale,
        modifier = modifier,
    )
}
