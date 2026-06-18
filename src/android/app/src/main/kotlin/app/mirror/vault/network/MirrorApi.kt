package app.mirror.vault.network

import app.mirror.vault.auth.DeviceCredential
import io.ktor.client.HttpClient
import io.ktor.client.engine.okhttp.OkHttp
import io.ktor.client.plugins.HttpTimeout
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.request.bearerAuth
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.header
import io.ktor.client.request.post
import io.ktor.client.request.put
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.isSuccess
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put
import java.io.IOException

data class IssuedDeviceToken(
    val deviceTokenId: String,
    val token: String,
)

interface MirrorApi {
    suspend fun login(
        endpoint: ServerEndpoint,
        password: String,
        deviceName: String,
    ): IssuedDeviceToken

    suspend fun revoke(credential: DeviceCredential)
}

/**
 * First-party HTTP boundary.
 *
 * Raw passwords and bearer tokens are never logged. All request URLs originate
 * from [ServerEndpoint], which enforces C015 cleartext restrictions.
 */
@Suppress("TooManyFunctions") // All first-party HTTP details remain in the declared MirrorApi boundary.
class KtorMirrorApi(
    private val client: HttpClient = createHttpClient(),
) : MirrorApi,
    TimelineApi,
    UploadApi {
    override suspend fun login(
        endpoint: ServerEndpoint,
        password: String,
        deviceName: String,
    ): IssuedDeviceToken =
        transport {
            val response =
                client.post("${endpoint.baseUrl}/auth/device-login") {
                    header(HttpHeaders.ContentType, ContentType.Application.Json)
                    setBody(
                        buildJsonObject {
                            put("password", password)
                            put("name", deviceName)
                        },
                    )
                }
            requireSuccess(response)
            val body = parseObject(response.bodyAsText())
            IssuedDeviceToken(
                deviceTokenId = body.requiredString("device_token_id"),
                token = body.requiredString("token"),
            )
        }

    override suspend fun revoke(credential: DeviceCredential) =
        transport {
            val endpoint = credential.endpoint()
            val response =
                client.delete(
                    "${endpoint.baseUrl}/device-tokens/${credential.deviceTokenId}",
                ) {
                    bearerAuth(credential.token)
                }
            if (
                response.status != HttpStatusCode.Unauthorized &&
                response.status != HttpStatusCode.NotFound
            ) {
                requireSuccess(response)
            }
        }

    override suspend fun listAssets(
        credential: DeviceCredential,
        cursor: String?,
        limit: Int,
    ): AssetTimelinePage =
        transport {
            val response =
                client.get("${credential.endpoint().baseUrl}/assets") {
                    bearerAuth(credential.token)
                    url {
                        parameters.append("limit", limit.toString())
                        cursor?.let { parameters.append("cursor", it) }
                    }
                }
            requireSuccess(response)
            val body = parseObject(response.bodyAsText())
            AssetTimelinePage(
                items =
                    body["items"]
                        ?.jsonArray
                        ?.map { it.assetTimelineItem() }
                        ?: throw invalidResponse(),
                nextCursor = body.optionalString("next_cursor"),
            )
        }

    override suspend fun createUpload(
        credential: DeviceCredential,
        upload: CreateUpload,
    ): UploadSession =
        transport {
            val response =
                client.post("${credential.endpoint().baseUrl}/uploads") {
                    bearerAuth(credential.token)
                    header(HttpHeaders.ContentType, ContentType.Application.Json)
                    setBody(
                        buildJsonObject {
                            put("original_filename", upload.filename)
                            put("expected_size", upload.sizeBytes)
                            put("expected_blake3", upload.blake3)
                            put("media_type", upload.mimeType)
                            put("client_upload_key", upload.clientUploadKey)
                        },
                    )
                }
            response.uploadSession()
        }

    override suspend fun getUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): UploadSession =
        transport {
            val response =
                client.get("${credential.endpoint().baseUrl}/uploads/$uploadId") {
                    bearerAuth(credential.token)
                }
            response.uploadSession()
        }

    override suspend fun putUploadPart(
        credential: DeviceCredential,
        uploadId: String,
        partIndex: Int,
        bytes: ByteArray,
    ) {
        transport {
            val response =
                client.put(
                    "${credential.endpoint().baseUrl}/uploads/$uploadId/parts/$partIndex",
                ) {
                    bearerAuth(credential.token)
                    header(HttpHeaders.ContentType, ContentType.Application.OctetStream)
                    setBody(bytes)
                }
            requireSuccess(response)
        }
    }

    override suspend fun completeUpload(
        credential: DeviceCredential,
        uploadId: String,
    ): CompletedUpload =
        transport {
            val response =
                client.post("${credential.endpoint().baseUrl}/uploads/$uploadId/complete") {
                    bearerAuth(credential.token)
                }
            requireSuccess(response)
            val body = parseObject(response.bodyAsText())
            CompletedUpload(
                assetId =
                    body["promoted"]
                        ?.jsonObject
                        ?.requiredString("asset_id")
                        ?: throw invalidResponse(),
            )
        }

    override suspend fun cancelUpload(
        credential: DeviceCredential,
        uploadId: String,
    ) {
        transport {
            val response =
                client.delete("${credential.endpoint().baseUrl}/uploads/$uploadId") {
                    bearerAuth(credential.token)
                }
            try {
                requireSuccess(response)
            } catch (error: MirrorApiException) {
                if (error.code != "upload_not_found") {
                    throw error
                }
            }
        }
    }

    private suspend fun requireSuccess(response: HttpResponse) {
        if (response.status.isSuccess()) {
            return
        }
        val body =
            runCatching {
                Json
                    .parseToJsonElement(response.bodyAsText())
                    .jsonObject
            }.getOrNull()
        throw MirrorApiException(
            status = response.status.value,
            code = body?.get("error")?.jsonPrimitive?.content,
            message = body?.get("message")?.jsonPrimitive?.content ?: "request failed",
        )
    }

    private suspend fun HttpResponse.uploadSession(): UploadSession {
        requireSuccess(this)
        val body = parseObject(bodyAsText())
        return UploadSession(
            uploadId = body.requiredString("upload_id"),
            status = body.requiredString("status"),
            committedParts =
                body["committed_parts"]
                    ?.jsonArray
                    ?.mapTo(mutableSetOf()) { it.jsonPrimitive.int }
                    ?: throw invalidResponse(),
        )
    }

    private fun Map<String, kotlinx.serialization.json.JsonElement>.requiredString(key: String): String =
        get(key)?.jsonPrimitive?.content?.takeIf(String::isNotEmpty)
            ?: throw invalidResponse()

    private fun Map<String, JsonElement>.requiredLong(key: String): Long =
        get(key)?.jsonPrimitive?.long
            ?: throw invalidResponse()

    private fun Map<String, JsonElement>.requiredInt(key: String): Int =
        get(key)?.jsonPrimitive?.int
            ?: throw invalidResponse()

    private fun Map<String, JsonElement>.optionalString(key: String): String? =
        when (val value = get(key)) {
            null,
            JsonNull,
            -> null
            else -> value.jsonPrimitive.content
        }

    private fun JsonElement.assetTimelineItem(): AssetTimelineItem {
        val body = jsonObject
        return AssetTimelineItem(
            assetId = body.requiredString("asset_id"),
            createdAt = body.requiredString("created_at"),
            favoriteAt = body.optionalString("favorite_at"),
            originalBlake3 = body.requiredString("original_blake3"),
            mediaType = body.requiredString("media_type"),
            sizeBytes = body.requiredLong("size_bytes"),
            originalFilename = body.optionalString("original_filename"),
            thumbnail = body["thumbnail"].assetDerivativeOrNull(),
            preview = body["preview"].assetDerivativeOrNull(),
        )
    }

    private fun JsonElement?.assetDerivativeOrNull(): AssetDerivative? =
        when (this) {
            null,
            JsonNull,
            -> null
            else -> {
                val body = jsonObject
                AssetDerivative(
                    format = body.requiredString("format"),
                    width = body.requiredInt("width"),
                    height = body.requiredInt("height"),
                )
            }
        }

    private fun DeviceCredential.endpoint(): ServerEndpoint =
        ServerEndpoint.parse(
            rawValue = serverUrl,
            allowInsecurePrivateLan = true,
        )

    private suspend fun <T> transport(block: suspend () -> T): T =
        try {
            block()
        } catch (error: CancellationException) {
            throw error
        } catch (error: MirrorApiException) {
            throw error
        } catch (error: IOException) {
            throw MirrorTransportException(error)
        }

    private fun parseObject(body: String) =
        try {
            Json.parseToJsonElement(body).jsonObject
        } catch (_: SerializationException) {
            throw invalidResponse()
        } catch (_: IllegalArgumentException) {
            throw invalidResponse()
        }

    private fun invalidResponse(): MirrorApiException =
        MirrorApiException(
            status = INVALID_RESPONSE_STATUS,
            code = "invalid_response",
            message = "invalid server response",
        )

    private companion object {
        fun createHttpClient(): HttpClient =
            HttpClient(OkHttp) {
                expectSuccess = false
                install(ContentNegotiation) {
                    json(
                        Json {
                            ignoreUnknownKeys = true
                        },
                    )
                }
                install(HttpTimeout) {
                    connectTimeoutMillis = CONNECT_TIMEOUT_MILLIS
                    requestTimeoutMillis = REQUEST_TIMEOUT_MILLIS
                    socketTimeoutMillis = REQUEST_TIMEOUT_MILLIS
                }
            }

        const val INVALID_RESPONSE_STATUS = 500
        const val CONNECT_TIMEOUT_MILLIS = 10_000L
        const val REQUEST_TIMEOUT_MILLIS = 30_000L
    }
}

class MirrorApiException(
    val status: Int,
    val code: String?,
    override val message: String,
) : IllegalStateException(message)

class MirrorTransportException(
    cause: Throwable,
) : IOException("server request failed", cause)
