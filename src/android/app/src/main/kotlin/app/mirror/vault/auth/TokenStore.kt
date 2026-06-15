package app.mirror.vault.auth

interface TokenStore {
    fun read(): DeviceCredential?

    fun write(credential: DeviceCredential)

    fun clear()
}
