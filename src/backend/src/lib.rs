//! Mirror backend library.
//!
//! Actix bootstrap lives in `bin`/`http`; feature logic must remain free of
//! Actix request/response types as described in `docs/module-boundaries.md`.

pub mod assets;
pub mod auth;
pub mod config;
pub mod db;
pub mod http;
pub mod jobs;
pub mod media;
pub mod runtime;
pub mod state;
pub mod storage;
pub mod telemetry;
pub mod uploads;
pub mod worker;
