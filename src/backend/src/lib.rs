//! Mirror backend library.
//!
//! Actix bootstrap lives in `bin`/`http`; feature logic must remain free of
//! Actix request/response types as described in `docs/module-boundaries.md`.

pub mod assets;
pub mod auth;
pub mod backups;
pub mod config;
pub mod db;
pub mod exports;
pub mod http;
pub mod integrity;
pub mod jobs;
pub mod media;
mod public_derivatives;
pub mod rate_limit;
pub mod runtime;
pub mod shares;
pub mod state;
pub mod storage;
pub mod telemetry;
pub mod uploads;
pub mod video;
pub mod worker;
