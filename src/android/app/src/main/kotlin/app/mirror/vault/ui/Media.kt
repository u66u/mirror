package app.mirror.vault.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.FaceItem
import coil3.compose.AsyncImage
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
) {
    val header = LocalAuthHeader.current
    if (url == null || header == null) {
        Box(modifier.background(background))
        return
    }
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
            modifier = Modifier.fillMaxSize(),
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

/** Timeline rows: a month banner, a day caption, or a run of tiles. */
sealed interface GridEntry {
    val key: String

    data class Month(
        val label: String,
    ) : GridEntry {
        override val key: String get() = "month:$label"
    }

    data class Day(
        val label: String,
        val count: Int,
        val dateKey: String,
    ) : GridEntry {
        override val key: String get() = "day:$dateKey"
    }

    data class Tile(
        val item: AssetTimelineItem,
        val index: Int,
    ) : GridEntry {
        override val key: String get() = item.assetId
    }
}

fun buildGridEntries(items: List<AssetTimelineItem>): List<GridEntry> {
    val entries = ArrayList<GridEntry>(items.size + items.size / 4)
    val today = LocalDate.now()
    var lastMonth: String? = null
    var index = 0
    items
        .groupBy { it.localDate() }
        .forEach { (date, group) ->
            if (date != null) {
                val month = monthLabel(date)
                if (month != lastMonth && date.year * 12 + date.monthValue < today.year * 12 + today.monthValue) {
                    entries += GridEntry.Month(month)
                }
                lastMonth = month
                entries += GridEntry.Day(dayLabel(date, today), group.size, date.toString())
            } else {
                entries += GridEntry.Day("Undated", group.size, "undated")
            }
            group.forEach { entries += GridEntry.Tile(it, index++) }
        }
    return entries
}
