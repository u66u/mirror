package app.mirror.vault.ui

import android.content.Context
import android.view.TextureView
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.VideoSize
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.Spinner
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.tappable
import kotlinx.coroutines.delay
import okhttp3.OkHttpClient
import java.util.Locale
import kotlin.math.roundToInt

private const val POSITION_POLL_MILLIS = 200L

/**
 * One ExoPlayer shared by the whole viewer: only the visible page ever plays,
 * so a single instance keeps memory and audio focus simple. Requests carry the
 * device token and use HTTP range reads, so seeking never downloads the file.
 */
@Stable
@androidx.annotation.OptIn(UnstableApi::class) // media3 data-source factories are still marked unstable.
class VideoSession(
    context: Context,
    authorization: String?,
) {
    private val client = OkHttpClient()
    private val player: ExoPlayer =
        ExoPlayer
            .Builder(context)
            .setMediaSourceFactory(
                // Scheme-aware: vault videos stream over HTTP with the device token,
                // device videos open straight from their content URI.
                DefaultMediaSourceFactory(
                    DefaultDataSource.Factory(
                        context,
                        OkHttpDataSource
                            .Factory(client)
                            .setDefaultRequestProperties(
                                authorization?.let { mapOf("Authorization" to it) }.orEmpty(),
                            ),
                    ),
                ),
            ).setAudioAttributes(
                AudioAttributes
                    .Builder()
                    .setUsage(C.USAGE_MEDIA)
                    .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE)
                    .build(),
                true,
            ).setHandleAudioBecomingNoisy(true)
            .build()

    var assetId by mutableStateOf<String?>(null)
        private set
    var playing by mutableStateOf(false)
        private set
    var buffering by mutableStateOf(false)
        private set
    var ended by mutableStateOf(false)
        private set
    var failed by mutableStateOf(false)
        private set
    var positionMillis by mutableLongStateOf(0L)
        private set
    var durationMillis by mutableLongStateOf(0L)
        private set
    var progressFraction by mutableFloatStateOf(0f)
        private set

    /** Display aspect ratio of the loaded video, or 0 until the player knows it. */
    var aspect by mutableFloatStateOf(0f)
        private set

    init {
        player.addListener(
            object : Player.Listener {
                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    playing = isPlaying
                }

                override fun onPlaybackStateChanged(state: Int) {
                    buffering = state == Player.STATE_BUFFERING
                    ended = state == Player.STATE_ENDED
                    if (state == Player.STATE_READY) {
                        durationMillis = player.duration.coerceAtLeast(0L)
                    }
                    sync()
                }

                override fun onVideoSizeChanged(size: VideoSize) {
                    if (size.width > 0 && size.height > 0) {
                        aspect = size.width * size.pixelWidthHeightRatio / size.height
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    failed = true
                    playing = false
                    buffering = false
                }
            },
        )
    }

    fun attach(view: TextureView) {
        player.setVideoTextureView(view)
    }

    fun detach(view: TextureView) {
        player.clearVideoTextureView(view)
    }

    /** Starts playback of [id], or resumes it when it is already loaded. */
    fun play(
        id: String,
        url: String,
    ) {
        if (assetId != id) {
            stop()
            assetId = id
            failed = false
            player.setMediaItem(MediaItem.fromUri(url))
            player.prepare()
        } else if (ended) {
            player.seekTo(0)
        }
        failed = false
        player.play()
    }

    fun pause() = player.pause()

    fun togglePlayback(
        id: String,
        url: String,
    ) {
        if (assetId == id && playing) pause() else play(id, url)
    }

    fun seekToFraction(fraction: Float) {
        val duration = player.duration
        if (duration > 0) {
            val target = (duration * fraction.coerceIn(0f, 1f)).toLong()
            player.seekTo(target)
            positionMillis = target
            progressFraction = fraction.coerceIn(0f, 1f)
        }
    }

    fun stop() {
        player.stop()
        player.clearMediaItems()
        assetId = null
        playing = false
        buffering = false
        ended = false
        failed = false
        positionMillis = 0
        durationMillis = 0
        progressFraction = 0f
        aspect = 0f
    }

    fun sync() {
        positionMillis = player.currentPosition.coerceAtLeast(0L)
        val duration = player.duration
        if (duration > 0) {
            durationMillis = duration
            progressFraction = (positionMillis.toFloat() / duration).coerceIn(0f, 1f)
        }
    }

    fun release() {
        player.release()
    }
}

/** Video surface that fills its parent; the parent is already sized to the video's aspect ratio. */
@Composable
fun VideoSurface(
    session: VideoSession,
    modifier: Modifier = Modifier,
) {
    AndroidView(
        factory = { context ->
            TextureView(context).also { view ->
                view.keepScreenOn = true
                session.attach(view)
            }
        },
        onRelease = { session.detach(it) },
        modifier = modifier,
    )
}

/** Polls position while playing so the scrubber and clock follow along. */
@Composable
fun VideoClock(session: VideoSession) {
    LaunchedEffect(session, session.playing, session.assetId) {
        while (session.assetId != null) {
            session.sync()
            delay(POSITION_POLL_MILLIS)
        }
    }
}

@Composable
fun VideoPlayOverlay(
    session: VideoSession,
    assetId: String,
    url: String?,
    modifier: Modifier = Modifier,
) {
    val active = session.assetId == assetId
    val showButton = !(active && session.playing) && !(active && session.buffering)
    Box(modifier, contentAlignment = Alignment.Center) {
        when {
            active && session.failed ->
                Row(
                    Modifier
                        .clip(CircleShape)
                        .background(Color.Black.copy(alpha = 0.6f))
                        .padding(horizontal = 18.dp, vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Glyph(Glyphs.Alert, tint = Color.White, size = 18.dp)
                    Spacer(Modifier.width(8.dp))
                    Txt("Couldn't play this video", style = Mirror.type.label, color = Color.White)
                }
            active && session.buffering && !session.playing -> Spinner(color = Color.White, size = 34.dp)
            showButton ->
                Box(
                    Modifier
                        .size(76.dp)
                        .clip(CircleShape)
                        .background(Color.Black.copy(alpha = 0.5f))
                        .tappable(pressedScale = 0.9f) { url?.let { session.play(assetId, it) } },
                    contentAlignment = Alignment.Center,
                ) {
                    Glyph(Glyphs.Play, tint = Color.White, size = 32.dp)
                }
        }
    }
}

@Composable
fun VideoControls(
    session: VideoSession,
    assetId: String,
    url: String?,
    modifier: Modifier = Modifier,
) {
    val active = session.assetId == assetId
    var dragFraction by remember { mutableStateOf<Float?>(null) }
    val shown = dragFraction ?: if (active) session.progressFraction else 0f
    val duration = if (active) session.durationMillis else 0L
    val glass = Color.White.copy(alpha = 0.14f)
    Row(
        modifier
            .fillMaxWidth()
            .padding(horizontal = 18.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(44.dp)
                .clip(CircleShape)
                .background(glass)
                .tappable(pressedScale = 0.9f) { url?.let { session.togglePlayback(assetId, it) } },
            contentAlignment = Alignment.Center,
        ) {
            Glyph(
                if (active && session.playing) Glyphs.Pause else Glyphs.Play,
                tint = Color.White,
                size = 20.dp,
            )
        }
        Spacer(Modifier.width(12.dp))
        Txt(clock((shown * duration).toLong()), style = Mirror.type.caption, color = Color.White)
        Spacer(Modifier.width(10.dp))
        Scrubber(
            fraction = shown,
            enabled = active && duration > 0,
            onDrag = { dragFraction = it },
            onCommit = {
                dragFraction?.let(session::seekToFraction)
                dragFraction = null
            },
            modifier = Modifier.weight(1f),
        )
        Spacer(Modifier.width(10.dp))
        Txt(clock(duration), style = Mirror.type.caption, color = Color.White.copy(alpha = 0.7f))
    }
}

@Composable
private fun Scrubber(
    fraction: Float,
    enabled: Boolean,
    onDrag: (Float) -> Unit,
    onCommit: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var width by remember { mutableStateOf(0) }
    val accent = Mirror.colors.accent
    Box(
        modifier
            .height(36.dp)
            .onSizeChanged { width = it.width }
            .pointerInput(enabled) {
                if (!enabled) return@pointerInput
                awaitEachGesture {
                    val down = awaitFirstDown(requireUnconsumed = false)
                    down.consume()
                    onDrag((down.position.x / size.width).coerceIn(0f, 1f))
                    var change = down
                    while (change.pressed) {
                        val event = awaitPointerEvent()
                        change = event.changes.firstOrNull { it.id == down.id } ?: break
                        change.consume()
                        onDrag((change.position.x / size.width).coerceIn(0f, 1f))
                    }
                    onCommit()
                }
            },
        contentAlignment = Alignment.CenterStart,
    ) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(4.dp)
                .clip(CircleShape)
                .background(Color.White.copy(alpha = 0.25f)),
        )
        Box(
            Modifier
                .width(with(androidx.compose.ui.platform.LocalDensity.current) { (width * fraction).toDp() })
                .height(4.dp)
                .clip(CircleShape)
                .background(accent),
        )
        Box(
            Modifier
                .offset { IntOffset((width * fraction).roundToInt() - 7.dp.roundToPx(), 0) }
                .size(14.dp)
                .clip(CircleShape)
                .background(Color.White),
        )
    }
}

private fun clock(millis: Long): String {
    val totalSeconds = (millis / 1000).coerceAtLeast(0)
    val seconds = totalSeconds % 60
    val minutes = (totalSeconds / 60) % 60
    val hours = totalSeconds / 3600
    return if (hours > 0) {
        String.format(Locale.ROOT, "%d:%02d:%02d", hours, minutes, seconds)
    } else {
        String.format(Locale.ROOT, "%d:%02d", minutes, seconds)
    }
}
