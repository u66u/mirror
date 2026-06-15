package app.mirror.vault

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import app.mirror.vault.ui.BackupViewModel
import app.mirror.vault.ui.BackupViewModelFactory
import app.mirror.vault.ui.LoginViewModel
import app.mirror.vault.ui.LoginViewModelFactory
import app.mirror.vault.ui.MirrorApp

class MainActivity : ComponentActivity() {
    private val viewModel: LoginViewModel by viewModels {
        LoginViewModelFactory(
            repository = (application as MirrorApplication).authRepository,
            backupRepository = (application as MirrorApplication).backupRepository,
            defaultDeviceName = Build.MODEL.ifBlank { "Android device" },
        )
    }
    private val backupViewModel: BackupViewModel by viewModels {
        BackupViewModelFactory(
            repository = (application as MirrorApplication).backupRepository,
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        setContent {
            MirrorApp(viewModel, backupViewModel)
        }
    }

    override fun onResume() {
        super.onResume()
        backupViewModel.refresh()
    }
}
