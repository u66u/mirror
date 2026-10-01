package app.mirror.vault.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.FaceItem
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.Txt
import coil3.compose.AsyncImage
import coil3.compose.AsyncImagePainter
import coil3.network.NetworkHeaders
import coil3.network.httpHeaders
import coil3.request.ImageRequest
import coil3.request.crossfade
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.TextStyle
import java.time.temporal.ChronoUnit
import java.util.Locale

/** Bearer header for derivative requests; null while signed out. */
val LocalAuthHeader = staticCompositionLocalOf<String?> { null }

@Composable
fun RemoteImage(
    url: String?,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Crop,
    contentDescription: String? = null,
    crossfadeMillis: Int = 220,
    background: Color = Color.Transparent,
    fallbackLabel: String? = null,
) {
    val header = LocalAuthHeader.current
    if (url == null || header == null) {
        Box(modifier.background(background))
        return
    }
    var failed by remember(url) { mutableStateOf(false) }
    val context = LocalContext.current
    val request =
        remember(url, header) {
            ImageRequest
                .Builder(context)
                .data(url)
                .memoryCacheKey(url)
                .httpHeaders(NetworkHeaders.Builder().set("Authorization", header).build())
                .crossfade(crossfadeMillis)
                .build()
        }
    Box(modifier.background(background)) {
        AsyncImage(
            model = request,
            contentDescription = contentDescription,
            contentScale = contentScale,
            onState = { failed = it is AsyncImagePainter.State.Error },
            modifier = Modifier.fillMaxSize(),
        )
        // A blank tile says nothing; name the file so the user knows what they're looking at.
        if (failed && fallbackLabel != null) FailedImage(fallbackLabel)
    }
}

@Composable
private fun FailedImage(label: String) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Glyph(Glyphs.Photos, tint = Mirror.colors.inkFaint, size = 20.dp)
        Spacer(Modifier.height(4.dp))
        Txt(
            label,
            style = Mirror.type.caption.copy(textAlign = TextAlign.Center),
            color = Mirror.colors.inkMuted,
            maxLines = 2,
        )
    }
}

val AssetTimelineItem.isVideo: Boolean get() = mediaType.startsWith("video/")

val AssetTimelineItem.isFavorite: Boolean get() = favoriteAt != null

/** Aspect ratio of the best known derivative, falling back to 4:3. */
val AssetTimelineItem.aspect: Float
    get() {
        val derivative = preview ?: thumbnail ?: return DEFAULT_ASPECT
        return if (derivative.height > 0) derivative.width.toFloat() / derivative.height else DEFAULT_ASPECT
    }

private const val DEFAULT_ASPECT = 4f / 3f

fun FaceItem.asAsset(): AssetTimelineItem =
    AssetTimelineItem(
        assetId = assetId,
        createdAt = assetCreatedAt,
        favoriteAt = null,
        originalBlake3 = "",
        mediaType = mediaType,
        sizeBytes = 0,
        originalFilename = null,
        thumbnail = null,
        preview = null,
    )

private val zone: ZoneId get() = ZoneId.systemDefault()

fun AssetTimelineItem.localDateTime() =
    runCatching { OffsetDateTime.parse(createdAt).atZoneSameInstant(zone).toLocalDateTime() }.getOrNull()

fun AssetTimelineItem.localDate(): LocalDate? = localDateTime()?.toLocalDate()

private fun dayMonth() = DateTimeFormatter.ofPattern("d MMMM", Locale.getDefault())

private fun dayMonthYear() = DateTimeFormatter.ofPattern("d MMMM yyyy", Locale.getDefault())

private fun monthYear() = DateTimeFormatter.ofPattern("MMMM yyyy", Locale.getDefault())

private fun fullStamp() = DateTimeFormatter.ofPattern("EEEE, d MMMM yyyy", Locale.getDefault())

private fun timeOnly() = DateTimeFormatter.ofPattern("HH:mm", Locale.getDefault())

fun dayLabel(
    date: LocalDate,
    today: LocalDate = LocalDate.now(),
): String {
    val days = ChronoUnit.DAYS.between(date, today)
    return when {
        days == 0L -> "Today"
        days == 1L -> "Yesterday"
        days in 2..6 -> date.dayOfWeek.getDisplayName(TextStyle.FULL, Locale.getDefault())
        date.year == today.year -> date.format(dayMonth())
        else -> date.format(dayMonthYear())
    }
}

fun monthLabel(date: LocalDate): String = date.format(monthYear())

fun AssetTimelineItem.stampLabel(): String = localDateTime()?.format(fullStamp()) ?: createdAt

fun AssetTimelineItem.timeLabel(): String = localDateTime()?.format(timeOnly()) ?: ""

fun humanBytes(bytes: Long): String {
    if (bytes <= 0) return "—"
    val units = listOf("B", "KB", "MB", "GB", "TB")
    var value = bytes.toDouble()
    var unit = 0
    while (value >= KILO && unit < units.lastIndex) {
        value /= KILO
        unit++
    }
    return if (unit == 0) "$bytes B" else String.format(Locale.getDefault(), "%.1f %s", value, units[unit])
}

private const val KILO = 1024.0

/**
 * Space scrolling content must leave at the bottom so its last row clears the
 * floating tab bar. Measured at runtime (pill + gesture/3-button inset + font
 * scale), with a sensible default until the first measurement arrives.
 */
val LocalBottomInset = compositionLocalOf { DEFAULT_BOTTOM_INSET }

private val DEFAULT_BOTTOM_INSET = 132.dp
