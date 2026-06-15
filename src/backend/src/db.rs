//! Postgres connection and migration boundary.
//!
//! Keep SQLx here or inside feature modules. Do not hide database access behind
//! generic repositories; v1 favors explicit queries and migrations.

use std::time::Duration;

use sqlx::{PgPool, postgres::PgPoolOptions};

/// Embedded SQLx migrations for the backend crate.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Opens the API Postgres pool.
///
/// Callers own policy: startup may fail hard once Postgres becomes mandatory,
/// while tests may connect only when a test database URL is present.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

/// Applies all embedded migrations to the connected database.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

/// Lightweight readiness query for Postgres.
pub async fn ping(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1").execute(pool).await?;
    Ok(())
}
