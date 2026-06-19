use mirror_backend::{
    auth::{
        self, OwnerLoginError, OwnerLoginInput, SecondFactorInput, authenticate_session, mfa_status,
    },
    config::AuthSecret,
};

mod support;
use support::{TestResult, current_totp_code, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn owner_login_rejects_wrong_password_and_creates_valid_session() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let wrong = auth::login_owner(
        &pool,
        &AuthSecret::from_secret("test-auth-secret"),
        OwnerLoginInput {
            password: "wrong horse battery staple".to_owned(),
            second_factor: None,
            user_agent: None,
            device_name: None,
        },
    )
    .await;
    assert!(matches!(wrong, Err(OwnerLoginError::InvalidCredentials)));

    let session = auth::login_owner(
        &pool,
        &AuthSecret::from_secret("test-auth-secret"),
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: None,
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

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn owner_login_requires_totp_or_recovery_code_when_enabled() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let auth_secret = AuthSecret::from_secret("test-auth-secret");

    let setup = auth::begin_totp_setup(&pool, &auth_secret).await?;
    assert!(setup.provisioning_uri.starts_with("otpauth://totp/"));
    let code = current_totp_code(&setup.secret_base32)?;
    let recovery = auth::enable_totp(&pool, &auth_secret, &code).await?;
    assert_eq!(recovery.recovery_codes.len(), 10);

    let status = mfa_status(&pool).await?;
    assert!(status.totp_enabled);
    assert!(!status.totp_setup_pending);
    assert_eq!(status.recovery_codes_remaining, 10);

    let password_only = auth::login_owner(
        &pool,
        &auth_secret,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: None,
            user_agent: None,
            device_name: None,
        },
    )
    .await;
    assert!(matches!(
        password_only,
        Err(OwnerLoginError::InvalidCredentials)
    ));

    let wrong_totp = auth::login_owner(
        &pool,
        &auth_secret,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: Some(SecondFactorInput {
                totp_code: Some("000000".to_owned()),
                recovery_code: None,
            }),
            user_agent: None,
            device_name: None,
        },
    )
    .await;
    assert!(matches!(
        wrong_totp,
        Err(OwnerLoginError::InvalidCredentials)
    ));

    let session = auth::login_owner(
        &pool,
        &auth_secret,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: Some(SecondFactorInput {
                totp_code: Some(current_totp_code(&setup.secret_base32)?),
                recovery_code: None,
            }),
            user_agent: None,
            device_name: None,
        },
    )
    .await?;
    assert!(
        authenticate_session(&pool, session.token.expose())
            .await?
            .is_some()
    );

    let recovery_code = recovery.recovery_codes[0].clone();
    let recovery_session = auth::login_owner(
        &pool,
        &auth_secret,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: Some(SecondFactorInput {
                totp_code: None,
                recovery_code: Some(recovery_code.clone()),
            }),
            user_agent: None,
            device_name: None,
        },
    )
    .await?;
    assert!(
        authenticate_session(&pool, recovery_session.token.expose())
            .await?
            .is_some()
    );
    assert_eq!(mfa_status(&pool).await?.recovery_codes_remaining, 9);

    let reused_recovery = auth::login_owner(
        &pool,
        &auth_secret,
        OwnerLoginInput {
            password: "correct horse battery staple".to_owned(),
            second_factor: Some(SecondFactorInput {
                totp_code: None,
                recovery_code: Some(recovery_code),
            }),
            user_agent: None,
            device_name: None,
        },
    )
    .await;
    assert!(matches!(
        reused_recovery,
        Err(OwnerLoginError::InvalidCredentials)
    ));

    Ok(())
}
