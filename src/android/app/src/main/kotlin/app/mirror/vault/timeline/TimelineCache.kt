package app.mirror.vault.timeline

import android.content.Context
import app.mirror.vault.network.AssetDerivative
import app.mirror.vault.network.AssetTimelineItem
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put
import java.io.File

/** Where the newest part of the vault timeline is kept so the library opens instantly and offline. */
interface TimelineStore {
    fun read(serverUrl: String): List<AssetTimelineItem>

    fun write(
        serverUrl: String,
        items: List<AssetTimelineItem>,
    )

    /** Forgets everything, e.g. when this device disconnects from the vault. */
    fun clear()
}

/**
 * File-backed [TimelineStore]. A single small JSON document is enough: it only
 * holds metadata for the newest items, tied to one server so switching vaults
 * can never show another vault's photos.
 */
class TimelineCache(
    context: Context,
) : TimelineStore {
    private val file = File(context.applicationContext.filesDir, FILE_NAME)

    override fun read(serverUrl: String): List<AssetTimelineItem> =
        runCatching {
            if (!file.exists()) return@runCatching emptyList()
            val root = Json.parseToJsonElement(file.readText()).jsonObject
            if (root["server"]?.jsonPrimitive?.contentOrNull != serverUrl) {
                emptyList()
            } else {
                root["items"]?.jsonArray?.map { it.toItem() }.orEmpty()
            }
        }.getOrDefault(emptyList())

    override fun write(
        serverUrl: String,
        items: List<AssetTimelineItem>,
    ) {
        runCatching {
            val document =
                buildJsonObject {
                    put("server", serverUrl)
                    put("items", JsonArray(items.take(MAX_ITEMS).map { it.toJson() }))
                }
            val temp = File(file.parentFile, "$FILE_NAME.tmp")
            temp.writeText(document.toString())
            temp.renameTo(file)
        }
    }

    override fun clear() {
        file.delete()
    }

    private fun AssetTimelineItem.toJson(): JsonObject =
        buildJsonObject {
            put("asset_id", assetId)
            put("created_at", createdAt)
            put("favorite_at", favoriteAt)
            put("original_blake3", originalBlake3)
            put("media_type", mediaType)
            put("size_bytes", sizeBytes)
            put("original_filename", originalFilename)
            put("thumbnail", thumbnail?.toJson() ?: JsonNull)
            put("preview", preview?.toJson() ?: JsonNull)
        }

    private fun AssetDerivative.toJson(): JsonObject =
        buildJsonObject {
            put("format", format)
            put("width", width)
            put("height", height)
        }

    private fun JsonElement.toItem(): AssetTimelineItem {
        val body = jsonObject
        return AssetTimelineItem(
            assetId = body.getValue("asset_id").jsonPrimitive.content,
            createdAt = body.getValue("created_at").jsonPrimitive.content,
            favoriteAt = body.optional("favorite_at"),
            originalBlake3 = body.getValue("original_blake3").jsonPrimitive.content,
            mediaType = body.getValue("media_type").jsonPrimitive.content,
            sizeBytes = body.getValue("size_bytes").jsonPrimitive.long,
            originalFilename = body.optional("original_filename"),
            thumbnail = body["thumbnail"].derivative(),
            preview = body["preview"].derivative(),
        )
    }

    private fun JsonObject.optional(key: String): String? = (this[key] as? JsonPrimitive)?.contentOrNull

    private fun JsonElement?.derivative(): AssetDerivative? =
        (this as? JsonObject)?.let {
            AssetDerivative(
                format = it.getValue("format").jsonPrimitive.content,
                width = it.getValue("width").jsonPrimitive.int,
                height = it.getValue("height").jsonPrimitive.int,
            )
        }

    private companion object {
        const val FILE_NAME = "timeline-cache.json"
        const val MAX_ITEMS = 300
    }
}
