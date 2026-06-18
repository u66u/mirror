//! Trusted-proxy client IP extraction.
//!
//! C003: forwarded headers are ignored unless the immediate peer is in a
//! configured trusted proxy range.

use std::net::IpAddr;

use actix_web::HttpRequest;
use ipnet::IpNet;

const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Returns the best client IP for rate limits and audit metadata.
#[must_use]
pub fn client_ip(req: &HttpRequest, trusted_proxies: &[IpNet]) -> Option<IpAddr> {
    let peer_ip = req.peer_addr().map(|addr| addr.ip())?;
    if !is_trusted_proxy(peer_ip, trusted_proxies) {
        return Some(peer_ip);
    }

    req.headers()
        .get(X_FORWARDED_FOR)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| forwarded_client_ip(value, trusted_proxies))
        .or(Some(peer_ip))
}

fn forwarded_client_ip(value: &str, trusted_proxies: &[IpNet]) -> Option<IpAddr> {
    let ips = value
        .split(',')
        .filter_map(|part| part.trim().parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    if ips.is_empty() {
        return None;
    }

    ips.iter()
        .rev()
        .copied()
        .find(|ip| !is_trusted_proxy(*ip, trusted_proxies))
        .or_else(|| ips.first().copied())
}

fn is_trusted_proxy(ip: IpAddr, trusted_proxies: &[IpNet]) -> bool {
    trusted_proxies.iter().any(|network| network.contains(&ip))
}
