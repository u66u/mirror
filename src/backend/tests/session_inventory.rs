use mirror_backend::auth::{SessionCreateInput, create_session, list_sessions, revoke_session};

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn session_inventory_marks_current_and_excludes_revoked_sessions() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let current = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("current".to_owned()),
            device_name: Some("current browser".to_owned()),
        },
    )
    .await?;
    let revoked = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("revoked".to_owned()),
            device_name: Some("revoked browser".to_owned()),
        },
    )
    .await?;
    revoke_session(&pool, revoked.session_id).await?;

    let sessions = list_sessions(&pool, 1, current.session_id).await?;

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, current.session_id);
    assert!(sessions[0].is_current);

    Ok(())
}
