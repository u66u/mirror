//! Process configuration.
//!
//! V1 keeps config explicit and environment-backed. Secret-bearing config must
//! never be logged without redaction; see `docs/style-guide.md`.

use std::{env, fmt, net::SocketAddr, path::PathBuf};

use ipnet::IpNet;
use sha2::{Digest, Sha256};

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
    /// Proxy CIDRs allowed to supply forwarded client IP headers.
    pub trusted_proxies: Vec<IpNet>,
    /// Preferred ML execution device for future runtime-backed workers.
    pub ml_device: MlDevicePreference,
    /// Maximum encoded image bytes read into the embedding runtime.
    pub ml_max_image_bytes: usize,
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
            .field("trusted_proxies", &self.trusted_proxies)
            .field("ml_device", &self.ml_device)
            .field("ml_max_image_bytes", &self.ml_max_image_bytes)
            .finish()
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

impl Config {
    /// Builds config from environment variables with local-dev defaults.
    ///
    /// This does not validate Postgres/storage settings yet; those arrive in
    /// later tasks and will extend `/ready`.
    #[must_use]
    pub fn from_env() -> Self {
        let bind_addr = env::var("MIRROR_BIND_ADDR")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(default_bind_addr);
        let log_level = env::var("MIRROR_LOG").unwrap_or_else(|_| "info".to_owned());

        let database_url = env::var("MIRROR_DATABASE_URL").ok();
        let storage_root = env::var("MIRROR_STORAGE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/storage"));
        let rate_limit_secret = env::var("MIRROR_RATE_LIMIT_SECRET")
            .map(|secret| RateLimitSecret::from_secret(&secret))
            .unwrap_or_else(|_| RateLimitSecret::random_or_dev_fallback());
        let trusted_proxies = env::var("MIRROR_TRUSTED_PROXIES")
            .ok()
            .map(|value| parse_trusted_proxies(&value))
            .unwrap_or_default();
        let ml_device = env::var("MIRROR_ML_DEVICE")
            .ok()
            .and_then(|value| parse_ml_device_preference(&value))
            .unwrap_or(MlDevicePreference::GpuWithCpuFallback);
        let ml_max_image_bytes = env::var("MIRROR_ML_MAX_IMAGE_BYTES")
            .ok()
            .and_then(|value| parse_positive_usize(&value))
            .unwrap_or(25 * 1024 * 1024);

        Self {
            bind_addr,
            log_level,
            database_url,
            storage_root,
            rate_limit_secret,
            trusted_proxies,
            ml_device,
            ml_max_image_bytes,
        }
    }
}

fn default_bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8080))
}

fn parse_trusted_proxies(value: &str) -> Vec<IpNet> {
    value
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                None
            } else {
                part.parse().ok()
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
