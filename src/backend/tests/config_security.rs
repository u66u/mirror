use std::{net::SocketAddr, path::PathBuf};

use mirror_backend::config::{
    AuthSecret, Config, ConfigError, MlDevicePreference, RateLimitConfig, RateLimitSecret,
    SemanticSearchConfig,
};

#[test]
fn config_debug_redacts_secret_values() {
    let config = test_config();

    let debug = format!("{config:?}");

    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("postgres://"));
    assert!(!debug.contains("secret@"));
    assert!(!debug.contains("rate-secret"));
    assert!(!debug.contains("auth-secret"));
}

#[test]
fn database_config_requires_explicit_auth_secret() {
    let mut config = test_config();
    config.auth_secret_configured = false;

    assert_eq!(
        config.validate_auth_secret_for_database(),
        Err(ConfigError::MissingAuthSecret)
    );

    config.database_url = None;
    assert_eq!(config.validate_auth_secret_for_database(), Ok(()));
}

fn test_config() -> Config {
    Config {
        bind_addr: SocketAddr::from(([127, 0, 0, 1], 8080)),
        log_level: "debug".to_owned(),
        database_url: Some("postgres://mirror:secret@db/mirror".to_owned()),
        storage_root: PathBuf::from("/vault"),
        rate_limit_secret: RateLimitSecret::from_secret("rate-secret"),
        auth_secret: AuthSecret::from_secret("auth-secret"),
        auth_secret_configured: true,
        cookie_secure: true,
        rate_limits: RateLimitConfig::default(),
        worker_concurrency: 1,
        trusted_proxies: Vec::new(),
        ml_device: MlDevicePreference::GpuWithCpuFallback,
        ml_intra_threads: None,
        ml_inter_threads: None,
        ml_parallel_execution: None,
        ml_max_concurrent_inferences: None,
        ml_max_image_bytes: 25 * 1024 * 1024,
        semantic_search: SemanticSearchConfig::default(),
        face_recognition_enabled: false,
    }
}
