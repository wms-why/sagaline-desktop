//! Internal error helpers. The crate re-exports types from
//! `sagaline_core::provider` for the public surface; this module is
//! for implementation-internal mapping (reqwest → ProviderError).

use sagaline_core::provider::ProviderError;

/// Map a `reqwest::Response` outcome into our public error type. Reads
/// the response body for diagnostics but never leaks the API key.
pub async fn map_response(
    resp: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    let status = resp.status();
    if status.is_success() { return Ok(resp); }

    let url = resp.url().clone();
    let body = resp.text().await.unwrap_or_default();
    let body_short = if body.len() > 512 { &body[..512] } else { &body };

    match status.as_u16() {
        401 | 403 => Err(ProviderError::Auth),
        429 => Err(ProviderError::RateLimit { retry_after_secs: None }),
        s => Err(ProviderError::HttpStatus {
            status: s,
            body: format!("{}: {}", url, body_short),
        }),
    }
}

/// Translate a `reqwest::Error` (transport / decode) into our public
/// error. We never propagate the raw error because it can embed the
/// request URL and any headers (incl. Authorization) it tried to send.
pub fn map_transport(err: reqwest::Error) -> ProviderError {
    ProviderError::Transport(err.without_url().to_string())
}