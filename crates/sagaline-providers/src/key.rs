//! BYOK API key type. Providers accept this by value; storage and
//! persistence live in the app shell (`~/.config/sagaline/`).

use std::env;

/// A user-supplied provider API key. Wrapped newtype so we never
/// accidentally log or print it (see [`Display`] impl, which masks).
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// Wrap a user-supplied key string.
    pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }

    /// Empty placeholder used in tests / offline mode.
    pub fn empty() -> Self { Self(String::new()) }

    /// Borrow the raw key. **Only** hand this to reqwest headers,
    /// never to logs or error messages.
    pub fn as_str(&self) -> &str { &self.0 }

    /// Read the key from an environment variable. Returns
    /// `Err(ProviderError::Auth)` if missing / empty so the caller
    /// gets a clear, actionable error.
    pub fn from_env_var(name: &str) -> Result<Self, sagaline_core::provider::ProviderError> {
        match env::var(name) {
            Ok(s) if !s.trim().is_empty() => Ok(Self(s)),
            Ok(_) | Err(_) => Err(sagaline_core::provider::ProviderError::Auth),
        }
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never leak the key in debug output.
        f.write_str("ApiKey(***)")
    }
}

impl std::fmt::Display for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***")
    }
}