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
pub mod face;
pub mod http;
pub mod integrity;
pub mod jobs;
pub mod media;
pub mod ml;
pub mod models;
pub mod paths;
pub mod people;
mod public_derivatives;
pub mod rate_limit;
pub mod runtime;
pub mod search;
pub mod semantic_index;
pub mod shares;
pub mod state;
pub mod storage;
pub mod telemetry;
pub mod uploads;
pub mod video;
pub mod worker;
