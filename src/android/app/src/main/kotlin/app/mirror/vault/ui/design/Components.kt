package app.mirror.vault.ui.design

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp

val Pill = RoundedCornerShape(percent = 50)
val CardShape = RoundedCornerShape(22.dp)
val TileShape = RoundedCornerShape(4.dp)

@Composable
fun Txt(
    text: String,
    modifier: Modifier = Modifier,
    style: TextStyle = Mirror.type.body,
    color: Color = Mirror.colors.ink,
    maxLines: Int = Int.MAX_VALUE,
) {
    BasicText(
        text = text,
        modifier = modifier,
        style = style.copy(color = color),
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}

/**
 * Tap target with a soft press-scale instead of a ripple, plus a light haptic
 * tick. Used by every interactive surface for a consistent physical feel.
 */
fun Modifier.tappable(
    enabled: Boolean = true,
    pressedScale: Float = 0.96f,
    haptic: Boolean = true,
    role: Role = Role.Button,
    onClick: () -> Unit,
): Modifier =
    composed {
        val interaction = remember { MutableInteractionSource() }
        val pressed by interaction.collectIsPressedAsState()
        val scale by animateFloatAsState(
            targetValue = if (pressed && enabled) pressedScale else 1f,
            animationSpec = spring(dampingRatio = 0.55f, stiffness = Spring.StiffnessMedium),
            label = "press",
        )
        val haptics = LocalHapticFeedback.current
        this
            .graphicsLayer {
                scaleX = scale
                scaleY = scale
            }.clickable(
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = role,
            ) {
                if (haptic) haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                onClick()
            }
    }

enum class ButtonTone { PRIMARY, SECONDARY, GHOST, DANGER }

@Composable
fun MirrorButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    tone: ButtonTone = ButtonTone.PRIMARY,
    glyph: ImageVector? = null,
    enabled: Boolean = true,
    loading: Boolean = false,
) {
    val colors = Mirror.colors
    val (background, foreground) =
        when (tone) {
            ButtonTone.PRIMARY -> colors.ink to colors.canvas
            ButtonTone.SECONDARY -> colors.raised to colors.ink
            ButtonTone.GHOST -> Color.Transparent to colors.ink
            ButtonTone.DANGER -> colors.danger.copy(alpha = 0.14f) to colors.danger
        }
    val alpha by animateFloatAsState(if (enabled || loading) 1f else 0.38f, label = "enabled")
    Row(
        modifier =
            modifier
                .heightIn(min = 52.dp)
                .graphicsLayer { this.alpha = alpha }
                .clip(Pill)
                .background(background)
                .tappable(enabled = enabled && !loading, onClick = onClick)
                .padding(horizontal = 22.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (loading) {
            Spinner(color = foreground, size = 18.dp)
        } else {
            glyph?.let {
                Glyph(it, tint = foreground, size = 19.dp)
                Spacer(Modifier.width(8.dp))
            }
            Txt(text, style = Mirror.type.bodyStrong, color = foreground, maxLines = 1)
        }
    }
}

@Composable
fun RoundGlyphButton(
    glyph: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    tint: Color = Mirror.colors.ink,
    background: Color = Mirror.colors.raised,
    size: Dp = 44.dp,
    glyphSize: Dp = 21.dp,
    contentDescription: String? = null,
    enabled: Boolean = true,
) {
    Box(
        modifier =
            modifier
                .size(size)
                .clip(CircleShape)
                .background(background)
                .tappable(enabled = enabled, pressedScale = 0.88f, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Glyph(
            glyph,
            tint = tint.copy(alpha = if (enabled) tint.alpha else 0.35f),
            size = glyphSize,
            contentDescription = contentDescription,
        )
    }
}

@Composable
fun Spinner(
    color: Color = Mirror.colors.ink,
    size: Dp = 22.dp,
    stroke: Dp = 2.dp,
) {
    val transition = rememberInfiniteTransition(label = "spinner")
    val angle by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(900, easing = LinearEasing)),
        label = "angle",
    )
    Canvas(Modifier.size(size)) {
        drawArc(
            color = color.copy(alpha = 0.18f),
            startAngle = 0f,
            sweepAngle = 360f,
            useCenter = false,
            style = Stroke(stroke.toPx()),
        )
        drawArc(
            color = color,
            startAngle = angle,
            sweepAngle = 100f,
            useCenter = false,
            style = Stroke(stroke.toPx(), cap = StrokeCap.Round),
        )
    }
}

@Composable
fun ProgressRing(
    progress: Float,
    modifier: Modifier = Modifier,
    size: Dp = 168.dp,
    stroke: Dp = 10.dp,
    track: Color = Mirror.colors.line,
    color: Color = Mirror.colors.accent,
    spinning: Boolean = false,
) {
    val animated by animateFloatAsState(
        targetValue = progress.coerceIn(0f, 1f),
        animationSpec = spring(stiffness = Spring.StiffnessVeryLow),
        label = "ring",
    )
    val transition = rememberInfiniteTransition(label = "ring-spin")
    val rotation by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(tween(2400, easing = LinearEasing)),
        label = "rotation",
    )
    Canvas(modifier.size(size)) {
        val width = stroke.toPx()
        drawArc(
            color = track,
            startAngle = 0f,
            sweepAngle = 360f,
            useCenter = false,
            style = Stroke(width),
            topLeft = Offset(width / 2, width / 2),
            size =
                androidx.compose.ui.geometry
                    .Size(this.size.width - width, this.size.height - width),
        )
        val start = if (spinning) rotation - 90f else -90f
        drawArc(
            color = color,
            startAngle = start,
            sweepAngle = (animated * 360f).coerceAtLeast(if (spinning) 24f else 0f),
            useCenter = false,
            style = Stroke(width, cap = StrokeCap.Round),
            topLeft = Offset(width / 2, width / 2),
            size =
                androidx.compose.ui.geometry
                    .Size(this.size.width - width, this.size.height - width),
        )
    }
}

@Composable
fun Toggle(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    enabled: Boolean = true,
) {
    val colors = Mirror.colors
    val track by animateColorAsState(if (checked) colors.accent else colors.line, label = "track")
    val knob by animateFloatAsState(
        targetValue = if (checked) 1f else 0f,
        animationSpec = spring(dampingRatio = 0.6f, stiffness = Spring.StiffnessMediumLow),
        label = "knob",
    )
    Box(
        modifier =
            Modifier
                .size(width = 50.dp, height = 30.dp)
                .graphicsLayer { alpha = if (enabled) 1f else 0.4f }
                .clip(Pill)
                .background(track)
                .tappable(enabled = enabled, pressedScale = 0.94f, role = Role.Switch) {
                    onCheckedChange(!checked)
                }.padding(3.dp),
    ) {
        Box(
            Modifier
                .offset { IntOffset((20.dp * knob).roundToPx(), 0) }
                .size(24.dp)
                .clip(CircleShape)
                .background(if (checked) colors.onAccent else colors.surface),
        )
    }
}

@Composable
fun <T> Segmented(
    options: List<Pair<T, String>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = Mirror.colors
    Row(
        modifier =
            modifier
                .clip(Pill)
                .background(colors.raised)
                .padding(4.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        options.forEach { (value, label) ->
            val active = value == selected
            val background by animateColorAsState(
                if (active) colors.ink else Color.Transparent,
                label = "segment",
            )
            Box(
                modifier =
                    Modifier
                        .clip(Pill)
                        .background(background)
                        .tappable { onSelect(value) }
                        .padding(horizontal = 16.dp, vertical = 9.dp),
                contentAlignment = Alignment.Center,
            ) {
                Txt(
                    label,
                    style = Mirror.type.label,
                    color = if (active) colors.canvas else colors.inkMuted,
                )
            }
        }
    }
}

@Composable
@Suppress("LongParameterList") // A text field genuinely has this many knobs.
fun Field(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    password: Boolean = false,
    enabled: Boolean = true,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    hint: String? = null,
    error: String? = null,
    onBlur: () -> Unit = {},
    trailing: @Composable (() -> Unit)? = null,
) {
    val colors = Mirror.colors
    var focused by remember { mutableStateOf(false) }
    // Merged so assistive tech reads "label, edit box, value" as one control.
    Column(modifier.semantics(mergeDescendants = true) {}) {
        Txt(label.uppercase(), style = Mirror.type.overline, color = colors.inkMuted)
        Spacer(Modifier.height(8.dp))
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(16.dp))
                    .background(colors.surface)
                    .border(
                        if (error != null) 1.5.dp else 1.dp,
                        if (error != null) colors.danger else colors.line,
                        RoundedCornerShape(16.dp),
                    ).padding(horizontal = 16.dp, vertical = 15.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(Modifier.weight(1f)) {
                if (value.isEmpty()) {
                    Txt(placeholder, color = colors.inkFaint, maxLines = 1)
                }
                BasicTextField(
                    value = value,
                    onValueChange = onValueChange,
                    enabled = enabled,
                    singleLine = true,
                    textStyle = Mirror.type.body.copy(color = colors.ink),
                    cursorBrush = SolidColor(colors.accent),
                    visualTransformation =
                        if (password) PasswordVisualTransformation() else VisualTransformation.None,
                    keyboardOptions = keyboardOptions,
                    keyboardActions = keyboardActions,
                    modifier =
                        Modifier.fillMaxWidth().onFocusChanged {
                            if (focused && !it.isFocused) onBlur()
                            focused = it.isFocused
                        },
                )
            }
            trailing?.invoke()
        }
        val note = error ?: hint
        if (note != null) {
            Spacer(Modifier.height(6.dp))
            Txt(note, style = Mirror.type.caption, color = if (error != null) colors.danger else colors.inkMuted)
        }
    }
}

/** Rounded grouped container, used for settings-like lists. */
@Composable
fun Card(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    Column(
        modifier =
            modifier
                .fillMaxWidth()
                .clip(CardShape)
                .background(Mirror.colors.surface),
    ) {
        content()
    }
}

@Composable
fun CardRow(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    glyph: ImageVector? = null,
    glyphTint: Color = Mirror.colors.ink,
    onClick: (() -> Unit)? = null,
    trailing: @Composable RowScope.() -> Unit = {},
) {
    val colors = Mirror.colors
    Row(
        modifier =
            modifier
                .fillMaxWidth()
                .then(if (onClick != null) Modifier.tappable(pressedScale = 0.985f, onClick = onClick) else Modifier)
                .padding(horizontal = 18.dp, vertical = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        glyph?.let {
            Box(
                Modifier
                    .size(36.dp)
                    .clip(RoundedCornerShape(11.dp))
                    .background(colors.raised),
                contentAlignment = Alignment.Center,
            ) {
                Glyph(it, tint = glyphTint, size = 19.dp)
            }
            Spacer(Modifier.width(14.dp))
        }
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Txt(title, style = Mirror.type.bodyStrong, maxLines = 1)
            subtitle?.let {
                Txt(it, style = Mirror.type.caption, color = colors.inkMuted, maxLines = 2)
            }
        }
        trailing()
    }
}

@Composable
fun Hairline(modifier: Modifier = Modifier) {
    Box(
        modifier
            .fillMaxWidth()
            .padding(start = 18.dp)
            .height(1.dp)
            .background(Mirror.colors.line),
    )
}

/** Slow diagonal sheen for placeholders while thumbnails stream in. */
fun Modifier.shimmer(base: Color): Modifier =
    composed {
        val transition = rememberInfiniteTransition(label = "shimmer")
        val shift by transition.animateFloat(
            initialValue = -1f,
            targetValue = 2f,
            animationSpec = infiniteRepeatable(tween(1600, easing = LinearEasing), RepeatMode.Restart),
            label = "shift",
        )
        background(base).drawWithContent {
            val width = size.width
            drawRect(
                Brush.linearGradient(
                    colors = listOf(Color.Transparent, Color.White.copy(alpha = 0.06f), Color.Transparent),
                    start = Offset(width * shift - width, 0f),
                    end = Offset(width * shift, size.height),
                ),
            )
            drawContent()
        }
    }

/** Solid strip under the status bar so scrolling content never collides with the clock and icons. */
@Composable
fun StatusBarScrim(modifier: Modifier = Modifier) {
    Box(
        modifier
            .fillMaxWidth()
            .windowInsetsTopHeight(WindowInsets.statusBars)
            .background(Mirror.colors.canvas.copy(alpha = 0.97f)),
    )
}
