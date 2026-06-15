use mirror_backend::auth::{OpaqueToken, TokenHash};

mod support;
use support::TestResult;

#[test]
fn opaque_token_hash_does_not_store_raw_token_and_verifies_by_digest() -> TestResult {
    let token = OpaqueToken::generate()?;
    let raw = token.expose();
    let hash = token.hash();

    assert_eq!(hash.as_bytes().len(), 32);
    assert!(raw.len() >= 40);
    assert!(
        raw.bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' })
    );
    assert!(hash.matches_raw(raw));
    assert!(!hash.matches_raw("wrong-token"));

    Ok(())
}

#[test]
fn same_raw_token_produces_same_lookup_hash() {
    let first = TokenHash::from_raw("same-token");
    let second = TokenHash::from_raw("same-token");
    let other = TokenHash::from_raw("other-token");

    assert_eq!(first, second);
    assert_ne!(first, other);
}
