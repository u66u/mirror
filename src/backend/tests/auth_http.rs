use actix_web::cookie::SameSite;
use mirror_backend::http::auth::{csrf_cookie, session_cookie};

#[test]
fn session_cookie_has_csrf_and_script_theft_defense_flags() {
    let cookie = session_cookie("opaque-session-token");

    assert_eq!(cookie.name(), "mirror_session");
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert!(cookie.max_age().is_some());
}

#[test]
fn csrf_cookie_is_readable_by_client_code_but_same_site_lax() {
    let cookie = csrf_cookie("opaque-csrf-token");

    assert_eq!(cookie.name(), "mirror_csrf");
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.http_only(), Some(false));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert!(cookie.max_age().is_some());
}
