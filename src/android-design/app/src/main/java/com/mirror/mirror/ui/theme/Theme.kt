package com.mirror.mirror.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp

private val DarkColorScheme = darkColorScheme(
    primary = MirrorPrimaryDark,
    onPrimary = MirrorOnPrimaryDark,
    primaryContainer = MirrorPrimaryContainerDark,
    onPrimaryContainer = MirrorOnPrimaryContainerDark,
    secondary = MirrorSecondaryDark,
    onSecondary = MirrorOnSecondaryDark,
    secondaryContainer = MirrorSecondaryContainerDark,
    onSecondaryContainer = MirrorOnSecondaryContainerDark,
    tertiary = MirrorTertiaryDark,
    onTertiary = MirrorOnTertiaryDark,
    tertiaryContainer = MirrorTertiaryContainerDark,
    onTertiaryContainer = MirrorOnTertiaryContainerDark,
    error = MirrorErrorDark,
    onError = MirrorOnErrorDark,
    errorContainer = MirrorErrorContainerDark,
    onErrorContainer = MirrorOnErrorContainerDark,
    background = MirrorBackgroundDark,
    onBackground = MirrorOnBackgroundDark,
    surface = MirrorSurfaceDark,
    onSurface = MirrorOnSurfaceDark,
    surfaceVariant = MirrorSurfaceVariantDark,
    onSurfaceVariant = MirrorOnSurfaceVariantDark,
    outline = MirrorOutlineDark,
    outlineVariant = MirrorOutlineVariantDark,
)

private val LightColorScheme = lightColorScheme(
    primary = MirrorPrimaryLight,
    onPrimary = MirrorOnPrimaryLight,
    primaryContainer = MirrorPrimaryContainerLight,
    onPrimaryContainer = MirrorOnPrimaryContainerLight,
    secondary = MirrorSecondaryLight,
    onSecondary = MirrorOnSecondaryLight,
    secondaryContainer = MirrorSecondaryContainerLight,
    onSecondaryContainer = MirrorOnSecondaryContainerLight,
    tertiary = MirrorTertiaryLight,
    onTertiary = MirrorOnTertiaryLight,
    tertiaryContainer = MirrorTertiaryContainerLight,
    onTertiaryContainer = MirrorOnTertiaryContainerLight,
    error = MirrorErrorLight,
    onError = MirrorOnErrorLight,
    errorContainer = MirrorErrorContainerLight,
    onErrorContainer = MirrorOnErrorContainerLight,
    background = MirrorBackgroundLight,
    onBackground = MirrorOnBackgroundLight,
    surface = MirrorSurfaceLight,
    onSurface = MirrorOnSurfaceLight,
    surfaceVariant = MirrorSurfaceVariantLight,
    onSurfaceVariant = MirrorOnSurfaceVariantLight,
    outline = MirrorOutlineLight,
    outlineVariant = MirrorOutlineVariantLight,
)

private val MirrorShapes = Shapes(
    extraSmall = RoundedCornerShape(4.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(8.dp),
    large = RoundedCornerShape(8.dp),
    extraLarge = RoundedCornerShape(12.dp),
)

@Composable
fun MirrorTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit,
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }

        darkTheme -> DarkColorScheme
        else -> LightColorScheme
    }

    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography,
        shapes = MirrorShapes,
        content = content,
    )
}
