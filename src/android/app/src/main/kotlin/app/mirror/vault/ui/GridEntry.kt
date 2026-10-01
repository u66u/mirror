package app.mirror.vault.ui

import app.mirror.vault.network.AssetTimelineItem
import java.time.LocalDate

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

/**
 * Title for the sticky header: the day (or "Today") of the section the first
 * visible row belongs to, found by scanning back to the nearest day caption.
 */
fun sectionTitleAt(
    entries: List<GridEntry>,
    index: Int,
): String? {
    val days = entries.withIndex().filter { it.value is GridEntry.Day }
    val at = if (entries.isEmpty()) -1 else index.coerceIn(0, entries.lastIndex)
    val day = days.lastOrNull { it.index <= at } ?: days.firstOrNull()
    return (day?.value as? GridEntry.Day)?.label
}
