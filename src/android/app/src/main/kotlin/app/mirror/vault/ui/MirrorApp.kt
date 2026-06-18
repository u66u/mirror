package app.mirror.vault.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.mirror.vault.timeline.TimelineUiState
import app.mirror.vault.timeline.TimelineViewModel

private val VaultBackground = Color(0xFFFAFAF8)
private val VaultInk = Color(0xFF1A1D1B)
private val VaultGreen = Color(0xFF246B4A)

private data class LoginFormValues(
    val serverUrl: String = "",
    val deviceName: String,
    val password: String = "",
)

private data class ConnectedUiState(
    val serverUrl: String,
    val loading: Boolean,
    val error: String?,
    val backup: BackupUiState,
    val timeline: TimelineUiState,
)

private data class ConnectedActions(
    val logout: () -> Unit,
    val backup: BackupActions,
    val timeline: TimelineActions,
)

private enum class ConnectedTab {
    PHOTOS,
    BACKUP,
}

@Composable
fun MirrorApp(
    viewModel: LoginViewModel,
    backupViewModel: BackupViewModel,
    timelineViewModel: TimelineViewModel,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val backupState by backupViewModel.state.collectAsStateWithLifecycle()
    val timelineState by timelineViewModel.state.collectAsStateWithLifecycle()
    val permissionLauncher =
        rememberLauncherForActivityResult(
            ActivityResultContracts.RequestMultiplePermissions(),
        ) {
            backupViewModel.refresh()
        }
    LaunchedEffect(state.credential) {
        timelineViewModel.setCredential(state.credential)
    }
    MaterialTheme(
        colorScheme =
            MaterialTheme.colorScheme.copy(
                background = VaultBackground,
                surface = VaultBackground,
                primary = VaultGreen,
                onBackground = VaultInk,
                onSurface = VaultInk,
            ),
    ) {
        MirrorSurface(
            state = state,
            backupState = backupState,
            timelineState = timelineState,
            defaultDeviceName = viewModel.defaultDeviceName,
            onLogin = viewModel::login,
            connectedActions =
                ConnectedActions(
                    logout = viewModel::logout,
                    backup =
                        BackupActions(
                            requestPermission = {
                                permissionLauncher.launch(
                                    backupViewModel.requiredPermissions(),
                                )
                            },
                            selectFolder = backupViewModel::setFolderSelected,
                            setWifiOnly = backupViewModel::setWifiOnly,
                            runNow = backupViewModel::runNow,
                        ),
                    timeline =
                        TimelineActions(
                            refresh = timelineViewModel::refresh,
                            loadNext = timelineViewModel::loadNext,
                            open = timelineViewModel::open,
                            close = timelineViewModel::close,
                            next = { timelineViewModel.select(1) },
                            previous = { timelineViewModel.select(-1) },
                            imageUrl = timelineViewModel::derivativeUrl,
                            authorizationHeader = timelineViewModel::authorizationHeader,
                        ),
                ),
        )
    }
}

@Composable
@Suppress("LongParameterList")
private fun MirrorSurface(
    state: LoginUiState,
    backupState: BackupUiState,
    timelineState: TimelineUiState,
    defaultDeviceName: String,
    onLogin: (String, Boolean, String, String) -> Unit,
    connectedActions: ConnectedActions,
) {
    Box(
        modifier =
            Modifier
                .fillMaxSize()
                .background(VaultBackground)
                .statusBarsPadding(),
    ) {
        when {
            state.loading && state.credential == null -> {
                CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
            }
            state.credential != null -> {
                ConnectedScreen(
                    state =
                        ConnectedUiState(
                            serverUrl = state.credential.serverUrl,
                            loading = state.loading,
                            error = state.error,
                            backup = backupState,
                            timeline = timelineState,
                        ),
                    actions = connectedActions,
                )
            }
            else -> {
                LoginScreen(
                    defaultDeviceName = defaultDeviceName,
                    loading = state.loading,
                    error = state.error,
                    onLogin = onLogin,
                )
            }
        }
    }
}

@Composable
private fun LoginScreen(
    defaultDeviceName: String,
    loading: Boolean,
    error: String?,
    onLogin: (String, Boolean, String, String) -> Unit,
) {
    var values by remember {
        mutableStateOf(LoginFormValues(deviceName = defaultDeviceName))
    }
    var allowInsecureLan by remember { mutableStateOf(false) }

    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp, vertical = 32.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Text("Mirror", style = MaterialTheme.typography.headlineLarge)
        Text("Private photo vault", color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(32.dp))
        LoginFields(
            values = values,
            onValuesChange = { values = it },
            enabled = !loading,
        )
        InsecureLanToggle(
            checked = allowInsecureLan,
            onCheckedChange = { allowInsecureLan = it },
            enabled = !loading,
        )
        LoginAction(
            loading = loading,
            enabled = values.serverUrl.isNotBlank() && values.password.isNotEmpty(),
            error = error,
            onClick = {
                onLogin(
                    values.serverUrl,
                    allowInsecureLan,
                    values.password,
                    values.deviceName,
                )
            },
        )
    }
}

@Composable
private fun LoginFields(
    values: LoginFormValues,
    onValuesChange: (LoginFormValues) -> Unit,
    enabled: Boolean,
) {
    Column {
        OutlinedTextField(
            value = values.serverUrl,
            onValueChange = { onValuesChange(values.copy(serverUrl = it)) },
            modifier = Modifier.fillMaxWidth(),
            enabled = enabled,
            label = { Text("Server URL") },
            singleLine = true,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = values.deviceName,
            onValueChange = { onValuesChange(values.copy(deviceName = it)) },
            modifier = Modifier.fillMaxWidth(),
            enabled = enabled,
            label = { Text("Device name") },
            singleLine = true,
        )
        Spacer(Modifier.height(12.dp))
        OutlinedTextField(
            value = values.password,
            onValueChange = { onValuesChange(values.copy(password = it)) },
            modifier = Modifier.fillMaxWidth(),
            enabled = enabled,
            label = { Text("Password") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
        )
    }
}

@Composable
private fun InsecureLanToggle(
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    enabled: Boolean,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(
            checked = checked,
            onCheckedChange = onCheckedChange,
            enabled = enabled,
        )
        Text("Allow private-LAN HTTP")
    }
}

@Composable
private fun LoginAction(
    loading: Boolean,
    enabled: Boolean,
    error: String?,
    onClick: () -> Unit,
) {
    error?.let {
        Text(
            text = it,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.padding(vertical = 8.dp),
        )
    }
    Button(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth(),
        enabled = !loading && enabled,
    ) {
        if (loading) {
            CircularProgressIndicator(
                modifier = Modifier.height(20.dp),
                strokeWidth = 2.dp,
            )
        } else {
            Text("Connect")
        }
    }
}

@Composable
private fun ConnectedScreen(
    state: ConnectedUiState,
    actions: ConnectedActions,
) {
    var tab by remember { mutableStateOf(ConnectedTab.PHOTOS) }
    Column(modifier = Modifier.fillMaxSize()) {
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 20.dp, vertical = 16.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column {
                Text("Mirror", style = MaterialTheme.typography.titleLarge)
                Text(
                    state.serverUrl,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            TextButton(onClick = actions.logout, enabled = !state.loading) {
                Text("Disconnect")
            }
        }
        HorizontalDivider()
        ConnectedTabs(
            selected = tab,
            onSelected = { tab = it },
        )
        when (tab) {
            ConnectedTab.PHOTOS ->
                TimelineContent(
                    state = state.timeline,
                    actions = actions.timeline,
                    modifier = Modifier.weight(1f),
                )
            ConnectedTab.BACKUP ->
                BackupContent(
                    state = state.backup,
                    actions = actions.backup,
                    modifier = Modifier.weight(1f),
                )
        }
        state.error?.let {
            Text(
                text = it,
                color = MaterialTheme.colorScheme.error,
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 20.dp, vertical = 12.dp),
            )
        }
    }
}

@Composable
private fun ConnectedTabs(
    selected: ConnectedTab,
    onSelected: (ConnectedTab) -> Unit,
) {
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        TabButton(
            label = "Photos",
            selected = selected == ConnectedTab.PHOTOS,
            onClick = { onSelected(ConnectedTab.PHOTOS) },
        )
        TabButton(
            label = "Backup",
            selected = selected == ConnectedTab.BACKUP,
            onClick = { onSelected(ConnectedTab.BACKUP) },
        )
    }
}

@Composable
private fun TabButton(
    label: String,
    selected: Boolean,
    onClick: () -> Unit,
) {
    TextButton(onClick = onClick) {
        Text(
            text = label,
            color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
        )
    }
}
