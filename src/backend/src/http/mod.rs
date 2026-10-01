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
pub mod model_packs;
pub mod people;
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
        .service(auth::mfa_status_route)
        .service(auth::setup_totp_route)
        .service(auth::enable_totp_route)
        .service(auth::disable_totp_route)
        .service(auth::rotate_recovery_codes_route)
        .service(auth::logout)
        .service(auth::sessions)
        .service(auth::create_device_token_route)
        .service(auth::revoke_device_token_route)
        .service(assets::list_assets_route)
        .service(assets::list_trashed_assets_route)
        .service(assets::get_derivative)
        .service(assets::get_trashed_derivative)
        .service(assets::get_original)
        .service(assets::trash_asset_route)
        .service(assets::restore_asset_route)
        .service(assets::favorite_asset_route)
        .service(assets::unfavorite_asset_route)
        .service(assets::purge_asset_route)
        .service(exports::original_manifest_route)
        .service(exports::original_archive_route)
        .service(exports::original_blob_route)
        .service(model_packs::list_model_packs_route)
        .service(model_packs::install_model_pack_route)
        .service(model_packs::record_model_pack_self_test_route)
        .service(model_packs::run_model_pack_self_test_route)
        .service(model_packs::activate_model_pack_route)
        .service(model_packs::start_model_reindex_route)
        .service(model_packs::list_model_reindex_runs_route)
        .service(people::list_people_route)
        .service(people::list_unassigned_faces_route)
        .service(people::get_face_chip_route)
        .service(people::list_person_faces_route)
        .service(people::rename_person_route)
        .service(people::hide_person_route)
        .service(people::merge_people_route)
        .service(people::split_faces_route)
        .service(people::unassign_faces_route)
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
