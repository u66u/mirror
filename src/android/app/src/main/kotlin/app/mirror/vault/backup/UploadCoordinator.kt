package app.mirror.vault.backup

import app.mirror.vault.auth.DeviceCredential
import app.mirror.vault.network.CreateUpload
import app.mirror.vault.network.MirrorApiException
import app.mirror.vault.network.MirrorTransportException
import app.mirror.vault.network.UploadApi
import app.mirror.vault.network.UploadSession
import org.bouncycastle.crypto.digests.Blake3Digest
import java.io.InputStream
import kotlin.math.min

sealed class BackupFailure(
    message: String,
    cause: Throwable? = null,
) : Exception(message, cause) {
    class Retryable(
        message: String,
        cause: Throwable,
    ) : BackupFailure(message, cause)

    class AuthenticationRequired(
        cause: Throwable,
    ) : BackupFailure("login expired", cause)

    class Permanent(
        message: String,
        cause: Throwable? = null,
    ) : BackupFailure(message, cause)

    class Ineligible : BackupFailure("backup item is no longer selected")
}

/**
 * Uploads one immutable MediaStore snapshot using persisted server progress.
 *
 * Fingerprint changes invalidate hash and upload session in Room before this
 * class runs. A second metadata check prevents marking bytes verified when the
 * local item changed during hashing or transfer.
 */
class UploadCoordinator(
    private val api: UploadApi,
    private val source: LocalMediaSource,
    private val queue: UploadQueue,
) : MediaUploader {
    override suspend fun upload(
        media: BackupMedia,
        credential: DeviceCredential,
    ) {
        requireEligible(media)
        requireCurrentFingerprint(media, credential)
        val hash = media.blake3 ?: hash(media)
        requireEligible(media)
        val session = resolveSession(media, credential, hash)

        when (session.status) {
            STATUS_VERIFIED -> complete(media, credential, session.uploadId)
            STATUS_OPEN -> {
                uploadParts(media, credential, session)
                requireEligible(media)
                requireCurrentFingerprint(media, credential)
                complete(media, credential, session.uploadId)
            }
            else -> throw BackupFailure.Permanent("invalid upload state")
        }
    }

    private suspend fun hash(media: BackupMedia): String {
        val digest = Blake3Digest()
        var total = 0L
        source.open(media.uri).use { input ->
            val buffer = ByteArray(HASH_BUFFER_SIZE)
            var read = input.read(buffer)
            while (read >= 0) {
                if (read > 0) {
                    digest.update(buffer, 0, read)
                    total += read
                }
                read = input.read(buffer)
            }
        }
        if (total != media.sizeBytes) {
            throw BackupFailure.Permanent("local media changed during hashing")
        }
        val output = ByteArray(digest.digestSize)
        digest.doFinal(output, 0)
        val hash = output.toHex()
        queue.saveHash(media, hash)
        return hash
    }

    private suspend fun resolveSession(
        media: BackupMedia,
        credential: DeviceCredential,
        hash: String,
    ): UploadSession {
        val existing =
            media.uploadId?.let { uploadId ->
                try {
                    api.getUpload(credential, uploadId)
                } catch (error: MirrorApiException) {
                    if (error.code == "upload_not_found") {
                        queue.clearUploadSession(media)
                        null
                    } else {
                        throw classify(error)
                    }
                } catch (error: MirrorTransportException) {
                    throw BackupFailure.Retryable("server unavailable", error)
                }
            }
        if (existing != null && existing.status != STATUS_CANCELLED) {
            return existing
        }

        return apiCall {
            api.createUpload(
                credential = credential,
                upload =
                    CreateUpload(
                        filename = media.displayName.takeCodePoints(MAX_FILENAME_CODE_POINTS),
                        sizeBytes = media.sizeBytes,
                        blake3 = hash,
                        mimeType = media.mimeType,
                        clientUploadKey = media.clientUploadKey,
                    ),
            )
        }.also { queue.saveUploadSession(media, it.uploadId) }
    }

    private suspend fun uploadParts(
        media: BackupMedia,
        credential: DeviceCredential,
        session: UploadSession,
    ) {
        val partCount = ((media.sizeBytes + PART_SIZE_BYTES - 1) / PART_SIZE_BYTES).toInt()
        validateCommittedParts(session, partCount)

        source.open(media.uri).use { input ->
            var remaining = media.sizeBytes
            var partIndex = 0
            while (remaining > 0) {
                requireEligible(media)
                val partSize = min(PART_SIZE_BYTES, remaining).toInt()
                val bytes = input.readExactly(partSize)
                requireFullPart(bytes, partSize)
                if (partIndex !in session.committedParts) {
                    apiCall {
                        api.putUploadPart(
                            credential = credential,
                            uploadId = session.uploadId,
                            partIndex = partIndex,
                            bytes = bytes,
                        )
                    }
                }
                remaining -= partSize
                partIndex += 1
            }
            requireEndOfInput(input)
        }
    }

    private suspend fun complete(
        media: BackupMedia,
        credential: DeviceCredential,
        uploadId: String,
    ) {
        requireEligible(media)
        val completed =
            apiCall {
                api.completeUpload(credential, uploadId)
            }
        queue.markVerified(media, completed.assetId)
    }

    private suspend fun requireEligible(media: BackupMedia) {
        if (!queue.isEligible(media)) {
            throw BackupFailure.Ineligible()
        }
    }

    private suspend fun requireCurrentFingerprint(
        media: BackupMedia,
        credential: DeviceCredential,
    ) {
        val current =
            source.metadata(media.uri)
                ?: throw BackupFailure.Permanent("local media is unavailable")
        if (!current.matches(media)) {
            media.uploadId?.let { uploadId ->
                cancelStaleSession(credential, uploadId)
            }
            throw BackupFailure.Permanent("local media changed; rescan required")
        }
    }

    private suspend fun cancelStaleSession(
        credential: DeviceCredential,
        uploadId: String,
    ) {
        try {
            api.cancelUpload(
                credential = credential,
                uploadId = uploadId,
            )
        } catch (_: MirrorApiException) {
            // Session expiry is harmless; next scan clears local progress.
        } catch (_: MirrorTransportException) {
            // Best-effort cleanup must not verify changed local bytes.
        }
    }

    private suspend fun <T> apiCall(block: suspend () -> T): T =
        try {
            block()
        } catch (error: MirrorApiException) {
            throw classify(error)
        } catch (error: MirrorTransportException) {
            throw BackupFailure.Retryable("server unavailable", error)
        }

    private fun classify(error: MirrorApiException): BackupFailure =
        when {
            error.status == HTTP_UNAUTHORIZED ->
                BackupFailure.AuthenticationRequired(error)
            error.isRetryableHttp() ->
                BackupFailure.Retryable("server rejected upload temporarily", error)
            else -> BackupFailure.Permanent(error.message, error)
        }

    companion object {
        const val PART_SIZE_BYTES = 4L * 1024L * 1024L
        private const val HASH_BUFFER_SIZE = 128 * 1024
        private const val MAX_FILENAME_CODE_POINTS = 255
        private const val STATUS_OPEN = "open"
        private const val STATUS_VERIFIED = "verified"
        private const val STATUS_CANCELLED = "cancelled"
        private const val HTTP_UNAUTHORIZED = 401
    }
}

private fun InputStream.readExactly(size: Int): ByteArray {
    val bytes = ByteArray(size)
    var offset = 0
    var exhausted = false
    while (offset < size && !exhausted) {
        val read = read(bytes, offset, size - offset)
        if (read < 0) {
            exhausted = true
        } else if (read == 0) {
            val single = read()
            if (single < 0) {
                exhausted = true
            } else {
                bytes[offset] = single.toByte()
                offset += 1
            }
        } else {
            offset += read
        }
    }
    return if (offset == size) bytes else bytes.copyOf(offset)
}

private fun ByteArray.toHex(): String = joinToString(separator = "") { byte -> "%02x".format(byte.toInt() and 0xff) }

private fun String.takeCodePoints(limit: Int): String {
    if (codePointCount(0, length) <= limit) {
        return this
    }
    return substring(0, offsetByCodePoints(0, limit))
}

private fun LocalMedia.matches(media: BackupMedia): Boolean =
    sizeBytes == media.sizeBytes &&
        mimeType == media.mimeType &&
        modifiedAtSeconds == media.modifiedAtSeconds &&
        generationModified == media.generationModified

private fun validateCommittedParts(
    session: UploadSession,
    partCount: Int,
) {
    if (session.committedParts.any { it < 0 || it >= partCount }) {
        throw BackupFailure.Permanent("server upload progress is invalid")
    }
}

private fun requireFullPart(
    bytes: ByteArray,
    expectedSize: Int,
) {
    if (bytes.size != expectedSize) {
        throw BackupFailure.Permanent("local media changed during upload")
    }
}

private fun requireEndOfInput(input: InputStream) {
    if (input.read() >= 0) {
        throw BackupFailure.Permanent("local media changed during upload")
    }
}

private fun MirrorApiException.isRetryableHttp(): Boolean = status == 408 || status == 429 || status >= 500
