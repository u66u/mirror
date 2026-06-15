use mirror_backend::auth::{self, OwnerSetupError, OwnerSetupInput, SetupState, verify_password};

mod support;
use support::{TestResult, connect_test_database};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn owner_setup_hashes_password_and_consumes_setup_token() -> TestResult {
    let pool = connect_test_database().await?;
    sqlx::query("TRUNCATE owner_accounts CASCADE")
        .execute(&pool)
        .await?;

    let (setup, setup_token) = SetupState::pending()?;
    let password = "correct horse battery staple";

    let output = auth::create_owner(
        &pool,
        &setup,
        OwnerSetupInput {
            setup_token: setup_token.clone(),
            display_name: "Owner".to_owned(),
            password: password.to_owned(),
        },
    )
    .await?;

    let stored_hash: String =
        sqlx::query_scalar("SELECT password_hash FROM owner_accounts WHERE public_id = $1")
            .bind(output.owner_public_id)
            .fetch_one(&pool)
            .await?;

    assert!(verify_password(password, &stored_hash));
    assert!(!stored_hash.contains(password));

    let second = auth::create_owner(
        &pool,
        &setup,
        OwnerSetupInput {
            setup_token,
            display_name: "Owner 2".to_owned(),
            password: password.to_owned(),
        },
    )
    .await;

    assert!(matches!(second, Err(OwnerSetupError::InvalidSetupToken)));

    Ok(())
}
