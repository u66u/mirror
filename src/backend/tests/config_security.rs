use std::{net::SocketAddr, path::PathBuf};

use mirror_backend::config::{Config, RateLimitSecret};

#[test]
fn config_debug_redacts_secret_values() {
    let config = Config {
        bind_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
        log_level: "debug".to_owned(),
        database_url: Some("postgres://mirror:secret@db/mirror".to_owned()),
        storage_root: PathBuf::from("/vault"),
        rate_limit_secret: RateLimitSecret::from_secret("rate-secret"),
        trusted_proxies: Vec::new(),
    };

    let debug = format!("{config:?}");

    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("postgres://"));
    assert!(!debug.contains("secret@"));
    assert!(!debug.contains("rate-secret"));
}
