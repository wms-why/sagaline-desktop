//! Provider error type.

use thiserror::Error;

/// Errors raised by a provider backend. The agent surfaces this as an
/// `Act` event failure; it never panics.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Provider returned a non-2xx status. The body is included verbatim
    /// for debug logging; secrets are not expected in error bodies.
    #[error("provider `{provider}` returned HTTP {status}: {body}")]
    Http {
        provider: String,
        status: u16,
        body: String,
    },

    /// Network-level failure (DNS, TLS, timeout).
    #[error("provider `{provider}` network error: {message}")]
    Network { provider: String, message: String },

    /// The provider's response didn't match the expected schema. Almost
    /// always a version skew between this crate and the API.
    #[error("provider `{provider}` returned unexpected response: {message}")]
    BadResponse { provider: String, message: String },

    /// Provider rejected the request as bad.
    #[error("provider `{provider}` rejected the request: {message}")]
    Rejected { provider: String, message: String },

    /// The provider rate-limited; callers should back off and retry.
    #[error("provider `{provider}` rate-limited")]
    RateLimited { provider: String },

    /// The selected provider id (from frontmatter) has no registered
    /// backend in the [`crate::ProviderRegistry`].
    #[error("provider `{0}` is not registered")]
    Unknown(String),

    /// The caller asked for a capability the backend doesn't implement.
    #[error("provider `{provider}` does not support capability `{capability:?}`")]
    Unsupported {
        provider: String,
        capability: crate::Capability,
    },

    /// Configuration problem (missing base_url, missing key, malformed
    /// config.toml). Not an I/O failure on the story side.
    #[error("provider config error: {0}")]
    Config(String),

    /// Wrapped [`crate::ProviderConfigError`] for convenience.
    #[error(transparent)]
    ProviderConfig(#[from] crate::ProviderConfigError),

    /// Key lookup failure. Wrapped [`sagaline_keys::KeyError`].
    #[error(transparent)]
    Key(#[from] sagaline_keys::KeyError),

    /// Anything else.
    #[error("provider error: {0}")]
    Other(String),
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        ProviderError::Network {
            provider: "<unknown>".into(),
            message: e.to_string(),
        }
    }
}