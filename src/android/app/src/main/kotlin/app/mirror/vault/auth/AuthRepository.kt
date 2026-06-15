package app.mirror.vault.auth

import app.mirror.vault.network.IssuedDeviceToken
import app.mirror.vault.network.MirrorApi
import app.mirror.vault.network.MirrorApiException
import app.mirror.vault.network.MirrorTransportException
import app.mirror.vault.network.ServerEndpoint

enum class LogoutResult {
    REMOTE_REVOKED,
    LOCAL_ONLY,
}

/**
 * Owns Android login persistence ordering.
 *
 * Remote login completes before local replacement, so failed authentication or
 * transport errors cannot destroy a previously valid credential.
 */
class AuthRepository(
    private val api: MirrorApi,
    private val tokenStore: TokenStore,
) {
    fun currentCredential(): DeviceCredential? = tokenStore.read()

    suspend fun login(
        rawServerUrl: String,
        allowInsecurePrivateLan: Boolean,
        password: String,
        deviceName: String,
    ): DeviceCredential {
        val endpoint = ServerEndpoint.parse(rawServerUrl, allowInsecurePrivateLan)
        val normalizedDeviceName =
            deviceName.trim().takeIf(String::isNotEmpty)
                ?: throw IllegalArgumentException("device name is required")
        val issued: IssuedDeviceToken =
            api.login(
                endpoint = endpoint,
                password = password,
                deviceName = normalizedDeviceName,
            )
        val credential =
            DeviceCredential(
                serverUrl = endpoint.baseUrl,
                deviceTokenId = issued.deviceTokenId,
                token = issued.token,
            )
        tokenStore.write(credential)
        return credential
    }

    suspend fun logout(credential: DeviceCredential): LogoutResult {
        tokenStore.clear()
        return try {
            api.revoke(credential)
            LogoutResult.REMOTE_REVOKED
        } catch (_: MirrorApiException) {
            LogoutResult.LOCAL_ONLY
        } catch (_: MirrorTransportException) {
            LogoutResult.LOCAL_ONLY
        }
    }
}
