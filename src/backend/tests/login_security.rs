use mirror_backend::auth::{self, OwnerLoginError, OwnerLoginInput, authenticate_session};

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn owner_login_rejects_wrong_password_and_creates_valid_session() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let wrong = auth::login_owner(
        &pool,
        OwnerLoginInput {
            password: "wrong horse battery staple".to_owned(),
            user_agent: None,
            device_name: None,
        },
    )
    .await;
    assert!(matches!(wrong, Err(OwnerLoginError::InvalidCredentials)));

    let session = auth::login_owner(
        &pool,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            user_agent: Some("login-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;

    assert_eq!(
        authenticate_session(&pool, session.token.expose())
            .await?
            .map(|auth_session| auth_session.session_id),
        Some(session.session_id)
    );

    Ok(())
}
