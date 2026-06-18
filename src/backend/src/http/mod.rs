//! HTTP boundary.
//!
//! Actix types stay in this module tree. Feature modules receive typed inputs
//! and return typed outputs instead of depending on request/response types.

use actix_web::web;

pub mod assets;
pub mod auth;
pub mod client_ip;
pub mod error;
pub mod exports;
pub mod health;
pub mod search;
pub mod setup;
pub mod shares;
pub mod uploads;

/// Registers all HTTP routes for the API process.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(health::health)
        .service(health::ready)
        .service(auth::login)
        .service(auth::device_login)
        .service(auth::logout)
        .service(auth::sessions)
        .service(auth::create_device_token_route)
        .service(auth::revoke_device_token_route)
        .service(assets::list_assets_route)
        .service(assets::list_trashed_assets_route)
        .service(assets::get_derivative)
        .service(assets::trash_asset_route)
        .service(assets::restore_asset_route)
        .service(assets::favorite_asset_route)
        .service(assets::unfavorite_asset_route)
        .service(assets::purge_asset_route)
        .service(exports::original_manifest_route)
        .service(exports::original_archive_route)
        .service(exports::original_blob_route)
        .service(search::search_route)
        .service(shares::create_share_route)
        .service(shares::revoke_share_route)
        .service(shares::get_share_route)
        .service(shares::get_share_derivative_route)
        .service(setup::setup_owner)
        .service(uploads::create_upload_route)
        .service(uploads::get_upload_route)
        .service(uploads::put_part_route)
        .service(uploads::complete_upload_route)
        .service(uploads::cancel_upload_route);
}
