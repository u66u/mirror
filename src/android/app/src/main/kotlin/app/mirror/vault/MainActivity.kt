package app.mirror.vault

import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.mirror.vault.library.PeopleViewModel
import app.mirror.vault.library.SearchViewModel
import app.mirror.vault.library.TrashViewModel
import app.mirror.vault.timeline.TimelineViewModel
import app.mirror.vault.timeline.TimelineViewModelFactory
import app.mirror.vault.ui.BackupViewModel
import app.mirror.vault.ui.BackupViewModelFactory
import app.mirror.vault.ui.LoginViewModel
import app.mirror.vault.ui.LoginViewModelFactory
import app.mirror.vault.ui.MirrorApp
import app.mirror.vault.ui.MirrorViewModels
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val app get() = application as MirrorApplication

    private val viewModel: LoginViewModel by viewModels {
        LoginViewModelFactory(
            repository = app.authRepository,
            backupRepository = app.backupRepository,
            defaultDeviceName = friendlyDeviceName(),
            onSignedOut = { app.timelineCache.clear() },
        )
    }
    private val backupViewModel: BackupViewModel by viewModels {
        BackupViewModelFactory(repository = app.backupRepository)
    }
    private val timelineViewModel: TimelineViewModel by viewModels {
        TimelineViewModelFactory(
            repository = app.timelineRepository,
            library = app.libraryRepository,
            store = app.timelineCache,
        )
    }
    private val searchViewModel: SearchViewModel by viewModels {
        viewModelFactory { initializer { SearchViewModel(app.libraryRepository) } }
    }
    private val peopleViewModel: PeopleViewModel by viewModels {
        viewModelFactory { initializer { PeopleViewModel(app.libraryRepository) } }
    }
    private val trashViewModel: TrashViewModel by viewModels {
        viewModelFactory { initializer { TrashViewModel(app.libraryRepository) } }
    }

    /** "Pixel 8" rather than "google Pixel 8", and not the emulator's raw build string. */
    private fun friendlyDeviceName(): String {
        val model = Build.MODEL.orEmpty().trim()
        return when {
            model.isEmpty() -> "Android device"
            model.startsWith("sdk_") || Build.FINGERPRINT.contains("generic") -> "Android emulator"
            else -> model
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                app.appPreferences.settings.collect { settings ->
                    if (settings.hidePreviews) {
                        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    } else {
                        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                    }
                }
            }
        }
        setContent {
            MirrorApp(
                MirrorViewModels(
                    login = viewModel,
                    backup = backupViewModel,
                    timeline = timelineViewModel,
                    search = searchViewModel,
                    people = peopleViewModel,
                    trash = trashViewModel,
                    preferences = app.appPreferences,
                ),
            )
        }
    }

    override fun onResume() {
        super.onResume()
        backupViewModel.refresh()
        timelineViewModel.refreshQuietly()
    }
}
