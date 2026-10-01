package app.mirror.vault.timeline

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.library.LibraryRepository
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.MirrorTransportException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class TimelineUiState(
    val credential: DeviceCredential? = null,
    val items: List<AssetTimelineItem> = emptyList(),
    val nextCursor: String? = null,
    val loadingInitial: Boolean = false,
    val loadingNext: Boolean = false,
    val error: String? = null,
    val selectedAssetId: String? = null,
    val notice: String? = null,
    /** Showing remembered items; the server hasn't confirmed them this session. */
    val stale: Boolean = false,
    /** The last attempt to reach the vault failed at the network level. */
    val offline: Boolean = false,
)

class TimelineViewModel(
    private val repository: TimelineRepository,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
    private val library: LibraryRepository? = null,
    private val store: TimelineStore? = null,
) : ViewModel() {
    private val mutableState = MutableStateFlow(TimelineUiState())
    val state: StateFlow<TimelineUiState> = mutableState.asStateFlow()

    fun setCredential(credential: DeviceCredential?) {
        if (credential == null) {
            // Not a sign-out: the credential is also null while the stored login loads at startup.
            mutableState.value = TimelineUiState()
            return
        }
        val current = mutableState.value
        if (current.credential == credential && current.items.isNotEmpty()) {
            return
        }
        mutableState.value =
            TimelineUiState(
                credential = credential,
                loadingInitial = true,
            )
        remember(credential)
        loadPage(
            credential = credential,
            cursor = null,
            replace = true,
        )
    }

    /** Seeds the list from disk so the library is useful before (or without) the network. */
    private fun remember(credential: DeviceCredential) {
        val store = store ?: return
        viewModelScope.launch {
            val cached = withContext(ioDispatcher) { store.read(credential.serverUrl) }
            if (cached.isNotEmpty()) {
                mutableState.update { state ->
                    if (state.credential == credential && state.items.isEmpty()) {
                        state.copy(items = cached, stale = true)
                    } else {
                        state
                    }
                }
            }
        }
    }

    fun refresh() {
        val credential = mutableState.value.credential ?: return
        mutableState.update {
            it.copy(
                loadingInitial = true,
                loadingNext = false,
                error = null,
                selectedAssetId = null,
            )
        }
        loadPage(
            credential = credential,
            cursor = null,
            replace = true,
        )
    }

    /**
     * Fetches the newest page and merges it in without resetting scroll, paging
     * or selection, so fresh uploads appear in place.
     */
    fun refreshQuietly() {
        val current = mutableState.value
        val credential = current.credential ?: return
        if (current.loadingInitial) return
        if (current.items.isEmpty()) {
            // Nothing to merge into (first load failed, or the vault was empty): do a full load.
            refresh()
            return
        }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) { repository.loadPage(credential = credential, cursor = null) }
            }.onSuccess { page ->
                mutableState.update { state ->
                    if (state.credential != credential) {
                        state
                    } else {
                        val latest = page.items.associateBy { it.assetId }
                        val known = state.items.mapTo(HashSet()) { it.assetId }
                        // Remembered items may be gone server-side, so the first fresh page replaces them.
                        val replaceAll = state.stale || state.items.isEmpty()
                        state.copy(
                            items =
                                if (replaceAll) {
                                    page.items
                                } else {
                                    page.items.filter { it.assetId !in known } + state.items.map { latest[it.assetId] ?: it }
                                },
                            nextCursor = if (replaceAll) page.nextCursor else state.nextCursor,
                            stale = false,
                            offline = false,
                            error = null,
                        )
                    }
                }
                persist(credential)
            }.onFailure { error ->
                if (error is MirrorTransportException) {
                    mutableState.update { state -> if (state.credential == credential) state.copy(offline = true) else state }
                }
            }
        }
    }

    private fun persist(credential: DeviceCredential) {
        val store = store ?: return
        val items = mutableState.value.items
        viewModelScope.launch(ioDispatcher) { store.write(credential.serverUrl, items) }
    }

    fun loadNext() {
        val current = mutableState.value
        val credential = current.credential
        val cursor = current.nextCursor
        if (credential == null || cursor == null) {
            return
        }
        if (current.loadingInitial || current.loadingNext) {
            return
        }
        mutableState.update {
            it.copy(
                loadingNext = true,
                error = null,
            )
        }
        loadPage(
            credential = credential,
            cursor = cursor,
            replace = false,
        )
    }

    fun open(assetId: String) {
        if (mutableState.value.items.any { it.assetId == assetId }) {
            mutableState.update { it.copy(selectedAssetId = assetId) }
        }
    }

    fun close() {
        mutableState.update { it.copy(selectedAssetId = null) }
    }

    fun select(offset: Int) {
        val current = mutableState.value
        val selected = current.selectedAssetId ?: return
        val index = current.items.indexOfFirst { it.assetId == selected }
        val next = current.items.getOrNull(index + offset) ?: return
        mutableState.update { it.copy(selectedAssetId = next.assetId) }
    }

    /** Optimistically flips favorite markers, reverting any that fail. */
    fun setFavorite(
        assetIds: Set<String>,
        favorite: Boolean,
    ) {
        val credential = mutableState.value.credential ?: return
        val library = library ?: return
        val marker =
            if (favorite) {
                java.time.Instant
                    .now()
                    .toString()
            } else {
                null
            }
        val previous =
            mutableState.value.items
                .filter { it.assetId in assetIds }
                .associateBy { it.assetId }
        mutableState.update { state ->
            state.copy(
                items =
                    state.items.map { item ->
                        if (item.assetId in assetIds) item.copy(favoriteAt = marker) else item
                    },
            )
        }
        viewModelScope.launch {
            val failed =
                assetIds.filterNot { assetId ->
                    runCatching {
                        withContext(ioDispatcher) { library.setFavorite(credential, assetId, favorite) }
                    }.isSuccess
                }
            if (failed.isNotEmpty()) {
                mutableState.update { state ->
                    state.copy(
                        items =
                            state.items.map { item ->
                                if (item.assetId in failed) previous[item.assetId] ?: item else item
                            },
                        notice = "Couldn't update ${failed.size} favorite${if (failed.size == 1) "" else "s"}",
                    )
                }
            }
        }
    }

    /**
     * Moves assets to trash. Loaded grid items vanish immediately; failures
     * are put back in their original order. [onTrashed] receives the IDs the
     * server accepted, so callers can offer undo.
     */
    fun trash(
        assetIds: Set<String>,
        onTrashed: (Set<String>) -> Unit = {},
    ) {
        val credential = mutableState.value.credential ?: return
        val library = library ?: return
        val before = mutableState.value.items
        mutableState.update { state ->
            val selected = state.selectedAssetId
            val nextSelection =
                if (selected != null && selected in assetIds) {
                    val index = state.items.indexOfFirst { it.assetId == selected }
                    val survivors = state.items.filterNot { it.assetId in assetIds }
                    survivors.getOrNull(index.coerceAtMost(survivors.lastIndex))?.assetId
                } else {
                    selected
                }
            state.copy(
                items = state.items.filterNot { it.assetId in assetIds },
                selectedAssetId = nextSelection,
            )
        }
        viewModelScope.launch {
            val failed =
                assetIds.filterNotTo(mutableSetOf()) { assetId ->
                    runCatching {
                        withContext(ioDispatcher) { library.trash(credential, assetId) }
                    }.isSuccess
                }
            if (failed.isNotEmpty()) {
                mutableState.update { state ->
                    val current = state.items.mapTo(mutableSetOf()) { it.assetId }
                    state.copy(
                        items = before.filter { it.assetId in current || it.assetId in failed },
                        notice = "Couldn't move ${failed.size} to trash",
                    )
                }
            }
            val trashed = assetIds - failed
            if (trashed.isNotEmpty()) onTrashed(trashed)
        }
    }

    /** Undo for [trash]: restores server-side, then reloads the first page. */
    fun restore(assetIds: Set<String>) {
        val credential = mutableState.value.credential ?: return
        val library = library ?: return
        viewModelScope.launch {
            assetIds.forEach { assetId ->
                runCatching {
                    withContext(ioDispatcher) { library.restore(credential, assetId) }
                }
            }
            refresh()
        }
    }

    suspend fun shareLink(assetId: String): Result<String> =
        runCatching {
            val credential = checkNotNull(mutableState.value.credential) { "Not connected" }
            val library = checkNotNull(library) { "Sharing unavailable" }
            withContext(ioDispatcher) { library.shareLink(credential, assetId) }
        }

    fun consumeNotice() {
        mutableState.update { it.copy(notice = null) }
    }

    fun derivativeUrl(
        asset: AssetTimelineItem,
        kind: TimelineDerivativeKind,
    ): String? =
        mutableState.value.credential?.let { credential ->
            runCatching {
                repository.derivativeUrl(
                    credential = credential,
                    asset = asset,
                    kind = kind,
                )
            }.getOrNull()
        }

    fun authorizationHeader(): String? = mutableState.value.credential?.let(repository::authorizationHeader)

    private fun loadPage(
        credential: DeviceCredential,
        cursor: String?,
        replace: Boolean,
    ) {
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) {
                    repository.loadPage(
                        credential = credential,
                        cursor = cursor,
                    )
                }
            }.onSuccess { page ->
                mutableState.update { current ->
                    if (current.credential != credential) {
                        current
                    } else {
                        current.copy(
                            items = if (replace) page.items else current.items + page.items,
                            nextCursor = page.nextCursor,
                            loadingInitial = false,
                            loadingNext = false,
                            error = null,
                            stale = false,
                            offline = false,
                        )
                    }
                }
                persist(credential)
            }.onFailure { error ->
                mutableState.update { current ->
                    if (current.credential != credential) {
                        current
                    } else {
                        current.copy(
                            loadingInitial = false,
                            loadingNext = false,
                            error = error.message ?: "Timeline unavailable",
                            offline = error is MirrorTransportException,
                        )
                    }
                }
            }
        }
    }
}

class TimelineViewModelFactory(
    private val repository: TimelineRepository,
    private val library: LibraryRepository? = null,
    private val store: TimelineStore? = null,
) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        require(modelClass.isAssignableFrom(TimelineViewModel::class.java))
        return TimelineViewModel(repository, library = library, store = store) as T
    }
}
