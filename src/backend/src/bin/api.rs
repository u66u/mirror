//! Mirror API process.
//!
//! This binary owns Actix startup, HTTP middleware, and process configuration.

use std::io;

use actix_web::{App, HttpServer, web};
use mirror_backend::{
    auth::{self, SetupState},
    config::Config,
    db, http,
    runtime::io_other,
    state::AppState,
    storage::ObjectStorage,
    telemetry,
};
use tracing::{info, warn};
use tracing_actix_web::TracingLogger;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = Config::from_env();
    telemetry::init(&config.log_level);
    let db = connect_database(&config).await?;
    let storage = connect_storage(&config)?;
    let setup = build_setup_state(db.as_ref()).await?;
    let state = AppState {
        config: config.clone(),
        db,
        setup,
        storage,
    };

    let bind_addr = config.bind_addr;
    info!(%bind_addr, "starting mirror api");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .wrap(TracingLogger::default())
            .configure(http::configure)
    })
    .bind(&bind_addr)?
    .run()
    .await
}

async fn connect_database(config: &Config) -> io::Result<Option<sqlx::PgPool>> {
    let Some(database_url) = config.database_url.as_deref() else {
        warn!("MIRROR_DATABASE_URL missing; database-backed routes disabled");
        return Ok(None);
    };

    let pool = db::connect(database_url).await.map_err(io_other)?;
    db::run_migrations(&pool).await.map_err(io_other)?;
    Ok(Some(pool))
}

async fn build_setup_state(pool: Option<&sqlx::PgPool>) -> io::Result<SetupState> {
    let Some(pool) = pool else {
        return Ok(SetupState::Disabled);
    };

    if auth::owner_exists(pool).await.map_err(io_other)? {
        info!("owner exists; first-run setup disabled");
        return Ok(SetupState::Disabled);
    }

    let (setup, token) = SetupState::pending().map_err(io_other)?;
    warn!(setup_token = %token, "first-run owner setup token generated");
    Ok(setup)
}

fn connect_storage(config: &Config) -> io::Result<Option<ObjectStorage>> {
    std::fs::create_dir_all(&config.storage_root)?;
    ObjectStorage::local(&config.storage_root)
        .map(Some)
        .map_err(io_other)
}
