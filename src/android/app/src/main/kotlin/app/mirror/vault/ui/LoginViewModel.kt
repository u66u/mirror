package app.mirror.vault.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import app.mirror.vault.auth.AuthRepository
import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.auth.LogoutResult
import app.mirror.vault.backup.BackupRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class LoginUiState(
    val loading: Boolean = true,
    val credential: DeviceCredential? = null,
    val error: String? = null,
)

class LoginViewModel(
    private val repository: AuthRepository,
    private val backupRepository: BackupRepository,
    val defaultDeviceName: String,
) : ViewModel() {
    private val mutableState = MutableStateFlow(LoginUiState())
    val state: StateFlow<LoginUiState> = mutableState.asStateFlow()

    init {
        viewModelScope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    repository.currentCredential()
                }
            }.onSuccess { credential ->
                mutableState.value = LoginUiState(loading = false, credential = credential)
                if (credential != null) {
                    initializeBackup(credential)
                }
            }.onFailure {
                mutableState.value =
                    LoginUiState(
                        loading = false,
                        error = "Stored login could not be read",
                    )
            }
        }
    }

    fun login(
        serverUrl: String,
        allowInsecurePrivateLan: Boolean,
        password: String,
        deviceName: String,
    ) {
        if (mutableState.value.loading) {
            return
        }
        mutableState.value = mutableState.value.copy(loading = true, error = null)
        viewModelScope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    repository.login(
                        rawServerUrl = serverUrl,
                        allowInsecurePrivateLan = allowInsecurePrivateLan,
                        password = password,
                        deviceName = deviceName,
                    )
                }
            }.onSuccess { credential ->
                mutableState.value = LoginUiState(loading = false, credential = credential)
                initializeBackup(credential)
            }.onFailure { error ->
                mutableState.value =
                    LoginUiState(
                        loading = false,
                        error = error.message ?: "Login failed",
                    )
            }
        }
    }

    fun logout() {
        val credential = mutableState.value.credential ?: return
        mutableState.value = mutableState.value.copy(loading = true, error = null)
        viewModelScope.launch {
            runCatching {
                withContext(Dispatchers.IO) {
                    repository.logout(credential)
                }
            }.onSuccess { result ->
                backupRepository.signedOut()
                mutableState.value =
                    LoginUiState(
                        loading = false,
                        error =
                            if (result == LogoutResult.LOCAL_ONLY) {
                                "Disconnected locally. Revoke this device from the web app."
                            } else {
                                null
                            },
                    )
            }.onFailure { error ->
                mutableState.value =
                    mutableState.value.copy(
                        loading = false,
                        error = error.message ?: "Disconnect failed",
                    )
            }
        }
    }

    private fun initializeBackup(credential: DeviceCredential) {
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                runCatching {
                    backupRepository.initializeAuthenticated(credential)
                }
            }
        }
    }
}

class LoginViewModelFactory(
    private val repository: AuthRepository,
    private val backupRepository: BackupRepository,
    private val defaultDeviceName: String,
) : ViewModelProvider.Factory {
    @Suppress("UNCHECKED_CAST")
    override fun <T : ViewModel> create(modelClass: Class<T>): T {
        require(modelClass.isAssignableFrom(LoginViewModel::class.java))
        return LoginViewModel(repository, backupRepository, defaultDeviceName) as T
    }
}
