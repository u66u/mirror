package app.mirror.vault.auth

import android.annotation.SuppressLint
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Stores Android bearer credential encrypted by a non-exportable Keystore key.
 *
 * Ciphertext lives in private preferences. App backup is disabled because
 * restoring ciphertext without its device-bound key makes the credential
 * unreadable.
 */
@SuppressLint("UseKtx") // T105: synchronous commit result is a credential durability invariant.
class AndroidKeystoreTokenStore(
    context: Context,
) : TokenStore {
    private val preferences =
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    override fun read(): DeviceCredential? {
        val encodedIv = preferences.getString(IV_KEY, null)
        val encodedCiphertext = preferences.getString(CIPHERTEXT_KEY, null)
        if (encodedIv == null && encodedCiphertext == null) {
            return null
        }
        if (encodedIv == null || encodedCiphertext == null) {
            throw CredentialStorageException("stored credential is incomplete")
        }

        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(
            Cipher.DECRYPT_MODE,
            encryptionKey(),
            GCMParameterSpec(GCM_TAG_BITS, Base64.decode(encodedIv, Base64.NO_WRAP)),
        )
        val plaintext =
            cipher
                .doFinal(Base64.decode(encodedCiphertext, Base64.NO_WRAP))
                .toString(Charsets.UTF_8)
        return decodeCredential(plaintext)
    }

    override fun write(credential: DeviceCredential) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, encryptionKey())
        val ciphertext = cipher.doFinal(encodeCredential(credential).toByteArray(Charsets.UTF_8))
        val committed =
            preferences
                .edit()
                .putString(IV_KEY, Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
                .putString(
                    CIPHERTEXT_KEY,
                    Base64.encodeToString(ciphertext, Base64.NO_WRAP),
                ).commit()
        if (!committed) {
            throw CredentialStorageException("credential storage commit failed")
        }
    }

    override fun clear() {
        if (!preferences.edit().clear().commit()) {
            throw CredentialStorageException("credential storage clear failed")
        }
    }

    private fun encryptionKey(): SecretKey {
        val keyStore =
            KeyStore.getInstance(KEYSTORE_PROVIDER).apply {
                load(null)
            }
        val existing = keyStore.getKey(KEY_ALIAS, null)
        if (existing is SecretKey) {
            return existing
        }

        val generator =
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE_PROVIDER)
        generator.init(
            KeyGenParameterSpec
                .Builder(
                    KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                ).setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(KEY_SIZE_BITS)
                .build(),
        )
        return generator.generateKey()
    }

    private fun encodeCredential(credential: DeviceCredential): String =
        buildJsonObject {
            put("server_url", credential.serverUrl)
            put("device_token_id", credential.deviceTokenId)
            put("token", credential.token)
        }.toString()

    private fun decodeCredential(value: String): DeviceCredential {
        val objectValue = Json.parseToJsonElement(value).jsonObject
        return DeviceCredential(
            serverUrl = objectValue.requiredString("server_url"),
            deviceTokenId = objectValue.requiredString("device_token_id"),
            token = objectValue.requiredString("token"),
        )
    }

    private fun Map<String, kotlinx.serialization.json.JsonElement>.requiredString(key: String): String =
        get(key)?.jsonPrimitive?.content?.takeIf(String::isNotEmpty)
            ?: throw CredentialStorageException("stored credential field missing")

    private companion object {
        const val PREFERENCES_NAME = "mirror_device_credential"
        const val IV_KEY = "iv"
        const val CIPHERTEXT_KEY = "ciphertext"
        const val KEYSTORE_PROVIDER = "AndroidKeyStore"
        const val KEY_ALIAS = "mirror_device_token_key_v1"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val GCM_TAG_BITS = 128
        const val KEY_SIZE_BITS = 256
    }
}

class CredentialStorageException(
    message: String,
) : IllegalStateException(message)
