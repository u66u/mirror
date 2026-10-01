package app.mirror.vault.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import app.mirror.vault.ui.design.Field
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.SerifItalic
import app.mirror.vault.ui.design.Toggle
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.tappable
import kotlin.math.cos
import kotlin.math.sin

@Composable
@Suppress("LongMethod")
fun ConnectScreen(
    defaultDeviceName: String,
    loading: Boolean,
    error: String?,
    onConnect: (serverUrl: String, allowLan: Boolean, password: String, deviceName: String) -> Unit,
) {
    val colors = Mirror.colors
    val focus = LocalFocusManager.current
    var server by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    var deviceName by rememberSaveable { mutableStateOf(defaultDeviceName) }
    var allowLan by rememberSaveable { mutableStateOf(false) }
    var advanced by rememberSaveable { mutableStateOf(false) }
    var reveal by rememberSaveable { mutableStateOf(false) }
    val normalized = normalizeServer(server)
    val insecure = normalized.startsWith("http://")
    val canConnect = server.isNotBlank() && password.isNotEmpty() && (!insecure || allowLan)
    val submit = {
        if (canConnect) {
            focus.clearFocus()
            onConnect(normalized, allowLan, password, deviceName)
        }
    }

    Box(Modifier.fillMaxSize().background(colors.canvas)) {
        Aurora(Modifier.fillMaxWidth().height(420.dp))
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .statusBarsPadding()
                .navigationBarsPadding()
                .imePadding()
                .padding(horizontal = 24.dp),
        ) {
            Spacer(Modifier.height(64.dp))
            MirrorMark()
            Spacer(Modifier.height(26.dp))
            Txt("Mirror", style = Mirror.type.hero)
            Txt(
                "Your photos, kept at home.",
                style = Mirror.type.title.merge(SerifItalic),
                color = colors.inkMuted,
            )
            Spacer(Modifier.height(44.dp))
            Field(
                value = server,
                onValueChange = { server = it.trim() },
                label = "Vault address",
                placeholder = "photos.example.com",
                enabled = !loading,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                hint = if (server.isNotBlank() && normalized != server) normalized else null,
            )
            AnimatedVisibility(
                visible = insecure,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                Row(
                    Modifier
                        .padding(top = 12.dp)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(16.dp))
                        .background(colors.accent.copy(alpha = 0.12f))
                        .padding(14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Glyph(Glyphs.Wifi, tint = colors.accent, size = 20.dp)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Txt("Unencrypted local connection", style = Mirror.type.label)
                        Txt(
                            "Only allowed for private network addresses. Use HTTPS outside your home.",
                            style = Mirror.type.caption,
                            color = colors.inkMuted,
                        )
                    }
                    Spacer(Modifier.width(10.dp))
                    Toggle(allowLan, { allowLan = it }, enabled = !loading)
                }
            }
            Spacer(Modifier.height(18.dp))
            Field(
                value = password,
                onValueChange = { password = it },
                label = "Owner password",
                placeholder = "Your vault password",
                password = !reveal,
                enabled = !loading,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { submit() }),
                trailing = {
                    Txt(
                        if (reveal) "Hide" else "Show",
                        style = Mirror.type.label,
                        color = colors.inkMuted,
                        modifier = Modifier.tappable { reveal = !reveal }.padding(start = 10.dp),
                    )
                },
            )
            Spacer(Modifier.height(14.dp))
            Row(
                Modifier.tappable(haptic = false) { advanced = !advanced }.padding(vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Txt(
                    if (advanced) "Device name" else "Device name · $deviceName",
                    style = Mirror.type.label,
                    color = colors.inkMuted,
                )
                Spacer(Modifier.width(6.dp))
                Glyph(if (advanced) Glyphs.Close else Glyphs.Pencil, tint = colors.inkFaint, size = 14.dp)
            }
            AnimatedVisibility(advanced, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                Field(
                    value = deviceName,
                    onValueChange = { deviceName = it },
                    label = "Shown in your vault's device list",
                    enabled = !loading,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
            AnimatedVisibility(
                visible = error != null,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                Row(
                    Modifier
                        .padding(top = 16.dp)
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(16.dp))
                        .background(colors.danger.copy(alpha = 0.12f))
                        .padding(14.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Glyph(Glyphs.Alert, tint = colors.danger, size = 19.dp)
                    Spacer(Modifier.width(10.dp))
                    Txt(friendlyError(error.orEmpty()), style = Mirror.type.label, color = colors.ink)
                }
            }
            MirrorButton(
                "Connect",
                submit,
                enabled = canConnect,
                loading = loading,
                modifier = Modifier.fillMaxWidth().padding(top = 28.dp),
            )
            Spacer(Modifier.height(12.dp))
            Txt(
                "Sign in with the owner password you set on your vault.",
                style = Mirror.type.caption,
                color = colors.inkFaint,
                modifier = Modifier.align(Alignment.CenterHorizontally).padding(bottom = 16.dp),
            )
        }
    }
}

/** Accepts bare hosts: private IP literals default to http, everything else to https. */
fun normalizeServer(raw: String): String {
    val value = raw.trim().trimEnd('/')
    if (value.isEmpty() || value.contains("://")) return value
    val host = value.substringBefore(':').substringBefore('/')
    val local =
        host == "localhost" ||
            host.matches(Regex("""^(10|127)\.\d+\.\d+\.\d+$""")) ||
            host.matches(Regex("""^192\.168\.\d+\.\d+$""")) ||
            host.matches(Regex("""^172\.(1[6-9]|2\d|3[01])\.\d+\.\d+$""")) ||
            host.matches(Regex("""^169\.254\.\d+\.\d+$"""))
    return (if (local) "http://" else "https://") + value
}

fun friendlyError(raw: String): String {
    val message = raw.lowercase()
    return when {
        "server request failed" in message || "unable to resolve" in message || "timeout" in message ->
            "Check that your server is running and that this device can reach it."
        "password" in message || "credential" in message || "unauthorized" in message ->
            "That password didn't work."
        "private-lan" in message -> "Allow the local connection to continue."
        "too many" in message || "rate" in message -> "Too many attempts. Wait a minute and try again."
        else -> raw.replaceFirstChar(Char::uppercase)
    }
}

/** Two offset rounded frames: the photo and its reflection. */
@Composable
private fun MirrorMark() {
    val colors = Mirror.colors
    Canvas(Modifier.size(56.dp)) {
        val unit = size.minDimension
        val stroke = Stroke(width = unit * 0.06f)
        drawRoundRect(
            color = colors.ink,
            topLeft = Offset(unit * 0.06f, unit * 0.06f),
            size = Size(unit * 0.58f, unit * 0.72f),
            cornerRadius = CornerRadius(unit * 0.12f),
            style = stroke,
        )
        drawRoundRect(
            brush = Brush.linearGradient(listOf(colors.accent, colors.accent.copy(alpha = 0.2f))),
            topLeft = Offset(unit * 0.36f, unit * 0.22f),
            size = Size(unit * 0.58f, unit * 0.72f),
            cornerRadius = CornerRadius(unit * 0.12f),
        )
    }
}

/** Slow drifting light behind the title; deliberately subtle. */
@Composable
private fun Aurora(modifier: Modifier) {
    val colors = Mirror.colors
    val transition = rememberInfiniteTransition(label = "aurora")
    val phase by transition.animateFloat(
        initialValue = 0f,
        targetValue = (2 * Math.PI).toFloat(),
        animationSpec = infiniteRepeatable(tween(18_000, easing = LinearEasing)),
        label = "phase",
    )
    val warm = colors.accent.copy(alpha = if (colors.dark) 0.30f else 0.22f)
    val cool = Color(0xFF7FA7C9).copy(alpha = if (colors.dark) 0.18f else 0.16f)
    Canvas(modifier) {
        val w = size.width
        val h = size.height
        val a = Offset(w * (0.75f + 0.12f * cos(phase)), h * (0.25f + 0.1f * sin(phase)))
        val b = Offset(w * (0.2f + 0.15f * sin(phase * 0.7f)), h * (0.45f + 0.08f * cos(phase)))
        drawGlow(warm, a, w * 0.7f)
        drawGlow(cool, b, w * 0.6f)
        drawRect(Brush.verticalGradient(listOf(Color.Transparent, colors.canvas), startY = h * 0.55f, endY = h))
    }
}

private fun DrawScope.drawGlow(
    color: Color,
    center: Offset,
    radius: Float,
) {
    drawCircle(
        brush = Brush.radialGradient(listOf(color, Color.Transparent), center = center, radius = radius),
        radius = radius,
        center = center,
    )
}
