package app.mirror.vault.settings

import android.content.Context
import android.content.SharedPreferences
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

enum class ThemeMode(
    val label: String,
) {
    SYSTEM("System"),
    LIGHT("Light"),
    DARK("Dark"),
}

/** How long a private share link stays valid. */
enum class ShareExpiry(
    val label: String,
    val seconds: Long,
) {
    DAY("1 day", SECONDS_PER_DAY),
    WEEK("7 days", SECONDS_PER_DAY * 7),
    MONTH("30 days", SECONDS_PER_DAY * 30),
}

private const val SECONDS_PER_DAY = 24L * 60 * 60

const val DEFAULT_GRID_COLUMNS = 3

/**
 * Per-device preferences. Server-wide policy (accounts, models, retention)
 * deliberately lives in the admin web app, not here.
 */
data class AppSettings(
    val theme: ThemeMode = ThemeMode.SYSTEM,
    val shareExpiry: ShareExpiry = ShareExpiry.WEEK,
    val hidePreviews: Boolean = false,
    val backupOnlyWhileCharging: Boolean = false,
    val backupVideos: Boolean = true,
    val gridColumns: Int = DEFAULT_GRID_COLUMNS,
)

/**
 * SharedPreferences-backed settings exposed as a [StateFlow].
 *
 * Reads are synchronous so background workers and MediaStore scans can consult
 * the current value without suspending.
 */
class AppPreferences(
    context: Context,
) {
    private val store: SharedPreferences =
        context.applicationContext.getSharedPreferences(FILE_NAME, Context.MODE_PRIVATE)
    private val mutable = MutableStateFlow(read())
    val settings: StateFlow<AppSettings> = mutable.asStateFlow()

    val current: AppSettings get() = mutable.value

    fun setTheme(theme: ThemeMode) = write(KEY_THEME, theme.name) { copy(theme = theme) }

    fun setShareExpiry(expiry: ShareExpiry) = write(KEY_SHARE_EXPIRY, expiry.name) { copy(shareExpiry = expiry) }

    fun setHidePreviews(value: Boolean) = write(KEY_HIDE_PREVIEWS, value) { copy(hidePreviews = value) }

    fun setChargingOnly(value: Boolean) = write(KEY_CHARGING_ONLY, value) { copy(backupOnlyWhileCharging = value) }

    fun setBackupVideos(value: Boolean) = write(KEY_BACKUP_VIDEOS, value) { copy(backupVideos = value) }

    fun setGridColumns(value: Int) = write(KEY_GRID_COLUMNS, value) { copy(gridColumns = value) }

    private fun write(
        key: String,
        value: Any,
        update: AppSettings.() -> AppSettings,
    ) {
        store.edit {
            when (value) {
                is Boolean -> putBoolean(key, value)
                is Int -> putInt(key, value)
                else -> putString(key, value.toString())
            }
        }
        mutable.value = mutable.value.update()
    }

    private fun read(): AppSettings {
        val defaults = AppSettings()
        return AppSettings(
            theme = enumValue(KEY_THEME, defaults.theme),
            shareExpiry = enumValue(KEY_SHARE_EXPIRY, defaults.shareExpiry),
            hidePreviews = store.getBoolean(KEY_HIDE_PREVIEWS, defaults.hidePreviews),
            backupOnlyWhileCharging = store.getBoolean(KEY_CHARGING_ONLY, defaults.backupOnlyWhileCharging),
            backupVideos = store.getBoolean(KEY_BACKUP_VIDEOS, defaults.backupVideos),
            gridColumns = store.getInt(KEY_GRID_COLUMNS, defaults.gridColumns),
        )
    }

    private inline fun <reified T : Enum<T>> enumValue(
        key: String,
        default: T,
    ): T =
        store
            .getString(key, null)
            ?.let { name -> enumValues<T>().firstOrNull { it.name == name } }
            ?: default

    private companion object {
        const val FILE_NAME = "mirror_preferences"
        const val KEY_THEME = "theme"
        const val KEY_SHARE_EXPIRY = "share_expiry"
        const val KEY_HIDE_PREVIEWS = "hide_previews"
        const val KEY_CHARGING_ONLY = "backup_only_charging"
        const val KEY_BACKUP_VIDEOS = "backup_videos"
        const val KEY_GRID_COLUMNS = "grid_columns"
    }
}
