package app.mirror.vault.ui.design

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import app.mirror.vault.R

/**
 * Mirror's own design language. No Material components: every surface, control
 * and motion curve is defined here so the app reads as one quiet, photo-first
 * object instead of a stock Android form.
 */
@Immutable
data class MirrorColors(
    val dark: Boolean,
    val canvas: Color,
    val surface: Color,
    val raised: Color,
    val line: Color,
    val ink: Color,
    val inkMuted: Color,
    val inkFaint: Color,
    val accent: Color,
    val onAccent: Color,
    val positive: Color,
    val danger: Color,
    val scrim: Color,
    val tile: Color,
)

private val DarkPalette =
    MirrorColors(
        dark = true,
        canvas = Color(0xFF0C0C0B),
        surface = Color(0xFF161614),
        raised = Color(0xFF201F1C),
        line = Color(0xFF2C2B27),
        ink = Color(0xFFF4F1EA),
        inkMuted = Color(0xFFA6A197),
        inkFaint = Color(0xFF6C6962),
        accent = Color(0xFFF2B36B),
        onAccent = Color(0xFF1B1408),
        positive = Color(0xFF9AD5A0),
        danger = Color(0xFFF0806F),
        scrim = Color(0xCC050505),
        tile = Color(0xFF1C1B19),
    )

private val LightPalette =
    MirrorColors(
        dark = false,
        canvas = Color(0xFFF6F3EC),
        surface = Color(0xFFFFFFFF),
        raised = Color(0xFFEDE9E0),
        line = Color(0xFFE0DBD0),
        ink = Color(0xFF181715),
        inkMuted = Color(0xFF6D685F),
        inkFaint = Color(0xFFA39E94),
        accent = Color(0xFFB8621B),
        onAccent = Color(0xFFFFF8EF),
        positive = Color(0xFF2F7A4C),
        danger = Color(0xFFBF3F2B),
        scrim = Color(0x99141310),
        tile = Color(0xFFE6E1D7),
    )

@Immutable
data class MirrorType(
    val hero: TextStyle,
    val display: TextStyle,
    val title: TextStyle,
    val heading: TextStyle,
    val body: TextStyle,
    val bodyStrong: TextStyle,
    val label: TextStyle,
    val caption: TextStyle,
    val overline: TextStyle,
)

@OptIn(ExperimentalTextApi::class)
private fun geist(weight: Int) =
    Font(
        resId = R.font.geist,
        weight = FontWeight(weight),
        variationSettings = FontVariation.Settings(FontVariation.weight(weight)),
    )

private val Geist =
    FontFamily(
        geist(400),
        geist(500),
        geist(600),
        geist(700),
    )

private val Serif =
    FontFamily(
        Font(R.font.instrument_serif, FontWeight.Normal),
        Font(R.font.instrument_serif_italic, FontWeight.Normal, FontStyle.Italic),
    )

private val Type =
    MirrorType(
        hero = TextStyle(fontFamily = Serif, fontSize = 64.sp, lineHeight = 60.sp, letterSpacing = (-0.02).em),
        display = TextStyle(fontFamily = Serif, fontSize = 40.sp, lineHeight = 42.sp, letterSpacing = (-0.01).em),
        title = TextStyle(fontFamily = Serif, fontSize = 28.sp, lineHeight = 32.sp),
        heading = TextStyle(fontFamily = Geist, fontWeight = FontWeight(600), fontSize = 17.sp, lineHeight = 22.sp),
        body = TextStyle(fontFamily = Geist, fontWeight = FontWeight(400), fontSize = 15.sp, lineHeight = 21.sp),
        bodyStrong = TextStyle(fontFamily = Geist, fontWeight = FontWeight(500), fontSize = 15.sp, lineHeight = 21.sp),
        label = TextStyle(fontFamily = Geist, fontWeight = FontWeight(500), fontSize = 13.sp, lineHeight = 16.sp),
        caption = TextStyle(fontFamily = Geist, fontWeight = FontWeight(400), fontSize = 12.sp, lineHeight = 16.sp),
        overline =
            TextStyle(
                fontFamily = Geist,
                fontWeight = FontWeight(600),
                fontSize = 11.sp,
                lineHeight = 14.sp,
                letterSpacing = 0.12.em,
            ),
    )

val SerifItalic: TextStyle = TextStyle(fontFamily = Serif, fontStyle = FontStyle.Italic)

private val LocalColors = staticCompositionLocalOf { DarkPalette }
private val LocalType = staticCompositionLocalOf { Type }

object Mirror {
    val colors: MirrorColors
        @Composable get() = LocalColors.current

    val type: MirrorType
        @Composable get() = LocalType.current
}

@Composable
fun MirrorTheme(
    dark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val colors = if (dark) DarkPalette else LightPalette
    CompositionLocalProvider(
        LocalColors provides colors,
        LocalType provides Type,
        LocalTextSelectionColors provides
            TextSelectionColors(
                handleColor = colors.accent,
                backgroundColor = colors.accent.copy(alpha = 0.3f),
            ),
        content = content,
    )
}
