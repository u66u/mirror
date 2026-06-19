use std::{net::SocketAddr, path::PathBuf};

use mirror_backend::config::{
    AuthSecret, Config, MlDevicePreference, RateLimitConfig, RateLimitSecret, SemanticSearchConfig,
};

#[test]
fn config_debug_redacts_secret_values() {
    let config = Config {
        bind_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
        log_level: "debug".to_owned(),
        database_url: Some("postgres://mirror:secret@db/mirror".to_owned()),
        storage_root: PathBuf::from("/vault"),
        rate_limit_secret: RateLimitSecret::from_secret("rate-secret"),
        auth_secret: AuthSecret::from_secret("auth-secret"),
        rate_limits: RateLimitConfig::default(),
        trusted_proxies: Vec::new(),
        ml_device: MlDevicePreference::GpuWithCpuFallback,
        ml_max_image_bytes: 25 * 1024 * 1024,
        semantic_search: SemanticSearchConfig::default(),
        face_recognition_enabled: false,
    };

    let debug = format!("{config:?}");

    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("postgres://"));
    assert!(!debug.contains("secret@"));
    assert!(!debug.contains("rate-secret"));
    assert!(!debug.contains("auth-secret"));
}
