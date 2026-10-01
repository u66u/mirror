package app.mirror.vault.ui

import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.net.toUri
import app.mirror.vault.settings.AppSettings
import app.mirror.vault.settings.ShareExpiry
import app.mirror.vault.settings.ThemeMode
import app.mirror.vault.ui.design.ButtonTone
import app.mirror.vault.ui.design.Card
import app.mirror.vault.ui.design.CardRow
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.Hairline
import app.mirror.vault.ui.design.LocalToaster
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorButton
import app.mirror.vault.ui.design.RoundGlyphButton
import app.mirror.vault.ui.design.Segmented
import app.mirror.vault.ui.design.StatusBarScrim
import app.mirror.vault.ui.design.Toggle
import app.mirror.vault.ui.design.Txt
import coil3.SingletonImageLoader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class SettingsCallbacks(
    val onTheme: (ThemeMode) -> Unit,
    val onShareExpiry: (ShareExpiry) -> Unit,
    val onHidePreviews: (Boolean) -> Unit,
    val onClose: () -> Unit,
)

/**
 * Per-device preferences only. Anything that changes the vault for every
 * device or user (accounts, models, retention) stays in the web admin, which
 * the "Managed on your server" card points to.
 */
@Composable
@Suppress("LongMethod")
fun SettingsScreen(
    settings: AppSettings,
    serverUrl: String,
    callbacks: SettingsCallbacks,
) {
    val colors = Mirror.colors
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val toaster = LocalToaster.current
    var cacheBytes by remember { mutableLongStateOf(-1L) }
    val loader = remember { SingletonImageLoader.get(context) }
    val version =
        remember {
            runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
                .getOrNull()
                .orEmpty()
        }
    LaunchedEffect(Unit) {
        cacheBytes = withContext(Dispatchers.IO) { loader.diskCache?.size ?: 0L }
    }
    BackHandler(onBack = callbacks.onClose)

    Box(Modifier.fillMaxSize().background(colors.canvas)) {
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 48.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            item {
                Column(Modifier.statusBarsPadding().padding(top = 14.dp)) {
                    RoundGlyphButton(Glyphs.Back, callbacks.onClose, contentDescription = "Back")
                    Spacer(Modifier.height(18.dp))
                    Txt("Settings", style = Mirror.type.display, modifier = Modifier.padding(start = 4.dp))
                    Txt(
                        "For this device only",
                        color = colors.inkMuted,
                        modifier = Modifier.padding(start = 4.dp, bottom = 6.dp),
                    )
                }
            }
            item { Section("Appearance") }
            item {
                Card {
                    Column(Modifier.padding(18.dp)) {
                        Txt("Theme", style = Mirror.type.bodyStrong)
                        Spacer(Modifier.height(12.dp))
                        Segmented(
                            options = ThemeMode.entries.map { it to it.label },
                            selected = settings.theme,
                            onSelect = callbacks.onTheme,
                        )
                    }
                }
            }
            item { Section("Sharing") }
            item {
                Card {
                    Column(Modifier.padding(18.dp)) {
                        Txt("Private links expire after", style = Mirror.type.bodyStrong)
                        Spacer(Modifier.height(4.dp))
                        Txt(
                            "Anyone with the link can view a privacy-filtered copy until then.",
                            style = Mirror.type.caption,
                            color = colors.inkMuted,
                        )
                        Spacer(Modifier.height(12.dp))
                        Segmented(
                            options = ShareExpiry.entries.map { it to it.label },
                            selected = settings.shareExpiry,
                            onSelect = callbacks.onShareExpiry,
                        )
                    }
                }
            }
            item { Section("Privacy") }
            item {
                Card {
                    CardRow(
                        title = "Hide in app switcher",
                        subtitle = "Blanks Mirror's preview in recent apps and blocks screenshots.",
                        glyph = Glyphs.Eye,
                    ) { Toggle(settings.hidePreviews, callbacks.onHidePreviews) }
                }
            }
            item { Section("Storage") }
            item {
                Card {
                    CardRow(
                        title = "Image cache",
                        subtitle =
                            when {
                                cacheBytes < 0 -> "Calculating…"
                                else -> "${humanBytes(cacheBytes)} of thumbnails kept for quick browsing"
                            },
                        glyph = Glyphs.Cache,
                    ) {
                        MirrorButton(
                            "Clear",
                            {
                                scope.launch {
                                    withContext(Dispatchers.IO) {
                                        loader.memoryCache?.clear()
                                        loader.diskCache?.clear()
                                    }
                                    cacheBytes = 0
                                    toaster.show("Cache cleared")
                                }
                            },
                            tone = ButtonTone.SECONDARY,
                            enabled = cacheBytes != 0L,
                        )
                    }
                }
            }
            item { Section("Managed on your server") }
            item {
                Card {
                    Column(Modifier.padding(18.dp)) {
                        Txt(
                            "Some things affect every device, so they live in your vault's web admin:",
                            style = Mirror.type.body,
                            color = colors.inkMuted,
                        )
                        Spacer(Modifier.height(10.dp))
                        listOf(
                            "Password, two-factor sign-in and recovery codes",
                            "Other devices and sign-in sessions",
                            "Smart search and face recognition models",
                            "Backups, exports and trash retention",
                        ).forEach { line ->
                            Row(Modifier.padding(vertical = 3.dp), verticalAlignment = Alignment.Top) {
                                Glyph(Glyphs.Check, tint = colors.accent, size = 16.dp, modifier = Modifier.padding(top = 3.dp))
                                Spacer(Modifier.width(10.dp))
                                Txt(line, style = Mirror.type.body)
                            }
                        }
                        Spacer(Modifier.height(16.dp))
                        MirrorButton(
                            "Open web admin",
                            { context.startActivity(Intent(Intent.ACTION_VIEW, serverUrl.toUri())) },
                            tone = ButtonTone.SECONDARY,
                            glyph = Glyphs.Link,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
            }
            item { Section("About") }
            item {
                Card {
                    CardRow(title = "Mirror", subtitle = "Version $version", glyph = Glyphs.Shield)
                    Hairline()
                    CardRow(
                        title = "Vault",
                        subtitle = serverUrl.removePrefix("https://").removePrefix("http://"),
                        glyph = Glyphs.Server,
                    )
                    Hairline()
                    CardRow(
                        title = "Typefaces",
                        subtitle = "Geist and Instrument Serif, under the SIL Open Font License",
                        glyph = Glyphs.Pencil,
                    )
                }
            }
        }
        StatusBarScrim(Modifier.align(Alignment.TopCenter))
    }
}

@Composable
private fun Section(text: String) {
    Txt(
        text.uppercase(),
        style = Mirror.type.overline,
        color = Mirror.colors.inkMuted,
        modifier = Modifier.padding(start = 8.dp, top = 12.dp),
    )
}
