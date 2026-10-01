package app.mirror.vault.ui

import android.app.Activity
import android.content.Intent
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.expandHorizontally
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.shrinkHorizontally
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.mirror.vault.library.LOCAL_ID_PREFIX
import app.mirror.vault.library.PeopleViewModel
import app.mirror.vault.library.SearchViewModel
import app.mirror.vault.library.TrashViewModel
import app.mirror.vault.library.mergeLibrary
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.settings.AppPreferences
import app.mirror.vault.settings.ThemeMode
import app.mirror.vault.timeline.TimelineViewModel
import app.mirror.vault.ui.design.Glyph
import app.mirror.vault.ui.design.Glyphs
import app.mirror.vault.ui.design.LocalToaster
import app.mirror.vault.ui.design.Mirror
import app.mirror.vault.ui.design.MirrorTheme
import app.mirror.vault.ui.design.Pill
import app.mirror.vault.ui.design.ToastHost
import app.mirror.vault.ui.design.Toaster
import app.mirror.vault.ui.design.Txt
import app.mirror.vault.ui.design.tappable
import kotlinx.coroutines.launch

data class MirrorViewModels(
    val login: LoginViewModel,
    val backup: BackupViewModel,
    val timeline: TimelineViewModel,
    val search: SearchViewModel,
    val people: PeopleViewModel,
    val trash: TrashViewModel,
    val preferences: AppPreferences,
)

private enum class Phase { BOOT, CONNECT, MAIN }

enum class Tab(
    val label: String,
    val glyph: ImageVector,
) {
    LIBRARY("Library", Glyphs.Photos),
    SEARCH("Search", Glyphs.Search),
    PEOPLE("People", Glyphs.People),
    VAULT("Vault", Glyphs.Vault),
}

@Composable
fun MirrorApp(models: MirrorViewModels) {
    val login by models.login.state.collectAsStateWithLifecycle()
    val timeline by models.timeline.state.collectAsStateWithLifecycle()
    val settings by models.preferences.settings.collectAsStateWithLifecycle()
    val dark =
        when (settings.theme) {
            ThemeMode.SYSTEM -> isSystemInDarkTheme()
            ThemeMode.LIGHT -> false
            ThemeMode.DARK -> true
        }
    SystemBarIcons(dark)
    var booted by rememberSaveable { mutableStateOf(false) }
    if (!login.loading) booted = true
    LaunchedEffect(login.credential) {
        models.timeline.setCredential(login.credential)
        models.search.setCredential(login.credential)
        models.people.setCredential(login.credential)
        models.trash.setCredential(login.credential)
    }
    val toaster = remember { Toaster() }
    val phase =
        when {
            !booted -> Phase.BOOT
            login.credential == null -> Phase.CONNECT
            else -> Phase.MAIN
        }
    MirrorTheme(dark = dark) {
        CompositionLocalProvider(
            LocalToaster provides toaster,
            LocalAuthHeader provides models.timeline.authorizationHeader(),
        ) {
            Box(Modifier.fillMaxSize().background(Mirror.colors.canvas)) {
                AnimatedContent(
                    targetState = phase,
                    transitionSpec = {
                        (fadeIn(tween(420)) + scaleIn(initialScale = 1.03f, animationSpec = tween(420))) togetherWith
                            fadeOut(tween(200))
                    },
                    label = "phase",
                ) { current ->
                    when (current) {
                        Phase.BOOT -> Splash()
                        Phase.CONNECT ->
                            ConnectScreen(
                                defaultDeviceName = models.login.defaultDeviceName,
                                loading = login.loading,
                                error = login.error,
                                onConnect = models.login::login,
                            )
                        Phase.MAIN -> MainShell(models, login.credential?.serverUrl.orEmpty())
                    }
                }
                ToastHost(toaster, bottomOffset = if (phase == Phase.MAIN) 92 else 24)
            }
        }
    }
    LaunchedEffect(timeline.notice) {
        timeline.notice?.let {
            toaster.show(it)
            models.timeline.consumeNotice()
        }
    }
}

@Composable
private fun Splash() {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Txt("Mirror", style = Mirror.type.display, color = Mirror.colors.inkMuted)
    }
}

private val WIDE_SCREEN = 600.dp

/**
 * Only library-backed sessions survive rotation or process death; a frozen
 * search/people snapshot is cheap to reopen and too big to save.
 */
private val ViewerSessionSaver =
    listSaver<ViewerSession?, String>(
        save = { session -> if (session != null && session.fromLibrary) listOf(session.startId) else emptyList() },
        restore = { saved -> saved.firstOrNull()?.let { ViewerSession(fromLibrary = true, startId = it) } },
    )

/** What the viewer is showing: live library items, or a frozen result list. */
private data class ViewerSession(
    val fromLibrary: Boolean,
    val startId: String,
    val snapshot: List<AssetTimelineItem> = emptyList(),
)

/** Measures the floating tab bar and tells every scrolling screen how much bottom room it needs. */
@Composable
private fun MainShell(
    models: MirrorViewModels,
    serverUrl: String,
) {
    var chromeHeightPx by remember { mutableIntStateOf(0) }
    val density = LocalDensity.current
    val inset =
        if (chromeHeightPx == 0) {
            LocalBottomInset.current
        } else {
            with(density) { chromeHeightPx.toDp() } + CHROME_BREATHING_ROOM
        }
    CompositionLocalProvider(LocalBottomInset provides inset) {
        MainShellContent(models, serverUrl, onChromeMeasured = { chromeHeightPx = it })
    }
}

private val CHROME_BREATHING_ROOM = 20.dp

@Composable
@Suppress("LongMethod", "CyclomaticComplexMethod") // The shell is the app's single wiring point.
private fun MainShellContent(
    models: MirrorViewModels,
    serverUrl: String,
    onChromeMeasured: (Int) -> Unit,
) {
    val timeline by models.timeline.state.collectAsStateWithLifecycle()
    val backup by models.backup.state.collectAsStateWithLifecycle()
    val search by models.search.state.collectAsStateWithLifecycle()
    val people by models.people.state.collectAsStateWithLifecycle()
    val trash by models.trash.state.collectAsStateWithLifecycle()
    val toaster = LocalToaster.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current

    var tab by rememberSaveable { mutableStateOf(Tab.LIBRARY) }
    var filter by rememberSaveable { mutableStateOf(LibraryFilter.ALL) }
    val settings by models.preferences.settings.collectAsStateWithLifecycle()
    var columns by rememberSaveable { mutableStateOf(settings.gridColumns) }
    val setColumns: (Int) -> Unit = {
        columns = it
        models.preferences.setGridColumns(it)
    }
    var selection by remember { mutableStateOf(emptySet<String>()) }
    var viewer by rememberSaveable(stateSaver = ViewerSessionSaver) { mutableStateOf<ViewerSession?>(null) }
    val wide =
        with(LocalDensity.current) {
            LocalWindowInfo.current.containerSize.width
                .toDp()
        } >= WIDE_SCREEN
    val gridColumns = if (wide) columns * 2 else columns
    var showTrash by rememberSaveable { mutableStateOf(false) }
    var showSettings by rememberSaveable { mutableStateOf(false) }

    val libraryGrid = rememberLazyGridState()
    val searchGrid = rememberLazyGridState()
    val peopleGrid = rememberLazyGridState()
    val vaultList = rememberLazyListState()
    val heroBounds = remember { HeroBounds() }
    val tabs = rememberSaveableStateHolder()

    // One library: the vault's items plus whatever is on this device, de-duplicated.
    val localRows by models.backup.localLibrary.collectAsStateWithLifecycle()
    val remoteConfirmed =
        timeline.credential != null && !timeline.stale && !timeline.offline && !timeline.loadingInitial
    val remoteComplete = remoteConfirmed && timeline.nextCursor == null
    val library =
        remember(timeline.items, localRows, remoteConfirmed, remoteComplete) {
            mergeLibrary(timeline.items, localRows, remoteConfirmed, remoteComplete)
        }
    val visible =
        remember(library, filter) {
            when (filter) {
                LibraryFilter.ALL -> library
                LibraryFilter.FAVORITES -> library.filter { it.isFavorite }
                LibraryFilter.VIDEOS -> library.filter { it.isVideo }
            }
        }
    val entries = remember(visible) { buildGridEntries(visible) }

    val permissionLauncher =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
            models.backup.refresh()
        }

    // New uploads should show up without the user having to refresh.
    LaunchedEffect(backup.counts.verified) {
        if (backup.counts.verified > 0) models.timeline.refreshQuietly()
    }

    LaunchedEffect(tab) {
        if (tab == Tab.PEOPLE && !people.loaded) models.people.refresh()
        if (tab != Tab.LIBRARY) selection = emptySet()
    }

    /** Dismisses any open keyboard first, so Back closes the viewer rather than the IME. */
    fun openViewer(session: ViewerSession) {
        focusManager.clearFocus(force = true)
        keyboard?.hide()
        viewer = session
    }

    fun thumb(assetId: String): String? = models.people.thumbnailUrl(assetId)

    fun preview(assetId: String): String? = thumb(assetId)?.replace("/derivatives/thumbnail", "/derivatives/preview")

    /** Photos that haven't reached the vault yet can't be favorited, shared or trashed there. */
    fun blockedByDeviceOnly(ids: Set<String>): Boolean {
        val blocked = ids.any { it.startsWith(LOCAL_ID_PREFIX) }
        if (blocked) toaster.show("Back up this photo first — it isn't in your vault yet")
        return blocked
    }

    fun share(
        assetId: String,
        isVideo: Boolean = false,
    ) {
        if (blockedByDeviceOnly(setOf(assetId))) return
        scope.launch {
            models.timeline
                .shareLink(assetId)
                .onSuccess { url ->
                    val send =
                        Intent(Intent.ACTION_SEND).apply {
                            type = "text/plain"
                            putExtra(Intent.EXTRA_TEXT, url)
                        }
                    val title = if (isVideo) "Share video preview" else "Share photo link"
                    context.startActivity(Intent.createChooser(send, title))
                    toaster.show(
                        if (isVideo) {
                            "Shares a still preview · expires in ${settings.shareExpiry.label}"
                        } else {
                            "Private link · expires in ${settings.shareExpiry.label}"
                        },
                    )
                }.onFailure { toaster.show("Couldn't create a share link") }
        }
    }

    fun trashAssets(ids: Set<String>) {
        if (blockedByDeviceOnly(ids)) return
        viewer = viewer?.let { session -> session.copy(snapshot = session.snapshot.filterNot { it.assetId in ids }) }
        models.timeline.trash(ids) { trashed ->
            toaster.show(
                if (trashed.size == 1) "Moved to trash" else "Moved ${trashed.size} to trash",
                "Undo",
            ) { models.timeline.restore(trashed) }
        }
    }

    fun favorite(
        ids: Set<String>,
        value: Boolean,
    ) {
        if (blockedByDeviceOnly(ids)) return
        val marker =
            if (value) {
                java.time.Instant
                    .now()
                    .toString()
            } else {
                null
            }
        viewer =
            viewer?.let { session ->
                session.copy(
                    snapshot = session.snapshot.map { if (it.assetId in ids) it.copy(favoriteAt = marker) else it },
                )
            }
        models.timeline.setFavorite(ids, value)
    }

    val gridCallbacks =
        GridCallbacks(
            thumbUrl = ::thumb,
            onOpen = { openViewer(ViewerSession(fromLibrary = true, startId = it.assetId)) },
            onLongPress = { selection = selection + it.assetId },
            onToggle = { item ->
                selection = if (item.assetId in selection) selection - item.assetId else selection + item.assetId
            },
        )

    BackHandler(enabled = selection.isNotEmpty() && viewer == null) { selection = emptySet() }
    BackHandler(
        enabled = tab != Tab.LIBRARY && viewer == null && !showTrash && !showSettings && people.openPersonId == null,
    ) {
        tab = Tab.LIBRARY
    }

    Box(Modifier.fillMaxSize()) {
        AnimatedContent(
            targetState = tab,
            transitionSpec = { fadeIn(tween(240, delayMillis = 60)) togetherWith fadeOut(tween(120)) },
            label = "tabs",
        ) { current ->
            tabs.SaveableStateProvider(current.name) {
                when (current) {
                    Tab.LIBRARY ->
                        LibraryScreen(
                            timeline = timeline,
                            hasItems = library.isNotEmpty(),
                            visible = visible,
                            entries = entries,
                            backup = backup.counts,
                            filter = filter,
                            columns = gridColumns,
                            selection = selection,
                            gridState = libraryGrid,
                            heroBounds = heroBounds,
                            callbacks =
                                LibraryCallbacks(
                                    grid = gridCallbacks,
                                    onColumnsChange = setColumns,
                                    onFilterChange = { filter = it },
                                    onLoadNext = models.timeline::loadNext,
                                    onRefresh = models.timeline::refresh,
                                    onOpenVault = { tab = Tab.VAULT },
                                    onClearSelection = { selection = emptySet() },
                                ),
                        )
                    Tab.SEARCH ->
                        SearchScreen(
                            state = search,
                            columns = gridColumns,
                            gridState = searchGrid,
                            heroBounds = heroBounds,
                            callbacks =
                                SearchCallbacks(
                                    onQuery = models.search::setQuery,
                                    onMode = models.search::setMode,
                                    onSubmit = models.search::submit,
                                    onRecent = models.search::useRecent,
                                    onColumnsChange = { columns = it },
                                    grid =
                                        gridCallbacks.copy(
                                            onOpen = {
                                                openViewer(ViewerSession(false, it.assetId, search.results))
                                            },
                                            onLongPress = {},
                                        ),
                                ),
                        )
                    Tab.PEOPLE ->
                        PeopleScreen(
                            state = people,
                            gridState = peopleGrid,
                            callbacks =
                                PeopleCallbacks(
                                    chipUrl = models.people::chipUrl,
                                    thumbUrl = ::thumb,
                                    onOpenPerson = models.people::openPerson,
                                    onRename = models.people::rename,
                                    onHide = models.people::hide,
                                    onOpenAsset = { list, item ->
                                        val known = library.associateBy { it.assetId }
                                        val merged = list.map { known[it.assetId] ?: it }
                                        openViewer(ViewerSession(false, item.assetId, merged))
                                    },
                                    onRefresh = models.people::refresh,
                                ),
                        )
                    Tab.VAULT ->
                        VaultScreen(
                            backup = backup,
                            settings = settings,
                            serverUrl = serverUrl,
                            listState = vaultList,
                            callbacks =
                                VaultCallbacks(
                                    backup =
                                        BackupActions(
                                            requestPermission = {
                                                permissionLauncher.launch(models.backup.requiredPermissions())
                                            },
                                            selectFolder = models.backup::setFolderSelected,
                                            setWifiOnly = models.backup::setWifiOnly,
                                            runNow = {
                                                models.backup.runNow()
                                                toaster.show("Backup started")
                                            },
                                        ),
                                    onChargingOnly = {
                                        models.preferences.setChargingOnly(it)
                                        models.backup.preferencesChanged()
                                    },
                                    onBackupVideos = {
                                        models.preferences.setBackupVideos(it)
                                        models.backup.preferencesChanged()
                                    },
                                    onOpenTrash = { showTrash = true },
                                    onOpenSettings = { showSettings = true },
                                    onDisconnect = models.login::logout,
                                ),
                        )
                }
            }
        }

        Box(
            Modifier
                .fillMaxWidth()
                .windowInsetsTopHeight(WindowInsets.statusBars)
                .background(
                    Brush.verticalGradient(
                        listOf(Mirror.colors.canvas, Mirror.colors.canvas.copy(alpha = 0.97f)),
                    ),
                ),
        )

        val chromeVisible = viewer == null && !showTrash && !showSettings && people.openPersonId == null
        AnimatedVisibility(
            visible = chromeVisible && selection.isEmpty(),
            enter = fadeIn() + slideInVertically { it },
            exit = fadeOut() + slideOutVertically { it },
            modifier = Modifier.align(Alignment.BottomCenter),
        ) {
            NavPill(
                modifier = Modifier.onSizeChanged { onChromeMeasured(it.height) },
                selected = tab,
                onSelect = { next ->
                    if (next == tab) {
                        scope.launch {
                            when (next) {
                                Tab.LIBRARY -> libraryGrid.animateScrollToItem(0)
                                Tab.SEARCH -> searchGrid.animateScrollToItem(0)
                                Tab.PEOPLE -> peopleGrid.animateScrollToItem(0)
                                Tab.VAULT -> vaultList.animateScrollToItem(0)
                            }
                        }
                        if (next == Tab.LIBRARY) models.timeline.refresh()
                        if (next == Tab.PEOPLE) models.people.refresh()
                    } else {
                        tab = next
                    }
                },
            )
        }
        AnimatedVisibility(
            visible = chromeVisible && selection.isNotEmpty(),
            enter = fadeIn() + slideInVertically { it } + scaleIn(initialScale = 0.9f),
            exit = fadeOut() + slideOutVertically { it },
            modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 16.dp),
        ) {
            val chosen = library.filter { it.assetId in selection }
            val allFavorite = chosen.isNotEmpty() && chosen.all { it.isFavorite }
            SelectionActions(
                allFavorite = allFavorite,
                onFavorite = {
                    favorite(selection, !allFavorite)
                    toaster.show(if (allFavorite) "Removed from favorites" else "Added to favorites")
                    selection = emptySet()
                },
                onShare =
                    if (selection.size == 1) {
                        {
                            val id = selection.first()
                            share(id, library.firstOrNull { it.assetId == id }?.isVideo == true)
                        }
                    } else {
                        null
                    },
                onTrash = {
                    trashAssets(selection)
                    selection = emptySet()
                },
            )
        }

        AnimatedVisibility(
            visible = showTrash,
            enter = slideInHorizontally { it } + fadeIn(),
            exit = slideOutHorizontally { it } + fadeOut(),
        ) {
            TrashScreen(
                state = trash,
                callbacks =
                    TrashCallbacks(
                        thumbUrl = models.trash::thumbnailUrl,
                        onRefresh = models.trash::refresh,
                        onLoadNext = models.trash::loadNext,
                        onRestore = { ids, done ->
                            models.trash.restore(ids) { count ->
                                done(count)
                                models.timeline.refresh()
                            }
                        },
                        onPurge = models.trash::purge,
                        onClose = { showTrash = false },
                    ),
            )
        }

        AnimatedVisibility(
            visible = showSettings,
            enter = slideInHorizontally { it } + fadeIn(),
            exit = slideOutHorizontally { it } + fadeOut(),
        ) {
            SettingsScreen(
                settings = settings,
                serverUrl = serverUrl,
                callbacks =
                    SettingsCallbacks(
                        onTheme = models.preferences::setTheme,
                        onShareExpiry = models.preferences::setShareExpiry,
                        onHidePreviews = models.preferences::setHidePreviews,
                        onClose = { showSettings = false },
                    ),
            )
        }

        viewer?.let { session ->
            val items = if (session.fromLibrary) visible else session.snapshot
            Viewer(
                items = items,
                startId = session.startId,
                heroBounds = heroBounds,
                actions =
                    ViewerActions(
                        thumbUrl = ::thumb,
                        previewUrl = ::preview,
                        onFavorite = { item, value -> favorite(setOf(item.assetId), value) },
                        onShare = { share(it.assetId, it.isVideo) },
                        onTrash = { trashAssets(setOf(it.assetId)) },
                        originalUrl = models.people::originalUrl,
                        onPageChanged = { assetId ->
                            if (session.fromLibrary) {
                                // Remember the page so a rotation reopens where the user was.
                                if (session.startId != assetId) viewer = session.copy(startId = assetId)
                                val index = entries.indexOfFirst { it.key == assetId } + 1
                                val shown = libraryGrid.layoutInfo.visibleItemsInfo.any { it.key == assetId }
                                if (index > 0 && !shown) scope.launch { libraryGrid.scrollToItem(index) }
                            }
                        },
                        onClose = { viewer = null },
                    ),
            )
        }
    }
}

@Composable
private fun NavPill(
    selected: Tab,
    onSelect: (Tab) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = Mirror.colors
    Row(
        modifier
            .navigationBarsPadding()
            .padding(bottom = 14.dp)
            .shadow(
                18.dp,
                Pill,
                ambientColor = colors.scrim.copy(alpha = 0.25f),
                spotColor = colors.scrim.copy(alpha = 0.35f),
            ).clip(Pill)
            .background(colors.surface)
            .border(1.dp, colors.line, Pill)
            .padding(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Tab.entries.forEach { tab ->
            val active = tab == selected
            val background by animateColorAsState(if (active) colors.ink else Color.Transparent, label = "tab-bg")
            val tint by animateColorAsState(if (active) colors.canvas else colors.inkMuted, label = "tab-tint")
            Row(
                Modifier
                    .clip(Pill)
                    .background(background)
                    .tappable(pressedScale = 0.9f) { onSelect(tab) }
                    .animateContentSize(spring(dampingRatio = 0.75f, stiffness = Spring.StiffnessMediumLow))
                    .padding(horizontal = if (active) 18.dp else 16.dp, vertical = 13.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Glyph(tab.glyph, tint = tint, size = 21.dp, contentDescription = tab.label)
                AnimatedVisibility(
                    visible = active,
                    enter = fadeIn() + expandHorizontally(),
                    exit = fadeOut() + shrinkHorizontally(),
                ) {
                    Row {
                        Spacer(Modifier.width(8.dp))
                        Txt(tab.label, style = Mirror.type.label, color = tint)
                    }
                }
            }
        }
    }
}

/** Keeps status/navigation bar icons legible when the in-app theme overrides the system one. */
@Composable
private fun SystemBarIcons(dark: Boolean) {
    val view = LocalView.current
    SideEffect {
        val window = (view.context as? Activity)?.window ?: return@SideEffect
        val controller = WindowCompat.getInsetsController(window, view)
        controller.isAppearanceLightStatusBars = !dark
        controller.isAppearanceLightNavigationBars = !dark
    }
}
