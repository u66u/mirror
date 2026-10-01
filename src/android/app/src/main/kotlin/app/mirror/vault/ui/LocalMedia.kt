package app.mirror.vault.ui

import android.graphics.Bitmap
import android.util.LruCache
import android.util.Size
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.request.crossfade
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

private const val THUMBNAIL_CACHE_BYTES = 48 * 1024 * 1024
private const val DEFAULT_THUMBNAIL_PX = 384

private val thumbnailCache =
    object : LruCache<String, Bitmap>(THUMBNAIL_CACHE_BYTES) {
        override fun sizeOf(
            key: String,
            value: Bitmap,
        ): Int = value.byteCount
    }

/**
 * Thumbnail of a photo or video on this device, from the system's own
 * thumbnail store (so it is fast, works offline and covers videos too).
 */
@Composable
fun LocalThumbnail(
    uri: String,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Crop,
    sizePx: Int = DEFAULT_THUMBNAIL_PX,
    background: Color = Color.Transparent,
) {
    val context = LocalContext.current
    val key = "$uri@$sizePx"
    val bitmap: ImageBitmap? by
        produceState(initialValue = thumbnailCache.get(key)?.asImageBitmap(), key) {
            if (value == null) {
                value =
                    withContext(Dispatchers.IO) {
                        runCatching { context.contentResolver.loadThumbnail(uri.toUri(), Size(sizePx, sizePx), null) }
                            .getOrNull()
                            ?.also { thumbnailCache.put(key, it) }
                            ?.asImageBitmap()
                    }
            }
        }
    Box(modifier.background(background)) {
        bitmap?.let {
            Image(it, contentDescription = null, contentScale = contentScale, modifier = Modifier.fillMaxSize())
        }
    }
}

/** Full-size photo straight from the device; decoded at screen size, no network involved. */
@Composable
fun LocalImage(
    uri: String,
    modifier: Modifier = Modifier,
    contentScale: ContentScale = ContentScale.Fit,
    contentDescription: String? = null,
) {
    val context = LocalContext.current
    val request =
        remember(uri) {
            ImageRequest
                .Builder(context)
                .data(uri.toUri())
                .crossfade(180)
                .build()
        }
    AsyncImage(
        model = request,
        contentDescription = contentDescription,
        contentScale = contentScale,
        modifier = modifier,
    )
}
