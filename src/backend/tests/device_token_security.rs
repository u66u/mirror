use mirror_backend::auth::{
    self, DeviceTokenCreateInput, SessionCreateInput, authenticate_device_token,
    create_device_token, create_session, revoke_device_token,
};

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn device_token_can_be_tied_to_creating_session_after_password_check() -> TestResult {
    let pool = fresh_owner_pool().await?;
    assert!(!auth::verify_owner_password(&pool, "wrong horse battery staple").await?);
    assert!(auth::verify_owner_password(&pool, "correct horse battery staple").await?);

    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("device-token-creator".to_owned()),
            device_name: Some("browser".to_owned()),
        },
    )
    .await?;
    let device = create_device_token(
        &pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "Pixel".to_owned(),
            created_by_session_id: Some(session.session_id),
            user_agent: None,
        },
    )
    .await?;

    let created_by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT created_by_session_id FROM device_tokens WHERE id = $1")
            .bind(device.device_token_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(created_by, Some(session.session_id));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn device_token_authenticates_by_hash_and_rejects_wrong_or_revoked_token() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let created = create_device_token(
        &pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "Pixel".to_owned(),
            created_by_session_id: None,
            user_agent: Some("device-token-test".to_owned()),
        },
    )
    .await?;

    let raw_token = created.token.expose().to_owned();
    let stored_hash: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM device_tokens WHERE id = $1")
            .bind(created.device_token_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(stored_hash.len(), 32);
    assert_ne!(stored_hash, raw_token.as_bytes());
    assert_eq!(
        authenticate_device_token(&pool, &raw_token)
            .await?
            .map(|token| token.device_token_id),
        Some(created.device_token_id)
    );
    assert!(
        authenticate_device_token(&pool, "wrong-token")
            .await?
            .is_none()
    );

    revoke_device_token(&pool, created.device_token_id).await?;

    assert!(
        authenticate_device_token(&pool, &raw_token)
            .await?
            .is_none()
    );

    Ok(())
}
