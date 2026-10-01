package app.mirror.vault.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.calculateCentroid
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.geometry.lerp
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.util.lerp
import androidx.core.view.WindowCompat
import app.mirror.vault.library.isDeviceOnly
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Sheet
import app.mirror.vault.ui.design.Txt
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.abs
import kotlin.math.roundToInt

/** Window-space bounds of visible grid tiles, used for hero transitions. */
@Stable
class HeroBounds {
    private val bounds = HashMap<String, Rect>()

    fun put(
        assetId: String,
        rect: Rect,
    ) {
        bounds[assetId] = rect
    }

    fun remove(assetId: String) {
        bounds.remove(assetId)
    }

    operator fun get(assetId: String): Rect? = bounds[assetId]
}

data class ViewerActions(
    val thumbUrl: (String) -> String?,
    val previewUrl: (String) -> String?,
    val onFavorite: (AssetTimelineItem, Boolean) -> Unit,
    val onShare: (AssetTimelineItem) -> Unit,
    val onTrash: (AssetTimelineItem) -> Unit,
    val originalUrl: (String) -> String?,
    val onPageChanged: (String) -> Unit,
    val onClose: () -> Unit,
)

private const val MAX_ZOOM = 5f
private const val CONTROLS_AUTO_HIDE_MILLIS = 3_000L
private const val DOUBLE_TAP_ZOOM = 2.5f

@Stable
private class ZoomState {
    var scale by mutableFloatStateOf(1f)
    var offset by mutableStateOf(Offset.Zero)

    val zoomed: Boolean get() = scale > 1.01f

    fun transform(
        zoom: Float,
        pan: Offset,
        centroid: Offset,
        size: Size,
    ) {
        val next = (scale * zoom).coerceIn(0.85f, MAX_ZOOM)
        val center = Offset(size.width / 2, size.height / 2)
        val anchor = centroid - center
        offset = clamp(anchor - (anchor - offset) * (next / scale) + pan, next, size)
        scale = next
    }

    fun clamp(
        value: Offset,
        atScale: Float,
        size: Size,
    ): Offset {
        val maxX = (size.width * (atScale - 1f) / 2f).coerceAtLeast(0f)
        val maxY = (size.height * (atScale - 1f) / 2f).coerceAtLeast(0f)
        return Offset(value.x.coerceIn(-maxX, maxX), value.y.coerceIn(-maxY, maxY))
    }

    suspend fun animateTo(
        targetScale: Float,
        targetOffset: Offset,
    ) {
        val startScale = scale
        val startOffset = offset
        animate(0f, 1f, animationSpec = spring(dampingRatio = 0.85f, stiffness = Spring.StiffnessMediumLow)) { t, _ ->
            scale = lerp(startScale, targetScale, t)
            offset = lerp(startOffset, targetOffset, t)
        }
    }
}

private enum class GestureMode { UNDECIDED, PASS, ZOOM, PAN, DISMISS }

@Composable
@Suppress("LongMethod", "CyclomaticComplexMethod") // One cohesive gesture + transition surface.
fun Viewer(
    items: List<AssetTimelineItem>,
    startId: String,
    heroBounds: HeroBounds,
    actions: ViewerActions,
) {
    val startIndex = remember { items.indexOfFirst { it.assetId == startId }.coerceAtLeast(0) }
    val pager = rememberPagerState(initialPage = startIndex) { items.size }
    val scope = rememberCoroutineScope()
    val density = LocalDensity.current
    val progress = remember { Animatable(0f) }
    val drag = remember { Animatable(Offset.Zero, Offset.VectorConverter) }
    var chrome by remember { mutableStateOf(true) }
    var closing by remember { mutableStateOf(false) }
    var infoFor by remember { mutableStateOf<AssetTimelineItem?>(null) }
    val zoomStates = remember { HashMap<String, ZoomState>() }
    var currentZoomed by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val authorization = LocalAuthHeader.current
    val video = remember { VideoSession(context, authorization) }
    DisposableEffect(video) { onDispose { video.release() } }
    VideoClock(video)
    LaunchedEffect(video.playing, chrome) {
        // Let the picture breathe: controls fade away a few seconds into playback.
        if (video.playing && chrome) {
            delay(CONTROLS_AUTO_HIDE_MILLIS)
            chrome = false
        }
    }

    LightStatusBarIcons()

    LaunchedEffect(Unit) {
        progress.animateTo(1f, spring(dampingRatio = 0.86f, stiffness = 420f))
    }
    LaunchedEffect(items.size) {
        if (items.isEmpty()) actions.onClose()
    }
    LaunchedEffect(pager) {
        snapshotFlow { pager.currentPage }.collect { page ->
            items.getOrNull(page)?.let { item ->
                if (video.assetId != null && video.assetId != item.assetId) video.stop()
                actions.onPageChanged(item.assetId)
            }
        }
    }

    fun close() {
        if (closing) return
        closing = true
        chrome = false
        scope.launch {
            progress.animateTo(0f, spring(dampingRatio = 0.9f, stiffness = 520f))
            actions.onClose()
        }
    }

    BackHandler(enabled = infoFor == null) { close() }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val screen = with(density) { Size(maxWidth.toPx(), maxHeight.toPx()) }
        val dismissDistance = with(density) { 140.dp.toPx() }
        val dragFraction = (abs(drag.value.y) / (screen.height / 2f)).coerceIn(0f, 1f)

        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer { alpha = progress.value * (1f - dragFraction * 0.9f) }
                .background(Color.Black),
        )

        HorizontalPager(
            state = pager,
            key = { items.getOrNull(it)?.assetId ?: it },
            pageSpacing = 18.dp,
            beyondViewportPageCount = 1,
            userScrollEnabled = !currentZoomed && !closing,
            modifier = Modifier.fillMaxSize(),
        ) { page ->
            val item = items.getOrNull(page) ?: return@HorizontalPager
            val zoom = remember(item.assetId) { zoomStates.getOrPut(item.assetId) { ZoomState() } }
            val isCurrent = page == pager.currentPage
            LaunchedEffect(zoom, isCurrent) {
                if (isCurrent) snapshotFlow { zoom.zoomed }.collect { currentZoomed = it }
            }
            val knownAspect = item.preview != null || item.thumbnail != null
            val fitted = if (knownAspect) fitRect(screen, item.aspect) else Rect(Offset.Zero, screen)
            val dragScale = 1f - dragFraction * 0.35f
            val dragged =
                if (isCurrent) {
                    scaleAround(fitted, dragScale).translate(drag.value)
                } else {
                    fitted
                }
            val origin = if (isCurrent) heroBounds[item.assetId] else null
            val t = progress.value
            val rect = if (origin != null) lerp(origin, dragged, t) else dragged
            val fadeOnly = isCurrent && origin == null
            val corner = lerp(4f, 0f, t.coerceIn(0f, 1f))

            Box(
                Modifier
                    .fillMaxSize()
                    .pointerInput(item.assetId) {
                        detectTapGestures(
                            onTap = { chrome = !chrome },
                            onDoubleTap = { tap ->
                                if (!item.isVideo) {
                                    scope.launch {
                                        if (zoom.zoomed) {
                                            zoom.animateTo(1f, Offset.Zero)
                                        } else {
                                            val anchor = tap - Offset(size.width / 2f, size.height / 2f)
                                            val target =
                                                zoom.clamp(
                                                    anchor * (1f - DOUBLE_TAP_ZOOM),
                                                    DOUBLE_TAP_ZOOM,
                                                    Size(size.width.toFloat(), size.height.toFloat()),
                                                )
                                            zoom.animateTo(DOUBLE_TAP_ZOOM, target)
                                        }
                                    }
                                }
                            },
                        )
                    }.pointerInput(item.assetId) {
                        val slop = viewConfiguration.touchSlop
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false)
                            var mode = GestureMode.UNDECIDED
                            var travel = Offset.Zero
                            val velocity = VelocityTracker()
                            val area = Size(size.width.toFloat(), size.height.toFloat())
                            do {
                                val event = awaitPointerEvent()
                                val pressed = event.changes.filter { it.pressed }
                                if (pressed.size >= 2 && mode != GestureMode.DISMISS && !item.isVideo) {
                                    mode = GestureMode.ZOOM
                                    zoom.transform(
                                        event.calculateZoom(),
                                        event.calculatePan(),
                                        event.calculateCentroid(),
                                        area,
                                    )
                                    event.changes.forEach { it.consume() }
                                } else if (pressed.size == 1) {
                                    val change = pressed.first()
                                    val delta = change.positionChange()
                                    velocity.addPosition(change.uptimeMillis, change.position)
                                    when (mode) {
                                        GestureMode.UNDECIDED -> {
                                            if (zoom.zoomed) {
                                                mode = GestureMode.PAN
                                            } else {
                                                travel += delta
                                                if (travel.getDistance() > slop) {
                                                    mode =
                                                        if (abs(travel.y) > abs(travel.x) * 1.3f) {
                                                            GestureMode.DISMISS
                                                        } else {
                                                            GestureMode.PASS
                                                        }
                                                }
                                            }
                                        }
                                        GestureMode.PAN -> {
                                            zoom.offset = zoom.clamp(zoom.offset + delta, zoom.scale, area)
                                            change.consume()
                                        }
                                        GestureMode.DISMISS -> {
                                            scope.launch { drag.snapTo(drag.value + delta) }
                                            chrome = false
                                            change.consume()
                                        }
                                        GestureMode.ZOOM, GestureMode.PASS -> Unit
                                    }
                                }
                            } while (event.changes.any { it.pressed })
                            when (mode) {
                                GestureMode.DISMISS -> {
                                    val flick = velocity.calculateVelocity().y
                                    if (abs(drag.value.y) > dismissDistance || abs(flick) > 2200f) {
                                        close()
                                    } else {
                                        chrome = true
                                        scope.launch { drag.animateTo(Offset.Zero, spring(dampingRatio = 0.75f)) }
                                    }
                                }
                                GestureMode.ZOOM ->
                                    if (zoom.scale < 1f) {
                                        scope.launch { zoom.animateTo(1f, Offset.Zero) }
                                    }
                                else -> Unit
                            }
                        }
                    },
            ) {
                Box(
                    Modifier
                        .offset { IntOffset(rect.left.roundToInt(), rect.top.roundToInt()) }
                        .size(with(density) { rect.width.toDp() }, with(density) { rect.height.toDp() })
                        .graphicsLayer {
                            scaleX = zoom.scale
                            scaleY = zoom.scale
                            translationX = zoom.offset.x
                            translationY = zoom.offset.y
                            if (fadeOnly) {
                                alpha = t.coerceIn(0f, 1f)
                                val s = lerp(0.9f, 1f, t)
                                scaleX *= s
                                scaleY *= s
                            }
                        }.clip(RoundedCornerShape(corner.dp)),
                ) {
                    val scale = if (knownAspect) ContentScale.Crop else ContentScale.Fit
                    if (item.localUri != null) {
                        LocalThumbnail(item.localUri, Modifier.fillMaxSize(), contentScale = scale)
                        LocalImage(item.localUri, Modifier.fillMaxSize(), scale, item.originalFilename)
                    } else {
                        RemoteImage(actions.thumbUrl(item.assetId), Modifier.fillMaxSize(), scale, crossfadeMillis = 0)
                        RemoteImage(actions.previewUrl(item.assetId), Modifier.fillMaxSize(), scale, item.originalFilename)
                    }
                    if (item.isVideo && isCurrent) {
                        if (video.assetId == item.assetId) {
                            val started = video.playing || video.positionMillis > 0 || video.ended
                            // Sized from the video itself, so it letterboxes correctly even
                            // when the item's own dimensions aren't known (e.g. device-only).
                            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                                val shape = video.aspect.takeIf { it > 0f } ?: item.aspect
                                VideoSurface(
                                    video,
                                    Modifier.aspectRatio(shape).graphicsLayer { alpha = if (started) 1f else 0f },
                                )
                            }
                        }
                        VideoPlayOverlay(video, item.assetId, item.localUri ?: actions.originalUrl(item.assetId), Modifier.fillMaxSize())
                    } else if (item.isVideo) {
                        Box(
                            Modifier
                                .align(Alignment.Center)
                                .size(68.dp)
                                .clip(RoundedCornerShape(34.dp))
                                .background(Color.Black.copy(alpha = 0.45f)),
                            contentAlignment = Alignment.Center,
                        ) {
                            Glyph(Glyphs.Play, tint = Color.White, size = 28.dp)
                        }
                    }
                }
            }
        }

        val current = items.getOrNull(pager.currentPage)
        AnimatedVisibility(
            visible = chrome && current != null && progress.value > 0.6f,
            enter = fadeIn(),
            exit = fadeOut(),
        ) {
            if (current != null) {
                ViewerChrome(
                    item = current,
                    video = video,
                    originalUrl = current.localUri ?: actions.originalUrl(current.assetId),
                    position = "${pager.currentPage + 1} / ${items.size}",
                    onClose = ::close,
                    onFavorite = { actions.onFavorite(current, !current.isFavorite) },
                    onShare = { actions.onShare(current) },
                    onInfo = { infoFor = current },
                    onTrash = { actions.onTrash(current) },
                )
            }
        }

        Sheet(visible = infoFor != null, onDismiss = { infoFor = null }) {
            infoFor?.let { InfoSheetContent(it) }
        }
    }
}

@Composable
private fun ViewerChrome(
    item: AssetTimelineItem,
    video: VideoSession,
    originalUrl: String?,
    position: String,
    onClose: () -> Unit,
    onFavorite: () -> Unit,
    onShare: () -> Unit,
    onInfo: () -> Unit,
    onTrash: () -> Unit,
) {
    val glass = Color.White.copy(alpha = 0.12f)
    Box(Modifier.fillMaxSize()) {
        Row(
            Modifier
                .fillMaxWidth()
                .background(Brush.verticalGradient(listOf(Color.Black.copy(alpha = 0.55f), Color.Transparent)))
                .statusBarsPadding()
                .padding(horizontal = 14.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RoundGlyphButton(Glyphs.Back, onClose, tint = Color.White, background = glass, contentDescription = "Close")
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Txt(dayOrStamp(item), style = Mirror.type.bodyStrong, color = Color.White, maxLines = 1)
                Txt(
                    listOf(item.timeLabel(), position, item.backupState.statusLabel().orEmpty())
                        .filter(String::isNotEmpty)
                        .joinToString("  ·  "),
                    style = Mirror.type.caption,
                    color = Color.White.copy(alpha = 0.7f),
                )
            }
        }
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.6f))))
                .navigationBarsPadding()
                .padding(top = 24.dp, bottom = 18.dp),
        ) {
            if (item.isVideo) {
                VideoControls(video, item.assetId, originalUrl)
                Spacer(Modifier.height(14.dp))
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 28.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                val favorite = item.isFavorite
                val remoteActions = !item.isDeviceOnly
                RoundGlyphButton(
                    if (favorite) Glyphs.HeartFilled else Glyphs.Heart,
                    onFavorite,
                    enabled = remoteActions,
                    tint = if (favorite) Mirror.colors.accent else Color.White,
                    background = glass,
                    size = 52.dp,
                    contentDescription = if (favorite) "Unfavorite" else "Favorite",
                )
                RoundGlyphButton(
                    Glyphs.Share,
                    onShare,
                    enabled = remoteActions,
                    tint = Color.White,
                    background = glass,
                    size = 52.dp,
                    contentDescription = "Share",
                )
                RoundGlyphButton(
                    Glyphs.Info,
                    onInfo,
                    tint = Color.White,
                    background = glass,
                    size = 52.dp,
                    contentDescription = "Details",
                )
                RoundGlyphButton(
                    Glyphs.Trash,
                    onTrash,
                    enabled = remoteActions,
                    tint = Color.White,
                    background = glass,
                    size = 52.dp,
                    contentDescription = "Move to trash",
                )
            }
        }
    }
}

private fun dayOrStamp(item: AssetTimelineItem): String = item.localDate()?.let { dayLabel(it) } ?: "Photo"

@Composable
private fun InfoSheetContent(item: AssetTimelineItem) {
    val colors = Mirror.colors
    Txt(item.stampLabel(), style = Mirror.type.title)
    Spacer(Modifier.height(4.dp))
    Txt(item.timeLabel(), style = Mirror.type.body, color = colors.inkMuted)
    Spacer(Modifier.height(22.dp))
    InfoLine("File", item.originalFilename ?: "Unnamed")
    InfoLine("Type", item.mediaType)
    InfoLine(
        "Stored",
        when {
            item.isDeviceOnly -> "This device only"
            item.localUri != null -> "This device and your vault"
            else -> "Your vault"
        },
    )
    if (item.sizeBytes > 0) InfoLine("Size", humanBytes(item.sizeBytes))
    item.preview?.let { InfoLine("Preview", "${it.width} × ${it.height}  ·  ${it.format.uppercase()}") }
    if (item.originalBlake3.isNotEmpty()) {
        Spacer(Modifier.height(16.dp))
        Row(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(16.dp))
                .background(colors.positive.copy(alpha = 0.12f))
                .padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Glyph(Glyphs.Shield, tint = colors.positive, size = 22.dp)
            Spacer(Modifier.width(12.dp))
            Column {
                Txt("Original verified on your server", style = Mirror.type.label, color = colors.positive)
                Txt(
                    "BLAKE3 ${item.originalBlake3.take(16)}…",
                    style = Mirror.type.caption,
                    color = colors.inkMuted,
                )
            }
        }
    }
    Spacer(Modifier.height(10.dp))
}

@Composable
private fun InfoLine(
    label: String,
    value: String,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(vertical = 9.dp),
    ) {
        Txt(label, style = Mirror.type.body, color = Mirror.colors.inkMuted, modifier = Modifier.width(92.dp))
        Txt(value, style = Mirror.type.bodyStrong, maxLines = 2)
    }
}

/** Forces light status-bar icons while the black viewer is on screen. */
@Composable
private fun LightStatusBarIcons() {
    val view = LocalView.current
    val dark = Mirror.colors.dark
    DisposableEffect(view, dark) {
        val window = (view.context as? android.app.Activity)?.window
        val controller = window?.let { WindowCompat.getInsetsController(it, view) }
        controller?.isAppearanceLightStatusBars = false
        controller?.isAppearanceLightNavigationBars = false
        onDispose {
            controller?.isAppearanceLightStatusBars = !dark
            controller?.isAppearanceLightNavigationBars = !dark
        }
    }
}

private fun fitRect(
    screen: Size,
    aspect: Float,
): Rect {
    val screenAspect = screen.width / screen.height
    return if (aspect > screenAspect) {
        val height = screen.width / aspect
        Rect(Offset(0f, (screen.height - height) / 2f), Size(screen.width, height))
    } else {
        val width = screen.height * aspect
        Rect(Offset((screen.width - width) / 2f, 0f), Size(width, screen.height))
    }
}

private fun scaleAround(
    rect: Rect,
    scale: Float,
): Rect {
    val size = Size(rect.width * scale, rect.height * scale)
    return Rect(Offset(rect.center.x - size.width / 2f, rect.center.y - size.height / 2f), size)
}
