@file:Suppress("MaxLineLength", "ktlint:standard:max-line-length") // SVG path data reads best unwrapped.

package app.mirror.vault.ui.design

import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/**
 * Hand-drawn 24px line glyphs. Kept in one place so stroke weight, caps and
 * proportions stay consistent across the whole app.
 */
object Glyphs {
    val Photos =
        line(
            "photos",
            "M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v11a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 17.5z M4 16l4.5-4.5 4 4 2.5-2.5L20 18 M15.5 7a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3",
        )
    val Search = line("search", "M11 4.5a6.5 6.5 0 1 0 0 13a6.5 6.5 0 1 0 0-13 M20 20l-4.4-4.4")
    val People =
        line(
            "people",
            "M9 5a3.5 3.5 0 1 0 0 7a3.5 3.5 0 1 0 0-7 M3 19.5c.8-3.2 3.2-5 6-5s5.2 1.8 6 5 M16 5.2a3.3 3.3 0 0 1 0 6.6 M17.5 14.6c1.9.6 3.1 2.3 3.5 4.9",
        )
    val Vault = line("vault", "M7 18.5h10.5a4 4 0 0 0 .6-7.95A6 6 0 0 0 6.4 9.6A4.5 4.5 0 0 0 7 18.5z M12 15.5v-6 M9.5 12l2.5-2.5 2.5 2.5")
    val Heart = line("heart", "M12 19.5s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 6.7a4.3 4.3 0 0 1 7.5 2.6C19.5 14.9 12 19.5 12 19.5z")
    val HeartFilled =
        filled("heart-filled", "M12 19.5s-7.5-4.6-7.5-10.2A4.3 4.3 0 0 1 12 6.7a4.3 4.3 0 0 1 7.5 2.6C19.5 14.9 12 19.5 12 19.5z")
    val Trash =
        line("trash", "M4.5 7h15 M9.5 7V4.8h5V7 M6.5 7l.9 12.2a1.5 1.5 0 0 0 1.5 1.3h6.2a1.5 1.5 0 0 0 1.5-1.3L17.5 7 M10 11v6 M14 11v6")
    val Share = line("share", "M12 14.5V3.5 M8 7.5l4-4 4 4 M5.5 11.5v7A2 2 0 0 0 7.5 20.5h9a2 2 0 0 0 2-2v-7")
    val Close = line("close", "M6 6l12 12 M18 6L6 18")
    val Back = line("back", "M14.5 5l-7 7 7 7")
    val Forward = line("forward", "M9.5 5l7 7-7 7")
    val Check = line("check", "M5 12.5l4.5 4.5L19 7.5")
    val Info = line("info", "M12 3.5a8.5 8.5 0 1 0 0 17a8.5 8.5 0 1 0 0-17 M12 11v5.5 M12 7.9v.1")
    val Wifi = line("wifi", "M2.5 9a14 14 0 0 1 19 0 M5.5 12.5a9.5 9.5 0 0 1 13 0 M8.7 15.8a5 5 0 0 1 6.6 0 M12 19.2v.1")
    val Folder = line("folder", "M3.5 7.5a2 2 0 0 1 2-2h4l2 2.5h7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z")
    val Play = filled("play", "M8.5 5.5v13l10-6.5z")
    val Pause = filled("pause", "M8 5.5h3v13H8z M13 5.5h3v13h-3z")
    val Settings = line("settings", "M4 7h9 M17 7h3 M4 17h3 M11 17h9 M15 4.5v5 M9 14.5v5")
    val Moon = line("moon", "M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z")
    val Eye =
        line("eye", "M2.5 12C4 8.5 7.5 6 12 6s8 2.5 9.5 6c-1.5 3.5-5 6-9.5 6s-8-2.5-9.5-6z M12 9.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 1 0 0-5")
    val Battery =
        line(
            "battery",
            "M3.5 8.5h14a1.5 1.5 0 0 1 1.5 1.5v4a1.5 1.5 0 0 1-1.5 1.5h-14A1.5 1.5 0 0 1 2 14v-4a1.5 1.5 0 0 1 1.5-1.5z M21 11v2 M9 9.5l-1.5 3h3L9 15.5",
        )
    val Cache =
        line(
            "cache",
            "M5 6.5C5 5.1 8 4 12 4s7 1.1 7 2.5S16 9 12 9 5 7.9 5 6.5z M5 6.5v5C5 12.9 8 14 12 14s7-1.1 7-2.5v-5 M5 11.5v5C5 17.9 8 19 12 19s7-1.1 7-2.5v-5",
        )
    val Restore = line("restore", "M4.5 12a7.5 7.5 0 1 0 2.2-5.3 M4.5 4.5v4h4")
    val Sparkle = line("sparkle", "M11 3.5l1.8 5.2 5.2 1.8-5.2 1.8L11 17.5l-1.8-5.2L4 10.5l5.2-1.8z M18.5 15v5 M16 17.5h5")
    val Server = line("server", "M4.5 5h15v5.5h-15z M4.5 13.5h15V19h-15z M8 7.75h.01 M8 16.25h.01")
    val Leave = line("leave", "M14 4.5h3.5A1.5 1.5 0 0 1 19 6v12a1.5 1.5 0 0 1-1.5 1.5H14 M10 8l-4 4 4 4 M6 12h9.5")
    val Refresh = line("refresh", "M19.5 12a7.5 7.5 0 1 1-2.2-5.3 M19.5 4.5v4h-4")
    val Hide =
        line(
            "hide",
            "M3.5 3.5l17 17 M10.6 6.1A9 9 0 0 1 12 6c5 0 8.5 4.5 9.5 6-.5.8-1.6 2.2-3.1 3.5 M6.6 7.6C4.6 8.9 3.2 10.8 2.5 12c1 1.5 4.5 6 9.5 6 1.6 0 3-.4 4.3-1.1 M9.9 9.9a3 3 0 0 0 4.2 4.2",
        )
    val Pencil = line("pencil", "M14.5 5.5l4 4 M4.5 19.5l1-4.5L15.8 4.7a1.5 1.5 0 0 1 2.1 0l1.4 1.4a1.5 1.5 0 0 1 0 2.1L9 18.5z")
    val Link = line("link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1 M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1")
    val Lock = line("lock", "M6 11h12v9H6z M8.5 11V8a3.5 3.5 0 0 1 7 0v3")
    val Shield = line("shield", "M12 3.5l7 2.5v5.5c0 4.5-3 7.8-7 9-4-1.2-7-4.5-7-9V6z M9 12l2.2 2.2L15.5 10")
    val Grid = line("grid", "M4.5 4.5h6v6h-6z M13.5 4.5h6v6h-6z M4.5 13.5h6v6h-6z M13.5 13.5h6v6h-6z")
    val Alert = line("alert", "M12 4l8.5 15h-17z M12 10v4 M12 16.8v.1")
}

private fun line(
    name: String,
    path: String,
): ImageVector =
    ImageVector
        .Builder(
            name = name,
            defaultWidth = 24.dp,
            defaultHeight = 24.dp,
            viewportWidth = 24f,
            viewportHeight = 24f,
        ).addPath(
            pathData = addPathNodes(path),
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.7f,
            strokeLineCap = StrokeCap.Round,
            strokeLineJoin = StrokeJoin.Round,
        ).build()

private fun filled(
    name: String,
    path: String,
): ImageVector =
    ImageVector
        .Builder(
            name = name,
            defaultWidth = 24.dp,
            defaultHeight = 24.dp,
            viewportWidth = 24f,
            viewportHeight = 24f,
        ).addPath(
            pathData = addPathNodes(path),
            fill = SolidColor(Color.Black),
            stroke = SolidColor(Color.Black),
            strokeLineWidth = 1.7f,
            strokeLineJoin = StrokeJoin.Round,
        ).build()

@Composable
fun Glyph(
    glyph: ImageVector,
    tint: Color,
    modifier: Modifier = Modifier,
    size: Dp = 22.dp,
    contentDescription: String? = null,
) {
    Image(
        painter = rememberVectorPainter(glyph),
        contentDescription = contentDescription,
        colorFilter = ColorFilter.tint(tint),
        modifier = modifier.size(size),
    )
}
