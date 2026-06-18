//! Password hashing.
//!
//! Raw passwords must stay request-local. Store only Argon2id PHC strings.

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use thiserror::Error;

/// Password hashing or policy failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PasswordError {
    /// Password violates local policy.
    #[error("invalid password")]
    InvalidPassword,
    /// Hashing failed.
    #[error("password hashing failed")]
    HashFailed,
}

/// Hashes a user password with Argon2id.
pub fn hash_password(password: &str) -> Result<String, PasswordError> {
    validate_password(password)?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| PasswordError::HashFailed)
}

/// Verifies a password against an Argon2 PHC string.
pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

fn validate_password(password: &str) -> Result<(), PasswordError> {
    if password.chars().count() < 12 {
        return Err(PasswordError::InvalidPassword);
    }
    Ok(())
}
