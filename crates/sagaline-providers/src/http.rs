//! Shared HTTP client used by every provider. Holds the API key in a
//! closure so we don't have to thread it through every request builder.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use sagaline_core::provider::ProviderError;

use crate::error::{map_response, map_transport};
use crate::key::ApiKey;

/// Shared HTTP client + auth state. Cheap to clone — reqwest::Client
/// is already an Arc internally.
#[derive(Clone)]
pub struct Client {
    inner: reqwest::Client,
    base: reqwest::Url,
    key: ApiKey,
}

impl Client {
    pub fn new(
        base: impl AsRef<str>,
        key: ApiKey,
        timeout: std::time::Duration,
    ) -> Result<Self, ProviderError> {
        let base = reqwest::Url::parse(base.as_ref())
            .map_err(|e| ProviderError::Config(format!("invalid base url: {e}")))?;
        let inner = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(map_transport)?;
        Ok(Self { inner, base, key })
    }

    pub fn base(&self) -> &reqwest::Url { &self.base }
    pub fn key(&self) -> &ApiKey { &self.key }

    /// Apply default headers (Authorization + a fresh request id).
    pub fn headers(&self, request_id: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if !self.key.as_str().is_empty() {
            let v = HeaderValue::from_str(&format!("Bearer {}", self.key.as_str()))
                .expect("API key contains no control chars");
            h.insert(AUTHORIZATION, v);
        }
        if let Ok(v) = HeaderValue::from_str(request_id) {
            h.insert(HeaderName::from_static("x-request-id"), v);
        }
        h
    }

    /// Build a POST request with the path joined onto the base URL.
    pub fn post(&self, path: &str) -> Result<reqwest::RequestBuilder, ProviderError> {
        let url = self.base.join(path)
            .map_err(|e| ProviderError::Config(format!("invalid path {path}: {e}")))?;
        Ok(self.inner.post(url))
    }

    /// Build a GET request with the path joined onto the base URL.
    pub fn get(&self, path: &str) -> Result<reqwest::RequestBuilder, ProviderError> {
        let url = self.base.join(path)
            .map_err(|e| ProviderError::Config(format!("invalid path {path}: {e}")))?;
        Ok(self.inner.get(url))
    }

    /// Send a built request, automatically attach Authorization +
    /// x-request-id, and run the response through our error mapper.
    pub async fn send(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ProviderError> {
        let req_id = uuid::Uuid::new_v4().simple().to_string();
        let resp = req.headers(self.headers(&req_id)).send().await.map_err(map_transport)?;
        map_response(resp).await
    }

    /// Download a URL to bytes. Used by image gen when the provider
    /// returns a 24h-expiring CDN URL we want to immediately cache.
    pub async fn download_bytes(
        &self,
        url: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        let resp = self.inner.get(url).send().await.map_err(map_transport)?;
        let resp = map_response(resp).await?;
        let bytes = resp.bytes().await.map_err(map_transport)?;
        Ok(bytes.to_vec())
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.base)
            .field("key", &self.key) // Display impl masks it
            .finish()
    }
}
