use mirror_backend::auth::{SetupTokenVerifier, hash_password, verify_password};

mod support;
use support::TestResult;

#[test]
fn setup_token_rejects_wrong_token_and_cannot_be_reused() {
    let verifier = SetupTokenVerifier::from_token("correct-high-entropy-token");

    assert!(verifier.verify_available("wrong-token").is_err());
    assert!(
        verifier
            .verify_available("correct-high-entropy-token")
            .is_ok()
    );

    verifier.mark_consumed();

    assert!(
        verifier
            .verify_available("correct-high-entropy-token")
            .is_err()
    );
}

#[test]
fn password_hash_accepts_expected_password_only() -> TestResult {
    let password = "correct horse battery staple";
    let hash = hash_password(password)?;

    assert!(verify_password(password, &hash));
    assert!(!verify_password("wrong horse battery staple", &hash));
    assert!(!hash.contains(password));

    Ok(())
}
