//! One-time setup token verifier.
//!
//! C002: verifier stores a SHA-256 digest of a high-entropy random token, never
//! the raw token.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::getrandom;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Setup-token construction or verification error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupTokenError {
    /// OS random source failed.
    RandomFailed,
    /// Setup is disabled.
    Disabled,
    /// Supplied token is wrong or already consumed.
    Invalid,
}

impl std::fmt::Display for SetupTokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::RandomFailed => "setup token generation failed",
            Self::Disabled => "setup disabled",
            Self::Invalid => "invalid setup token",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SetupTokenError {}

/// First-run setup state shared by API workers.
#[derive(Clone)]
pub enum SetupState {
    /// Owner exists or setup cannot run.
    Disabled,
    /// Owner setup may proceed once with the matching token.
    Pending(Arc<SetupTokenVerifier>),
}

impl SetupState {
    /// Creates a pending verifier and returns the raw token for startup output.
    pub fn pending() -> Result<(Self, String), SetupTokenError> {
        let (verifier, token) = SetupTokenVerifier::generate()?;
        Ok((Self::Pending(Arc::new(verifier)), token))
    }

    /// Verifies setup is pending and token matches. Does not consume yet.
    pub fn verify_available(&self, candidate: &str) -> Result<(), SetupTokenError> {
        match self {
            Self::Disabled => Err(SetupTokenError::Disabled),
            Self::Pending(verifier) => verifier.verify_available(candidate),
        }
    }

    /// Marks setup token consumed after owner creation commits.
    pub fn mark_consumed(&self) {
        if let Self::Pending(verifier) = self {
            verifier.mark_consumed();
        }
    }
}

/// Verifier for a single high-entropy setup token.
pub struct SetupTokenVerifier {
    token_hash: [u8; 32],
    consumed: AtomicBool,
}

impl SetupTokenVerifier {
    /// Generates a high-entropy URL-safe token and its verifier.
    pub fn generate() -> Result<(Self, String), SetupTokenError> {
        let mut bytes = [0_u8; 32];
        getrandom(&mut bytes).map_err(|_| SetupTokenError::RandomFailed)?;
        let token = URL_SAFE_NO_PAD.encode(bytes);
        Ok((Self::from_token(&token), token))
    }

    /// Builds a verifier from an existing raw token.
    pub fn from_token(token: &str) -> Self {
        Self {
            token_hash: token_hash(token),
            consumed: AtomicBool::new(false),
        }
    }

    /// Verifies token is correct and not consumed. Does not mutate state.
    pub fn verify_available(&self, candidate: &str) -> Result<(), SetupTokenError> {
        if self.consumed.load(Ordering::Acquire) {
            return Err(SetupTokenError::Invalid);
        }

        if bool::from(token_hash(candidate).ct_eq(&self.token_hash)) {
            Ok(())
        } else {
            Err(SetupTokenError::Invalid)
        }
    }

    /// Consumes the verifier. Consumption is irreversible.
    pub fn mark_consumed(&self) {
        self.consumed.store(true, Ordering::Release);
    }
}

fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
