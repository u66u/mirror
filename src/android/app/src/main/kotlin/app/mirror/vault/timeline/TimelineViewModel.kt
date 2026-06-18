package app.mirror.vault.timeline

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.AssetTimelineItem
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
)

class TimelineViewModel(
    private val repository: TimelineRepository,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
) : ViewModel() {
    private val mutableState = MutableStateFlow(TimelineUiState())
    val state: StateFlow<TimelineUiState> = mutableState.asStateFlow()

    fun setCredential(credential: DeviceCredential?) {
        if (credential == null) {
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
        loadPage(
            credential = credential,
            cursor = null,
            replace = true,
        )
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
                        )
                    }
                }
            }.onFailure { error ->
                mutableState.update { current ->
                    if (current.credential != credential) {
                        current
                    } else {
                        current.copy(
                            loadingInitial = false,
                            loadingNext = false,
                            error = error.message ?: "Timeline unavailable",
                        )
                    }
                }
            }
        }
    }
}

class TimelineViewModelFactory(
    private val repository: TimelineRepository,
) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        require(modelClass.isAssignableFrom(TimelineViewModel::class.java))
        return TimelineViewModel(repository) as T
    }
}
