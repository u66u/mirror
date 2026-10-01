package app.mirror.vault.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.AssetTimelineItem
import app.mirror.vault.network.FaceItem
import app.mirror.vault.network.MirrorApiException
import app.mirror.vault.network.PersonSummary
import app.mirror.vault.network.SearchMode
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private const val SEARCH_DEBOUNCE_MILLIS = 450L

// Meaning search runs the image/text model per query, so it waits for a couple of
// characters; file-name search is a cheap substring match and starts at one.
private const val MIN_SEMANTIC_QUERY_LENGTH = 2
private const val MIN_FILENAME_QUERY_LENGTH = 1

fun minQueryLength(mode: SearchMode): Int =
    when (mode) {
        SearchMode.SEMANTIC -> MIN_SEMANTIC_QUERY_LENGTH
        SearchMode.FILENAME -> MIN_FILENAME_QUERY_LENGTH
    }

private const val RECENT_LIMIT = 6

data class SearchUiState(
    val query: String = "",
    val mode: SearchMode = SearchMode.SEMANTIC,
    val results: List<AssetTimelineItem> = emptyList(),
    val searching: Boolean = false,
    val searched: Boolean = false,
    val semanticUnavailable: Boolean = false,
    val error: String? = null,
    val recent: List<String> = emptyList(),
)

class SearchViewModel(
    private val library: LibraryRepository,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = mutableState.asStateFlow()
    private var credential: DeviceCredential? = null
    private var pending: Job? = null

    fun setCredential(value: DeviceCredential?) {
        if (value == credential) return
        credential = value
        mutableState.value = SearchUiState()
    }

    fun setQuery(query: String) {
        mutableState.update { it.copy(query = query) }
        schedule(SEARCH_DEBOUNCE_MILLIS)
    }

    fun setMode(mode: SearchMode) {
        mutableState.update { it.copy(mode = mode) }
        schedule(0)
    }

    fun submit() = schedule(0, remember = true)

    fun useRecent(query: String) {
        mutableState.update { it.copy(query = query) }
        schedule(0)
    }

    private fun schedule(
        delayMillis: Long,
        remember: Boolean = false,
    ) {
        pending?.cancel()
        val query = mutableState.value.query.trim()
        if (query.length < minQueryLength(mutableState.value.mode)) {
            mutableState.update {
                it.copy(results = emptyList(), searching = false, searched = false, error = null)
            }
            return
        }
        val credential = credential ?: return
        pending =
            viewModelScope.launch {
                delay(delayMillis)
                mutableState.update { it.copy(searching = true, error = null) }
                val requested = mutableState.value.mode
                val outcome =
                    runCatching {
                        withContext(ioDispatcher) { library.search(credential, query, requested) }
                    }
                val unavailable =
                    (outcome.exceptionOrNull() as? MirrorApiException)?.status == HTTP_UNAVAILABLE &&
                        requested == SearchMode.SEMANTIC
                val finalOutcome =
                    if (unavailable) {
                        runCatching {
                            withContext(ioDispatcher) { library.search(credential, query, SearchMode.FILENAME) }
                        }
                    } else {
                        outcome
                    }
                mutableState.update { state ->
                    state.copy(
                        results = finalOutcome.getOrDefault(emptyList()),
                        searching = false,
                        searched = true,
                        semanticUnavailable = unavailable || state.semanticUnavailable,
                        error = finalOutcome.exceptionOrNull()?.let { it.message ?: "Search failed" },
                        recent =
                            if (remember) {
                                (listOf(query) + state.recent.filterNot { it.equals(query, true) }).take(RECENT_LIMIT)
                            } else {
                                state.recent
                            },
                    )
                }
            }
    }

    private companion object {
        const val HTTP_UNAVAILABLE = 503
    }
}

data class PersonCard(
    val person: PersonSummary,
    val coverFaceId: String?,
)

data class PeopleUiState(
    val loading: Boolean = false,
    val loaded: Boolean = false,
    val people: List<PersonCard> = emptyList(),
    val unassigned: List<FaceItem> = emptyList(),
    val openPersonId: String? = null,
    val personFaces: List<FaceItem> = emptyList(),
    val loadingFaces: Boolean = false,
    val error: String? = null,
)

class PeopleViewModel(
    private val library: LibraryRepository,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow(PeopleUiState())
    val state: StateFlow<PeopleUiState> = mutableState.asStateFlow()
    private var credential: DeviceCredential? = null

    fun setCredential(value: DeviceCredential?) {
        if (value == credential) return
        credential = value
        mutableState.value = PeopleUiState()
    }

    fun chipUrl(faceId: String): String? = credential?.let { library.faceChipUrl(it, faceId) }

    fun thumbnailUrl(assetId: String): String? = credential?.let { library.thumbnailUrl(it, assetId) }

    fun originalUrl(assetId: String): String? = credential?.let { library.originalUrl(it, assetId) }

    fun refresh() {
        val credential = credential ?: return
        mutableState.update { it.copy(loading = true, error = null) }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) {
                    val people = library.people(credential).sortedWith(peopleOrder)
                    val cards =
                        people
                            .map { person ->
                                async {
                                    val cover =
                                        runCatching {
                                            library.personFaces(credential, person.personId, limit = 1)
                                        }.getOrNull()?.firstOrNull()
                                    PersonCard(person, cover?.faceId)
                                }
                            }.awaitAll()
                    val unassigned = runCatching { library.unassignedFaces(credential) }.getOrDefault(emptyList())
                    cards to unassigned
                }
            }.onSuccess { (cards, unassigned) ->
                mutableState.update {
                    it.copy(loading = false, loaded = true, people = cards, unassigned = unassigned)
                }
            }.onFailure { error ->
                mutableState.update {
                    it.copy(loading = false, loaded = true, error = error.message ?: "People unavailable")
                }
            }
        }
    }

    fun openPerson(personId: String?) {
        mutableState.update { it.copy(openPersonId = personId, personFaces = emptyList()) }
        val credential = credential ?: return
        personId ?: return
        mutableState.update { it.copy(loadingFaces = true) }
        viewModelScope.launch {
            val faces =
                runCatching {
                    withContext(ioDispatcher) { library.personFaces(credential, personId) }
                }
            mutableState.update { state ->
                if (state.openPersonId != personId) {
                    state
                } else {
                    state.copy(
                        personFaces = faces.getOrDefault(emptyList()).distinctBy { it.assetId },
                        loadingFaces = false,
                        error = faces.exceptionOrNull()?.message,
                    )
                }
            }
        }
    }

    fun rename(
        personId: String,
        name: String,
    ) {
        val credential = credential ?: return
        val trimmed = name.trim()
        if (trimmed.isEmpty()) return
        mutableState.update { state ->
            state.copy(
                people =
                    state.people.map { card ->
                        if (card.person.personId == personId) {
                            card.copy(person = card.person.copy(displayName = trimmed))
                        } else {
                            card
                        }
                    },
            )
        }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) { library.renamePerson(credential, personId, trimmed) }
            }.onFailure { refresh() }
        }
    }

    fun hide(personId: String) {
        val credential = credential ?: return
        mutableState.update { state ->
            state.copy(
                people = state.people.filterNot { it.person.personId == personId },
                openPersonId = null,
            )
        }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) { library.hidePerson(credential, personId) }
            }.onFailure { refresh() }
        }
    }

    private companion object {
        val peopleOrder =
            compareBy<PersonSummary> { it.displayName == null }
                .thenByDescending { it.faceCount }
                .thenBy { it.displayName?.lowercase() }
    }
}

data class TrashUiState(
    val items: List<AssetTimelineItem> = emptyList(),
    val nextCursor: String? = null,
    val loading: Boolean = false,
    val loaded: Boolean = false,
    val busy: Boolean = false,
    val error: String? = null,
)

class TrashViewModel(
    private val library: LibraryRepository,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow(TrashUiState())
    val state: StateFlow<TrashUiState> = mutableState.asStateFlow()
    private var credential: DeviceCredential? = null

    fun setCredential(value: DeviceCredential?) {
        if (value == credential) return
        credential = value
        mutableState.value = TrashUiState()
    }

    fun thumbnailUrl(assetId: String): String? = credential?.let { library.thumbnailUrl(it, assetId) }

    fun refresh() {
        val credential = credential ?: return
        mutableState.update { it.copy(loading = true, error = null) }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) { library.trashPage(credential, null) }
            }.onSuccess { page ->
                mutableState.update {
                    it.copy(items = page.items, nextCursor = page.nextCursor, loading = false, loaded = true)
                }
            }.onFailure { error ->
                mutableState.update {
                    it.copy(loading = false, loaded = true, error = error.message ?: "Trash unavailable")
                }
            }
        }
    }

    fun loadNext() {
        val credential = credential
        val cursor = mutableState.value.nextCursor
        if (credential == null || cursor == null || mutableState.value.loading) return
        mutableState.update { it.copy(loading = true) }
        viewModelScope.launch {
            runCatching {
                withContext(ioDispatcher) { library.trashPage(credential, cursor) }
            }.onSuccess { page ->
                mutableState.update {
                    it.copy(items = it.items + page.items, nextCursor = page.nextCursor, loading = false)
                }
            }.onFailure { mutableState.update { it.copy(loading = false) } }
        }
    }

    fun restore(
        assetIds: Set<String>,
        onDone: (Int) -> Unit,
    ) = mutate(assetIds, onDone) { credential, id -> library.restore(credential, id) }

    fun purge(
        assetIds: Set<String>,
        onDone: (Int) -> Unit,
    ) = mutate(assetIds, onDone) { credential, id -> library.purge(credential, id) }

    private fun mutate(
        assetIds: Set<String>,
        onDone: (Int) -> Unit,
        action: suspend (DeviceCredential, String) -> Unit,
    ) {
        val credential = credential ?: return
        mutableState.update { it.copy(busy = true) }
        viewModelScope.launch {
            val done =
                assetIds.filter { id ->
                    runCatching { withContext(ioDispatcher) { action(credential, id) } }.isSuccess
                }
            mutableState.update { state ->
                state.copy(items = state.items.filterNot { it.assetId in done }, busy = false)
            }
            onDone(done.size)
        }
    }
}
