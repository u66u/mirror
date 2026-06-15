package app.mirror.vault.network

import java.net.URI

/**
 * Validated Mirror server root.
 *
 * HTTPS accepts normal hosts. HTTP requires explicit user consent and a
 * private, loopback, or link-local IP literal. Hostnames are rejected for HTTP
 * to avoid DNS rebinding bypassing private-address policy.
 */
class ServerEndpoint private constructor(
    val baseUrl: String,
) {
    companion object {
        fun parse(
            rawValue: String,
            allowInsecurePrivateLan: Boolean,
        ): ServerEndpoint {
            val uri = parseUri(rawValue)
            val scheme = uri.scheme?.lowercase()
            val host = requireNotNull(uri.host) { "server host is required" }

            require(isOrigin(uri)) { "server URL must be an origin" }
            require(scheme == HTTPS || scheme == HTTP) {
                "server URL must use HTTPS or HTTP"
            }
            require(
                scheme != HTTP ||
                    (allowInsecurePrivateLan && isPrivateAddressLiteral(host)),
            ) { "HTTP requires explicit private-LAN access" }

            val normalized =
                URI(
                    scheme,
                    null,
                    host,
                    uri.port,
                    null,
                    null,
                    null,
                ).toString()
            return ServerEndpoint(normalized)
        }

        private fun parseUri(rawValue: String): URI =
            runCatching { URI(rawValue.trim()) }
                .getOrElse { throw IllegalArgumentException("invalid server URL", it) }

        private fun isOrigin(uri: URI): Boolean =
            uri.userInfo == null &&
                uri.query == null &&
                uri.fragment == null &&
                (uri.path.isEmpty() || uri.path == "/")

        private fun isPrivateAddressLiteral(host: String): Boolean {
            val value = host.lowercase().removePrefix("[").removeSuffix("]")
            return value == LOCALHOST ||
                value == IPV6_LOOPBACK ||
                isPrivateIpv6(value) ||
                isPrivateIpv4(value)
        }

        private fun isPrivateIpv6(value: String): Boolean = value.contains(':') && hasPrivateIpv6Prefix(value)

        private fun hasPrivateIpv6Prefix(value: String): Boolean = PRIVATE_IPV6_PREFIXES.any(value::startsWith)

        private fun isPrivateIpv4(value: String): Boolean {
            val octets =
                value
                    .split('.')
                    .mapNotNull(String::toIntOrNull)
                    .takeIf { it.size == IPV4_OCTET_COUNT && it.all(::isIpv4Octet) }
                    ?: return false

            return when (octets[0]) {
                10, 127 -> true
                169 -> octets[1] == 254
                172 -> octets[1] in 16..31
                192 -> octets[1] == 168
                else -> false
            }
        }

        private fun isIpv4Octet(value: Int): Boolean = value in 0..255

        private const val HTTPS = "https"
        private const val HTTP = "http"
        private const val LOCALHOST = "localhost"
        private const val IPV6_LOOPBACK = "::1"
        private const val IPV4_OCTET_COUNT = 4
        private val PRIVATE_IPV6_PREFIXES = listOf("fc", "fd", "fe8", "fe9", "fea", "feb")
    }
}
