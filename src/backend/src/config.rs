//! Process configuration.
//!
//! V1 keeps config explicit and environment-backed. Secret-bearing config must
//! never be logged without redaction; see `docs/style-guide.md`.

use std::{env, fmt, net::SocketAddr, num::NonZeroUsize, path::PathBuf, thread};

use ipnet::IpNet;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::Duration;

/// Runtime configuration shared by the API process.
#[derive(Clone)]
pub struct Config {
    /// Address Actix binds to.
    pub bind_addr: SocketAddr,
    /// Tracing filter, for example `info` or `mirror_backend=debug`.
    pub log_level: String,
    /// Postgres URL. Redact before logging.
    pub database_url: Option<String>,
    /// Local vault storage root. Must not be inside a public web root.
    pub storage_root: PathBuf,
    /// Stable keyed-hash secret for DB-backed rate-limit buckets.
    pub rate_limit_secret: RateLimitSecret,
    /// Stable encryption key material for owner authentication secrets.
    pub auth_secret: AuthSecret,
    /// Whether `MIRROR_AUTH_SECRET` was explicitly configured.
    pub auth_secret_configured: bool,
    /// Whether browser auth cookies should carry the Secure attribute.
    pub cookie_secure: bool,
    /// Operator-configurable DB-backed rate-limit quotas.
    pub rate_limits: RateLimitConfig,
    /// Number of concurrent durable worker jobs in the production worker.
    pub worker_concurrency: usize,
    /// Proxy CIDRs allowed to supply forwarded client IP headers.
    pub trusted_proxies: Vec<IpNet>,
    /// Preferred ML execution device for future runtime-backed workers.
    pub ml_device: MlDevicePreference,
    /// Optional ONNX intra-op thread count. Unset preserves the runtime default.
    pub ml_intra_threads: Option<usize>,
    /// Optional ONNX inter-op thread count. Unset preserves the runtime default.
    pub ml_inter_threads: Option<usize>,
    /// Optional ONNX graph-parallel execution override.
    pub ml_parallel_execution: Option<bool>,
    /// Optional application-level cap on concurrent ML inference calls.
    pub ml_max_concurrent_inferences: Option<usize>,
    /// Maximum encoded image bytes read into the embedding runtime.
    pub ml_max_image_bytes: usize,
    /// Semantic pgvector search execution knobs.
    pub semantic_search: SemanticSearchConfig,
    /// Enables face-index worker jobs. Disabled by default for privacy.
    pub face_recognition_enabled: bool,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("log_level", &self.log_level)
            .field(
                "database_url",
                &self.database_url.as_ref().map(|_| "[redacted]"),
            )
            .field("storage_root", &self.storage_root)
            .field("rate_limit_secret", &self.rate_limit_secret)
            .field("auth_secret", &self.auth_secret)
            .field("auth_secret_configured", &self.auth_secret_configured)
            .field("cookie_secure", &self.cookie_secure)
            .field("rate_limits", &self.rate_limits)
            .field("worker_concurrency", &self.worker_concurrency)
            .field("trusted_proxies", &self.trusted_proxies)
            .field("ml_device", &self.ml_device)
            .field("ml_intra_threads", &self.ml_intra_threads)
            .field("ml_inter_threads", &self.ml_inter_threads)
            .field("ml_parallel_execution", &self.ml_parallel_execution)
            .field(
                "ml_max_concurrent_inferences",
                &self.ml_max_concurrent_inferences,
            )
            .field("ml_max_image_bytes", &self.ml_max_image_bytes)
            .field("semantic_search", &self.semantic_search)
            .field("face_recognition_enabled", &self.face_recognition_enabled)
            .finish()
    }
}

const MIN_OPERATOR_SECRET_BYTES: usize = 32;

/// Configuration validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// DB-backed auth persists encrypted secrets and needs stable key material.
    #[error("MIRROR_AUTH_SECRET is required when MIRROR_DATABASE_URL is configured")]
    MissingAuthSecret,
    /// Auth secret was present but too weak for production secret encryption.
    #[error("MIRROR_AUTH_SECRET must be at least 32 non-whitespace bytes")]
    WeakAuthSecret,
    /// Rate-limit secret was present but too weak for stable keyed hashing.
    #[error("MIRROR_RATE_LIMIT_SECRET must be at least 32 non-whitespace bytes")]
    WeakRateLimitSecret,
    /// One configured trusted-proxy CIDR failed to parse.
    #[error("MIRROR_TRUSTED_PROXIES contains invalid CIDR: {0}")]
    InvalidTrustedProxy(String),
    /// A positive-integer ML setting was invalid.
    #[error("{0} must be a positive integer")]
    InvalidPositiveInteger(&'static str),
    /// An optional boolean ML setting was invalid.
    #[error("{0} must be a boolean")]
    InvalidBoolean(&'static str),
}

/// Operator-configurable semantic search behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticSearchConfig {
    /// Use ANN-compatible pgvector ordering. Exact search remains default.
    pub ann_enabled: bool,
    /// pgvector HNSW scan breadth when ANN is enabled.
    pub ann_ef_search: i32,
}

impl Default for SemanticSearchConfig {
    fn default() -> Self {
        Self {
            ann_enabled: false,
            ann_ef_search: 40,
        }
    }
}

impl SemanticSearchConfig {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            ann_enabled: env_bool("MIRROR_SEMANTIC_ANN_ENABLED", defaults.ann_enabled),
            ann_ef_search: env_i32("MIRROR_SEMANTIC_ANN_EF_SEARCH", defaults.ann_ef_search),
        }
    }
}

/// One DB-backed rate-limit quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitQuota {
    /// Maximum accepted attempts per window before quota routes return 429.
    pub max_per_window: i32,
    /// Counting window length.
    pub window: Duration,
    /// Block length after quota exhaustion.
    pub block_for: Duration,
}

/// Operator-configurable route rate limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Failed owner-password login attempts.
    pub owner_password_login: RateLimitQuota,
    /// Failed owner TOTP/recovery-code attempts.
    pub owner_mfa: RateLimitQuota,
    /// First-run owner setup attempts.
    pub setup_owner: RateLimitQuota,
    /// Upload session creation.
    pub upload_create: RateLimitQuota,
    /// Upload part writes.
    pub upload_part: RateLimitQuota,
    /// Upload completion requests.
    pub upload_complete: RateLimitQuota,
    /// Original export manifest reads.
    pub export_manifest: RateLimitQuota,
    /// Original export archive/blob downloads.
    pub export_download: RateLimitQuota,
    /// Private share creation.
    pub share_create: RateLimitQuota,
    /// Semantic search requests.
    pub semantic_search: RateLimitQuota,
    /// Model-pack install/self-test/activate/reindex admin actions.
    pub model_pack_admin: RateLimitQuota,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            owner_password_login: RateLimitQuota::new(
                5,
                Duration::minutes(15),
                Duration::minutes(15),
            ),
            owner_mfa: RateLimitQuota::new(10, Duration::minutes(15), Duration::minutes(15)),
            setup_owner: RateLimitQuota::per_hour(20),
            upload_create: RateLimitQuota::per_hour(2_000),
            upload_part: RateLimitQuota::per_hour(20_000),
            upload_complete: RateLimitQuota::per_hour(2_000),
            export_manifest: RateLimitQuota::per_hour(30),
            export_download: RateLimitQuota::per_hour(120),
            share_create: RateLimitQuota::per_hour(20),
            semantic_search: RateLimitQuota::per_hour(300),
            model_pack_admin: RateLimitQuota::per_hour(60),
        }
    }
}

impl RateLimitConfig {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            owner_password_login: quota_from_env(
                "MIRROR_RATE_LIMIT_OWNER_PASSWORD_LOGIN",
                defaults.owner_password_login,
            ),
            owner_mfa: quota_from_env("MIRROR_RATE_LIMIT_OWNER_MFA", defaults.owner_mfa),
            setup_owner: quota_from_env("MIRROR_RATE_LIMIT_SETUP_OWNER", defaults.setup_owner),
            upload_create: quota_from_env(
                "MIRROR_RATE_LIMIT_UPLOAD_CREATE",
                defaults.upload_create,
            ),
            upload_part: quota_from_env("MIRROR_RATE_LIMIT_UPLOAD_PART", defaults.upload_part),
            upload_complete: quota_from_env(
                "MIRROR_RATE_LIMIT_UPLOAD_COMPLETE",
                defaults.upload_complete,
            ),
            export_manifest: quota_from_env(
                "MIRROR_RATE_LIMIT_EXPORT_MANIFEST",
                defaults.export_manifest,
            ),
            export_download: quota_from_env(
                "MIRROR_RATE_LIMIT_EXPORT_DOWNLOAD",
                defaults.export_download,
            ),
            share_create: quota_from_env("MIRROR_RATE_LIMIT_SHARE_CREATE", defaults.share_create),
            semantic_search: quota_from_env(
                "MIRROR_RATE_LIMIT_SEMANTIC_SEARCH",
                defaults.semantic_search,
            ),
            model_pack_admin: quota_from_env(
                "MIRROR_RATE_LIMIT_MODEL_PACK_ADMIN",
                defaults.model_pack_admin,
            ),
        }
    }
}

impl RateLimitQuota {
    const fn new(max_per_window: i32, window: Duration, block_for: Duration) -> Self {
        Self {
            max_per_window,
            window,
            block_for,
        }
    }

    const fn per_hour(max_per_window: i32) -> Self {
        Self::new(max_per_window, Duration::hours(1), Duration::hours(1))
    }
}

/// Operator-selected ML execution device policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MlDevicePreference {
    /// Try GPU execution first and fall back to CPU if the runtime supports it.
    GpuWithCpuFallback,
    /// Use CPU execution only.
    CpuOnly,
    /// Require GPU execution and fail runtime startup if unavailable.
    GpuOnly,
}

impl MlDevicePreference {
    /// Stable environment/config representation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GpuWithCpuFallback => "gpu_with_cpu_fallback",
            Self::CpuOnly => "cpu_only",
            Self::GpuOnly => "gpu_only",
        }
    }
}

/// Redacted 256-bit key material for rate-limit key hashing.
#[derive(Clone)]
pub struct RateLimitSecret([u8; 32]);

impl RateLimitSecret {
    /// Builds stable key material from an operator-provided secret string.
    #[must_use]
    pub fn from_secret(secret: &str) -> Self {
        Self(Sha256::digest(secret.as_bytes()).into())
    }

    /// Generates process-local key material for development.
    #[must_use]
    pub fn random_or_dev_fallback() -> Self {
        let mut key = [0_u8; 32];
        if getrandom::getrandom(&mut key).is_ok() {
            Self(key)
        } else {
            Self::from_secret("mirror-dev-rate-limit-fallback")
        }
    }

    /// Returns BLAKE3 keyed-hash key bytes.
    #[must_use]
    pub fn as_key(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for RateLimitSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RateLimitSecret([redacted])")
    }
}

/// Redacted 256-bit key material for auth secret encryption.
#[derive(Clone)]
pub struct AuthSecret([u8; 32]);

impl AuthSecret {
    /// Builds stable key material from an operator-provided secret string.
    #[must_use]
    pub fn from_secret(secret: &str) -> Self {
        Self(Sha256::digest(secret.as_bytes()).into())
    }

    /// Derives local-dev key material from the configured rate-limit secret.
    #[must_use]
    pub fn from_rate_limit_secret(secret: &RateLimitSecret) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"mirror-auth-secret-v1");
        hasher.update(secret.as_key());
        Self(hasher.finalize().into())
    }

    /// Returns encryption key bytes.
    #[must_use]
    pub fn as_key(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for AuthSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthSecret([redacted])")
    }
}

impl Config {
    /// Builds config from environment variables with local-dev defaults.
    ///
    /// This does not validate Postgres/storage settings yet; those arrive in
    /// later tasks and will extend `/ready`.
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr = env::var("MIRROR_BIND_ADDR")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(default_bind_addr);
        let log_level = env::var("MIRROR_LOG").unwrap_or_else(|_| "info".to_owned());

        let database_url = env::var("MIRROR_DATABASE_URL").ok();
        let storage_root = env::var("MIRROR_STORAGE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/storage"));
        let rate_limit_secret = match env::var("MIRROR_RATE_LIMIT_SECRET") {
            Ok(secret) => {
                validate_operator_secret(&secret, ConfigError::WeakRateLimitSecret)?;
                RateLimitSecret::from_secret(&secret)
            }
            Err(_) => RateLimitSecret::random_or_dev_fallback(),
        };
        let auth_secret_env = env::var("MIRROR_AUTH_SECRET").ok();
        let auth_secret_configured = auth_secret_env.is_some();
        if let Some(secret) = auth_secret_env.as_deref() {
            validate_operator_secret(secret, ConfigError::WeakAuthSecret)?;
        }
        let auth_secret = auth_secret_env
            .as_deref()
            .map(AuthSecret::from_secret)
            .unwrap_or_else(|| AuthSecret::from_rate_limit_secret(&rate_limit_secret));
        let cookie_secure = env_bool("MIRROR_COOKIE_SECURE", true);
        let rate_limits = RateLimitConfig::from_env();
        let worker_concurrency = optional_positive_usize_env("MIRROR_WORKER_CONCURRENCY")?
            .unwrap_or_else(default_worker_concurrency);
        let trusted_proxies = env::var("MIRROR_TRUSTED_PROXIES")
            .ok()
            .map(|value| parse_trusted_proxies(&value))
            .transpose()?
            .unwrap_or_default();
        let ml_device = env::var("MIRROR_ML_DEVICE")
            .ok()
            .and_then(|value| parse_ml_device_preference(&value))
            .unwrap_or(MlDevicePreference::GpuWithCpuFallback);
        let ml_intra_threads = optional_positive_usize_env("MIRROR_ML_INTRA_THREADS")?;
        let ml_inter_threads = optional_positive_usize_env("MIRROR_ML_INTER_THREADS")?;
        let ml_parallel_execution = optional_bool_env("MIRROR_ML_PARALLEL_EXECUTION")?;
        let ml_max_concurrent_inferences =
            optional_positive_usize_env("MIRROR_ML_MAX_CONCURRENT_INFERENCES")?;
        let ml_max_image_bytes = env::var("MIRROR_ML_MAX_IMAGE_BYTES")
            .ok()
            .and_then(|value| parse_positive_usize(&value))
            .unwrap_or(25 * 1024 * 1024);
        let semantic_search = SemanticSearchConfig::from_env();
        let face_recognition_enabled = env_bool("MIRROR_FACE_RECOGNITION_ENABLED", false);

        Ok(Self {
            bind_addr,
            log_level,
            database_url,
            storage_root,
            rate_limit_secret,
            auth_secret,
            auth_secret_configured,
            cookie_secure,
            rate_limits,
            worker_concurrency,
            trusted_proxies,
            ml_device,
            ml_intra_threads,
            ml_inter_threads,
            ml_parallel_execution,
            ml_max_concurrent_inferences,
            ml_max_image_bytes,
            semantic_search,
            face_recognition_enabled,
        })
    }

    /// Validates settings required before serving DB-backed auth routes.
    pub fn validate_auth_secret_for_database(&self) -> Result<(), ConfigError> {
        if self.database_url.is_some() && !self.auth_secret_configured {
            Err(ConfigError::MissingAuthSecret)
        } else {
            Ok(())
        }
    }

    /// Returns the configured Mirror-level ML inference concurrency cap.
    ///
    /// `None` means callers should not install an application-level semaphore.
    #[must_use]
    pub fn ml_concurrency_limit(&self) -> Option<NonZeroUsize> {
        self.ml_max_concurrent_inferences
            .and_then(NonZeroUsize::new)
    }
}

fn validate_operator_secret(secret: &str, error: ConfigError) -> Result<(), ConfigError> {
    if secret.trim().len() < MIN_OPERATOR_SECRET_BYTES {
        Err(error)
    } else {
        Ok(())
    }
}

fn default_bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8080))
}

fn quota_from_env(prefix: &str, default: RateLimitQuota) -> RateLimitQuota {
    RateLimitQuota {
        max_per_window: env_i32(&format!("{prefix}_MAX"), default.max_per_window),
        window: Duration::seconds(env_i64(
            &format!("{prefix}_WINDOW_SECONDS"),
            default.window.whole_seconds(),
        )),
        block_for: Duration::seconds(env_i64(
            &format!("{prefix}_BLOCK_SECONDS"),
            default.block_for.whole_seconds(),
        )),
    }
}

fn parse_trusted_proxies(value: &str) -> Result<Vec<IpNet>, ConfigError> {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                None
            } else {
                Some(
                    part.parse()
                        .map_err(|_| ConfigError::InvalidTrustedProxy(part.to_owned())),
                )
            }
        })
        .collect()
}

fn parse_ml_device_preference(value: &str) -> Option<MlDevicePreference> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gpu_with_cpu_fallback" | "gpu-fallback" | "auto" => {
            Some(MlDevicePreference::GpuWithCpuFallback)
        }
        "cpu_only" | "cpu" => Some(MlDevicePreference::CpuOnly),
        "gpu_only" | "gpu" => Some(MlDevicePreference::GpuOnly),
        _ => None,
    }
}

fn parse_positive_usize(value: &str) -> Option<usize> {
    let parsed = value.trim().parse().ok()?;
    (parsed > 0).then_some(parsed)
}

fn optional_positive_usize_env(name: &'static str) -> Result<Option<usize>, ConfigError> {
    let Some(value) = env::var(name).ok() else {
        return Ok(None);
    };
    parse_positive_usize(&value)
        .map(Some)
        .ok_or(ConfigError::InvalidPositiveInteger(name))
}

fn optional_bool_env(name: &'static str) -> Result<Option<bool>, ConfigError> {
    let Some(value) = env::var(name).ok() else {
        return Ok(None);
    };
    parse_optional_bool_value(&value)
        .map(Some)
        .ok_or(ConfigError::InvalidBoolean(name))
}

fn default_worker_concurrency() -> usize {
    thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
}

fn parse_optional_bool_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn env_i32(name: &str, default: i32) -> i32 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_i64(name: &str, default: i64) -> i64 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_secrets_require_non_whitespace_minimum_length() {
        assert_eq!(
            validate_operator_secret("", ConfigError::WeakAuthSecret),
            Err(ConfigError::WeakAuthSecret)
        );
        assert_eq!(
            validate_operator_secret(
                "                                ",
                ConfigError::WeakAuthSecret
            ),
            Err(ConfigError::WeakAuthSecret)
        );
        assert_eq!(
            validate_operator_secret(
                "12345678901234567890123456789012",
                ConfigError::WeakAuthSecret
            ),
            Ok(())
        );
    }

    #[test]
    fn trusted_proxy_parser_rejects_invalid_non_empty_entries() {
        assert_eq!(
            parse_trusted_proxies("127.0.0.1/32, ::1/128").map(|parsed| parsed.len()),
            Ok(2)
        );

        assert_eq!(
            parse_trusted_proxies("127.0.0.1/32, not-a-cidr"),
            Err(ConfigError::InvalidTrustedProxy("not-a-cidr".to_owned()))
        );
    }

    #[test]
    fn ml_positive_thread_settings_reject_zero_and_invalid_values() {
        assert_eq!(parse_positive_usize("8"), Some(8));
        assert_eq!(parse_positive_usize("0"), None);
        assert_eq!(parse_positive_usize("many"), None);
    }

    #[test]
    fn default_worker_concurrency_is_positive() {
        assert!(default_worker_concurrency() > 0);
    }

    #[test]
    fn strict_optional_bool_parser_accepts_operator_spellings() {
        assert_eq!(parse_optional_bool_value("true"), Some(true));
        assert_eq!(parse_optional_bool_value("off"), Some(false));
        assert_eq!(parse_optional_bool_value("maybe"), None);
    }
}
