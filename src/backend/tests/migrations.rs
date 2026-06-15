use mirror_backend::db::ping;

mod support;
use support::{TestResult, connect_test_database};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at an empty test database"]
async fn migrations_apply_to_database() -> TestResult {
    let pool = connect_test_database().await?;
    ping(&pool).await?;

    Ok(())
}
