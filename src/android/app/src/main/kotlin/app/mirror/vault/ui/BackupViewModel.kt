package app.mirror.vault.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.mirror.vault.backup.BackupCounts
import app.mirror.vault.backup.BackupFolder
import app.mirror.vault.backup.BackupRepository
import app.mirror.vault.backup.MediaPermissionState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class BackupUiState(
    val permission: MediaPermissionState = MediaPermissionState.DENIED,
    val folders: List<BackupFolder> = emptyList(),
    val counts: BackupCounts = BackupCounts(),
    val wifiOnly: Boolean = true,
    val refreshing: Boolean = false,
    val error: String? = null,
)

class BackupViewModel(
    private val repository: BackupRepository,
) : ViewModel() {
    private val mutableState =
        MutableStateFlow(
            BackupUiState(permission = repository.permissionState()),
        )
    val state: StateFlow<BackupUiState> = mutableState.asStateFlow()

    init {
        viewModelScope.launch {
            repository.folders.collect { folders ->
                mutableState.update { it.copy(folders = folders) }
            }
        }
        viewModelScope.launch {
            repository.counts.collect { counts ->
                mutableState.update { it.copy(counts = counts) }
            }
        }
        viewModelScope.launch {
            repository.wifiOnly.collect { wifiOnly ->
                mutableState.update { it.copy(wifiOnly = wifiOnly) }
            }
        }
    }

    fun requiredPermissions(): Array<String> = repository.requiredPermissions()

    fun refresh() {
        mutableState.update {
            it.copy(
                permission = repository.permissionState(),
                refreshing = true,
                error = null,
            )
        }
        ioAction {
            repository.refreshFolders()
            mutableState.update {
                it.copy(
                    permission = repository.permissionState(),
                    refreshing = false,
                )
            }
        }
    }

    fun setFolderSelected(
        bucketId: String,
        selected: Boolean,
    ) = ioAction {
        repository.setFolderSelected(bucketId, selected)
    }

    fun setWifiOnly(wifiOnly: Boolean) =
        ioAction {
            repository.setWifiOnly(wifiOnly)
        }

    fun runNow() =
        ioAction {
            repository.runNow()
        }

    private fun ioAction(action: suspend () -> Unit) {
        viewModelScope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    action()
                }
            }.onFailure { error ->
                mutableState.update {
                    it.copy(
                        refreshing = false,
                        error = error.message ?: "Backup operation failed",
                    )
                }
            }
        }
    }
}

class BackupViewModelFactory(
    private val repository: BackupRepository,
) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        require(modelClass.isAssignableFrom(BackupViewModel::class.java))
        return BackupViewModel(repository) as T
    }
}
