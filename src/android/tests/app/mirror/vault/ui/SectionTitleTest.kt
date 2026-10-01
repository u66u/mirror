package app.mirror.vault.ui

import app.mirror.vault.network.AssetTimelineItem
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SectionTitleTest {
    private fun tile(
        id: String,
        index: Int,
    ) = GridEntry.Tile(
        AssetTimelineItem(id, "2026-10-01T10:00:00Z", null, "", "image/jpeg", 1, null, null, null),
        index,
    )

    private val entries =
        listOf(
            GridEntry.Day("Today", 2, "2026-10-01"),
            tile("a", 0),
            tile("b", 1),
            GridEntry.Month("September 2026"),
            GridEntry.Day("30 September", 1, "2026-09-30"),
            tile("c", 2),
        )

    @Test
    fun titleIsTheDayOfTheSectionTheRowBelongsTo() {
        assertEquals("Today", sectionTitleAt(entries, 0))
        assertEquals("Today", sectionTitleAt(entries, 2))
        assertEquals("30 September", sectionTitleAt(entries, 5))
    }

    @Test
    fun aMonthBannerStillReadsAsTheDayItPrecedes() {
        // Banner at index 3 belongs visually to the previous section until its day caption.
        assertEquals("Today", sectionTitleAt(entries, 3))
    }

    @Test
    fun outOfRangeIndexesClampAndEmptyIsNull() {
        assertEquals("30 September", sectionTitleAt(entries, 99))
        assertEquals("Today", sectionTitleAt(entries, -4))
        assertNull(sectionTitleAt(emptyList(), 0))
    }
}
