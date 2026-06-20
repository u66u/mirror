use std::{net::SocketAddr, path::PathBuf};

use mirror_backend::config::{
    AuthSecret, Config, ConfigError, MlDevicePreference, RateLimitConfig, RateLimitSecret,
    SemanticSearchConfig,
    test_support::{
        default_worker_concurrency_for_test, parse_optional_bool_value_for_test,
        parse_positive_usize_for_test, parse_trusted_proxy_count_for_test,
        validate_operator_secret_for_test,
    },
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

#[test]
fn operator_secrets_require_non_whitespace_minimum_length() {
    assert_eq!(
        validate_operator_secret_for_test("", ConfigError::WeakAuthSecret),
        Err(ConfigError::WeakAuthSecret)
    );
    assert_eq!(
        validate_operator_secret_for_test(
            "                                ",
            ConfigError::WeakAuthSecret
        ),
        Err(ConfigError::WeakAuthSecret)
    );
    assert_eq!(
        validate_operator_secret_for_test(
            "12345678901234567890123456789012",
            ConfigError::WeakAuthSecret,
        ),
        Ok(())
    );
}

#[test]
fn trusted_proxy_parser_rejects_invalid_non_empty_entries() {
    assert_eq!(
        parse_trusted_proxy_count_for_test("127.0.0.1/32, ::1/128"),
        Ok(2)
    );

    assert_eq!(
        parse_trusted_proxy_count_for_test("127.0.0.1/32, not-a-cidr"),
        Err(ConfigError::InvalidTrustedProxy("not-a-cidr".to_owned()))
    );
}

#[test]
fn ml_positive_thread_settings_reject_zero_and_invalid_values() {
    assert_eq!(parse_positive_usize_for_test("8"), Some(8));
    assert_eq!(parse_positive_usize_for_test("0"), None);
    assert_eq!(parse_positive_usize_for_test("many"), None);
}

#[test]
fn default_worker_concurrency_is_positive() {
    assert!(default_worker_concurrency_for_test() > 0);
}

#[test]
fn strict_optional_bool_parser_accepts_operator_spellings() {
    assert_eq!(parse_optional_bool_value_for_test("true"), Some(true));
    assert_eq!(parse_optional_bool_value_for_test("off"), Some(false));
    assert_eq!(parse_optional_bool_value_for_test("maybe"), None);
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
