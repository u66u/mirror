use mirror_backend::auth::{
    SessionCreateInput, authenticate_session, create_session, revoke_session, verify_session_csrf,
};

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn session_authenticates_by_hash_and_rejects_wrong_or_revoked_token() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let created = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("session-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;

    let raw_token = created.token.expose().to_owned();
    let stored_hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions WHERE id = $1")
        .bind(created.session_id)
        .fetch_one(&pool)
        .await?;

    assert_eq!(stored_hash.len(), 32);
    assert_ne!(stored_hash, raw_token.as_bytes());
    assert_eq!(
        authenticate_session(&pool, &raw_token)
            .await?
            .map(|session| session.session_id),
        Some(created.session_id)
    );
    assert!(authenticate_session(&pool, "wrong-token").await?.is_none());
    assert!(verify_session_csrf(&pool, created.session_id, created.csrf_token.expose()).await?);
    assert!(!verify_session_csrf(&pool, created.session_id, "wrong-csrf-token").await?);

    revoke_session(&pool, created.session_id).await?;

    assert!(authenticate_session(&pool, &raw_token).await?.is_none());
    assert!(!verify_session_csrf(&pool, created.session_id, created.csrf_token.expose()).await?);

    Ok(())
}
