package app.mirror.vault.ui.design

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

data class Toast(
    val message: String,
    val actionLabel: String? = null,
    val action: (() -> Unit)? = null,
    val id: Long = System.nanoTime(),
)

@Stable
class Toaster {
    var current by mutableStateOf<Toast?>(null)
        private set

    fun show(
        message: String,
        actionLabel: String? = null,
        action: (() -> Unit)? = null,
    ) {
        current = Toast(message, actionLabel, action)
    }

    fun dismiss(toast: Toast) {
        if (current?.id == toast.id) current = null
    }
}

val LocalToaster = staticCompositionLocalOf { Toaster() }

@Composable
fun BoxScope.ToastHost(
    toaster: Toaster,
    bottomOffset: Int,
) {
    val toast = toaster.current
    LaunchedEffect(toast?.id) {
        toast ?: return@LaunchedEffect
        delay(if (toast.action != null) 5_000 else 2_600)
        toaster.dismiss(toast)
    }
    AnimatedVisibility(
        visible = toast != null,
        enter = slideInVertically { it } + fadeIn() + scaleIn(initialScale = 0.92f),
        exit = slideOutVertically { it / 2 } + fadeOut() + scaleOut(targetScale = 0.96f),
        modifier =
            Modifier
                .align(Alignment.BottomCenter)
                .navigationBarsPadding()
                .padding(bottom = bottomOffset.dp, start = 20.dp, end = 20.dp),
    ) {
        val shown = remember(toast?.id) { toast } ?: return@AnimatedVisibility
        val colors = Mirror.colors
        Row(
            modifier =
                Modifier
                    .shadow(24.dp, Pill, ambientColor = colors.scrim, spotColor = colors.scrim)
                    .clip(Pill)
                    .background(colors.ink)
                    .padding(start = 20.dp, end = if (shown.action != null) 6.dp else 20.dp)
                    .height(48.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Txt(shown.message, style = Mirror.type.label, color = colors.canvas, maxLines = 1)
            if (shown.action != null && shown.actionLabel != null) {
                Spacer(Modifier.width(14.dp))
                Box(
                    Modifier
                        .clip(Pill)
                        .background(colors.accent)
                        .tappable {
                            shown.action.invoke()
                            toaster.dismiss(shown)
                        }.padding(horizontal = 16.dp, vertical = 9.dp),
                ) {
                    Txt(shown.actionLabel, style = Mirror.type.label, color = colors.onAccent)
                }
            }
        }
    }
}

/**
 * Modal sheet that rises from the bottom with a dimmed backdrop. Drag the
 * handle (or the sheet) down to dismiss; back gesture dismisses too.
 */
@Composable
fun Sheet(
    visible: Boolean,
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = Mirror.colors
    BackHandler(enabled = visible, onBack = onDismiss)
    Box(Modifier.fillMaxSize()) {
        AnimatedVisibility(visible = visible, enter = fadeIn(), exit = fadeOut()) {
            Box(
                Modifier
                    .fillMaxSize()
                    .background(colors.scrim)
                    .clickable(
                        interactionSource = remember { MutableInteractionSource() },
                        indication = null,
                        onClick = onDismiss,
                    ),
            )
        }
        AnimatedVisibility(
            visible = visible,
            enter = slideInVertically(spring(dampingRatio = 0.86f, stiffness = Spring.StiffnessMediumLow)) { it },
            exit = slideOutVertically { it },
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            val drag = remember { Animatable(0f) }
            val scope = rememberCoroutineScope()
            Column(
                modifier =
                    Modifier
                        .offset { IntOffset(0, drag.value.roundToInt()) }
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(topStart = 30.dp, topEnd = 30.dp))
                        .background(colors.surface)
                        .clickable(
                            interactionSource = remember { MutableInteractionSource() },
                            indication = null,
                            onClick = {},
                        ).draggable(
                            orientation = Orientation.Vertical,
                            state =
                                rememberDraggableState { delta ->
                                    scope.launch { drag.snapTo((drag.value + delta).coerceAtLeast(0f)) }
                                },
                            onDragStopped = { velocity ->
                                if (drag.value > 160f || velocity > 1400f) {
                                    onDismiss()
                                } else {
                                    drag.animateTo(0f, spring(dampingRatio = 0.7f))
                                }
                            },
                        ).navigationBarsPadding()
                        .imePadding()
                        .padding(horizontal = 22.dp)
                        .padding(bottom = 18.dp),
            ) {
                Box(
                    Modifier
                        .align(Alignment.CenterHorizontally)
                        .padding(top = 10.dp, bottom = 14.dp)
                        .size(width = 38.dp, height = 4.dp)
                        .clip(Pill)
                        .background(colors.line),
                )
                content()
            }
        }
    }
}
