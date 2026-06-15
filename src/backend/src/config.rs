//! Process configuration.
//!
//! V1 keeps config explicit and environment-backed. Secret-bearing config must
//! never be logged without redaction; see `docs/style-guide.md`.

use std::{env, net::SocketAddr, path::PathBuf};

/// Runtime configuration shared by the API process.
#[derive(Clone, Debug)]
pub struct Config {
    /// Address Actix binds to.
    pub bind_addr: SocketAddr,
    /// Tracing filter, for example `info` or `mirror_backend=debug`.
    pub log_level: String,
    /// Postgres URL. Redact before logging.
    pub database_url: Option<String>,
    /// Local vault storage root. Must not be inside a public web root.
    pub storage_root: PathBuf,
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

        Self {
            bind_addr,
            log_level,
            database_url,
            storage_root,
        }
    }
}

fn default_bind_addr() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8080))
}
