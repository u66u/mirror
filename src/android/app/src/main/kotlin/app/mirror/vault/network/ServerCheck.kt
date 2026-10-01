package app.mirror.vault.network

import java.net.URI

/** Result of checking what the user typed into the vault-address field. */
sealed interface ServerCheck {
    /** Nothing typed yet. */
    data object Empty : ServerCheck

    /** Can't be used; [message] says why in plain words and what to do. */
    data class Invalid(
        val message: String,
    ) : ServerCheck

    /** A private-network `http://` address that still needs the user's explicit consent. */
    data class NeedsLocalConsent(
        val url: String,
    ) : ServerCheck

    /** Ready to connect to [url]; [local] marks consented plain-HTTP private addresses. */
    data class Ok(
        val url: String,
        val local: Boolean,
    ) : ServerCheck
}

private val LOCAL_IPV4 =
    listOf(
        Regex("""^(10|127)\.\d+\.\d+\.\d+$"""),
        Regex("""^192\.168\.\d+\.\d+$"""),
        Regex("""^172\.(1[6-9]|2\d|3[01])\.\d+\.\d+$"""),
        Regex("""^169\.254\.\d+\.\d+$"""),
    )

/**
 * Accepts bare hosts: private IP literals default to `http://`, everything
 * else to `https://`. A typed scheme keeps its meaning but is case-folded, so
 * `HTTP://10.0.0.2` behaves like `http://10.0.0.2`.
 */
fun normalizeServer(raw: String): String {
    val typed = raw.trim()
    val schemeEnd = typed.indexOf("://")
    return when {
        typed.isEmpty() -> typed
        // Trim trailing slashes only after the "://", so a bare "https://" stays itself.
        schemeEnd >= 0 ->
            typed.substring(0, schemeEnd).lowercase() + "://" + typed.substring(schemeEnd + 3).trimEnd('/')
        else -> typed.trimEnd('/').let { (if (isLocalHost(it)) "http://" else "https://") + it }
    }
}

private fun isLocalHost(value: String): Boolean {
    val host = value.substringBefore(':').substringBefore('/')
    return host == "localhost" || LOCAL_IPV4.any { it.matches(host) }
}

/**
 * Judges the address using the same rules the connection will enforce
 * ([ServerEndpoint]), but phrased for people instead of developers.
 */
fun checkServer(
    raw: String,
    allowLocalHttp: Boolean,
): ServerCheck {
    if (raw.isBlank()) return ServerCheck.Empty
    val normalized = normalizeServer(raw)
    val uri = runCatching { URI(normalized) }.getOrNull()
    return uri?.let { problemWith(it) ?: acceptable(it, normalized, allowLocalHttp) }
        ?: ServerCheck.Invalid("That doesn't look like an address.")
}

/** The first thing wrong with the address's shape, or null when it is fine. */
private fun problemWith(uri: URI): ServerCheck.Invalid? {
    val scheme = uri.scheme?.lowercase()
    val message =
        when {
            scheme != "http" && scheme != "https" -> "Start with https:// (or http:// on your home network)."
            uri.host.isNullOrEmpty() -> "That doesn't look like an address."
            uri.userInfo != null || uri.query != null || uri.fragment != null || uri.hasPath() ->
                "Use just the address, with no path — like photos.example.com."
            scheme == "http" && !ServerEndpoint.isPrivateHost(uri.host) ->
                "Plain http:// only works on a private network. Use https:// for anything else."
            else -> null
        }
    return message?.let(ServerCheck::Invalid)
}

private fun acceptable(
    uri: URI,
    normalized: String,
    allowLocalHttp: Boolean,
): ServerCheck {
    val plainHttp = uri.scheme.equals("http", ignoreCase = true)
    return when {
        plainHttp && !allowLocalHttp -> ServerCheck.NeedsLocalConsent(normalized)
        else ->
            runCatching { ServerEndpoint.parse(normalized, allowInsecurePrivateLan = true).baseUrl }
                .fold(
                    onSuccess = { ServerCheck.Ok(it, local = plainHttp) },
                    onFailure = { ServerCheck.Invalid("That address can't be used.") },
                )
    }
}

private fun URI.hasPath(): Boolean = !path.isNullOrEmpty() && path != "/"
