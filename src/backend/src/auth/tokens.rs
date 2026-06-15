//! Opaque bearer/session token helpers.
//!
//! Raw tokens are generated at boundaries, returned once, and never stored.
//! Persist `TokenHash` bytes only.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use getrandom::getrandom;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Random URL-safe opaque token returned to clients once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpaqueToken(String);

impl OpaqueToken {
    /// Generates 256 bits of randomness encoded without padding.
    pub fn generate() -> Result<Self, TokenError> {
        let mut bytes = [0_u8; 32];
        getrandom(&mut bytes).map_err(|_| TokenError::RandomFailed)?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    /// Borrows the raw token for response/cookie construction.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Hashes the token for storage.
    #[must_use]
    pub fn hash(&self) -> TokenHash {
        TokenHash::from_raw(&self.0)
    }
}

/// SHA-256 token digest for DB storage and lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    /// Builds a digest from a raw bearer/session token.
    #[must_use]
    pub fn from_raw(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).into())
    }

    /// Returns digest bytes for SQL binding.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Constant-time digest comparison.
    #[must_use]
    pub fn matches_raw(&self, token: &str) -> bool {
        bool::from(Self::from_raw(token).0.ct_eq(&self.0))
    }
}

/// Token generation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// OS random source failed.
    RandomFailed,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("token generation failed")
    }
}

impl std::error::Error for TokenError {}
