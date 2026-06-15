package app.mirror.vault.auth

data class DeviceCredential(
    val serverUrl: String,
    val deviceTokenId: String,
    val token: String,
)
