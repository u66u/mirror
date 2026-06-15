//! HTTP boundary.
//!
//! Actix types stay in this module tree. Feature modules receive typed inputs
//! and return typed outputs instead of depending on request/response types.

use actix_web::web;

pub mod assets;
pub mod auth;
pub mod error;
pub mod health;
pub mod setup;
pub mod uploads;

/// Registers all HTTP routes for the API process.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(health::health)
        .service(health::ready)
        .service(auth::login)
        .service(auth::logout)
        .service(auth::sessions)
        .service(auth::create_device_token_route)
        .service(assets::list_assets_route)
        .service(assets::get_derivative)
        .service(setup::setup_owner)
        .service(uploads::create_upload_route)
        .service(uploads::get_upload_route)
        .service(uploads::put_part_route)
        .service(uploads::complete_upload_route)
        .service(uploads::cancel_upload_route);
}
