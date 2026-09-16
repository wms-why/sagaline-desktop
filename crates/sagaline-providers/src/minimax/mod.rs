//! minimax native providers (image / video / tts).
//!
//! Chat goes through `openai_compat::OpenAiCompatChat` (minimax exposes
//! an OpenAI-compatible chat endpoint). Image / video / tts use
//! minimax's own protocol because their body shapes don't match
//! OpenAI's.
//!
//! ## Image (this module)
//!
//! Uses minimax's native image endpoint (model id `image-01`).
//! Supports:
//!
//! - text prompt
//! - aspect ratio (`16:9`, `1:1`, …)
//! - seed
//! - response format: `base64` (default, no CDN round-trip)
//!
//! The endpoint URL is taken from config.toml's
//! `[image.<provider>].base_url`; we don't hardcode it. For tests
//! and offline use, see [`MinimaxImage::new_for_test`].
//!
//! ## Video / Tts (reserved)
//!
//! See the deferred list; the trait surface is in
//! `crate::image_to_video` and `crate::tts`.

use async_trait::async_trait;
use base64::Engine as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;

use crate::adapter::{Capability, GenerationOutput, GenerationRequest, ModelAdapter};
use crate::error::ProviderError;
use crate::image_gen::ImageGen;

/// minimax image backend.
#[derive(Clone)]
pub struct MinimaxImage {
    api_key: SecretString,
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl MinimaxImage {
    /// Construct a new backend. `base_url` is the API root
    /// (e.g. `https://api.minimax.chat/v1`); we append `/image/generations`.
    pub fn new(
        api_key: SecretString,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            base_url: base_url.into(),
            model: model.into(),
            http: reqwest::Client::builder()
                .user_agent(concat!("sagaline/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client builder"),
        }
    }

    /// Replace the model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl ModelAdapter for MinimaxImage {
    fn id(&self) -> &str {
        // The backend's stable id is the model (e.g. `image-01`).
        &self.model
    }
    fn provider_name(&self) -> &str {
        "minimax"
    }
    fn capabilities(&self) -> &[Capability] {
        &[Capability::Image]
    }
}

/// Wire shape returned by minimax's `/image/generations` endpoint
/// (response_format = "base64"). The exact shape may vary; this is the
/// documented contract at the time of writing.
#[derive(Debug, Deserialize)]
struct MinimaxImageResponse {
    /// `data` array; we ask for one image so we read `data[0]`.
    data: Vec<MinimaxImageEntry>,
}

#[derive(Debug, Deserialize)]
struct MinimaxImageEntry {
    /// Base64-encoded image bytes (no `data:image/png;base64,` prefix).
    b64_json: String,
}

#[async_trait]
impl ImageGen for MinimaxImage {
    async fn generate(
        &self,
        req: GenerationRequest<'_>,
    ) -> Result<GenerationOutput, ProviderError> {
        // Build the request body.
        let mut body = serde_json::json!({
            "model": self.model,
            "prompt": req.prompt,
            "n": 1,
            "response_format": "b64_json",
        });
        if let Some(ar) = req.aspect_ratio {
            body["aspect_ratio"] = serde_json::Value::String(ar.to_string());
        }
        if let Some(seed) = req.seed {
            body["seed"] = serde_json::json!(seed);
        }
        if let Some(np) = req.negative_prompt {
            body["negative_prompt"] = serde_json::Value::String(np.to_string());
        }
        // Caller can override api_key / model_id via `extra`. (The
        // agent tool passes these in.)
        if let Some(obj) = req.extra.as_object() {
            if let Some(body_obj) = body.as_object_mut() {
                for (k, v) in obj {
                    // Don't let `extra.api_key` leak into the wire body.
                    if k == "api_key" {
                        continue;
                    }
                    body_obj.insert(k.clone(), v.clone());
                }
            }
        }

        let url = format!(
            "{}/image/generations",
            self.base_url.trim_end_matches('/')
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.expose_secret())
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(if status.as_u16() == 429 {
                ProviderError::RateLimited {
                    provider: self.provider_name().to_string(),
                }
            } else {
                ProviderError::Http {
                    provider: self.provider_name().to_string(),
                    status: status.as_u16(),
                    body: resp_body,
                }
            });
        }

        let parsed: MinimaxImageResponse = serde_json::from_str(&resp_body).map_err(|e| {
            ProviderError::BadResponse {
                provider: self.provider_name().to_string(),
                message: format!("JSON parse: {e}; body head: {}",
                    &resp_body.chars().take(200).collect::<String>()),
            }
        })?;

        let entry = parsed.data.into_iter().next().ok_or_else(|| {
            ProviderError::BadResponse {
                provider: self.provider_name().to_string(),
                message: "no entries in `data` array".into(),
            }
        })?;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(entry.b64_json.as_bytes())
            .map_err(|e| ProviderError::BadResponse {
                provider: self.provider_name().to_string(),
                message: format!("base64 decode: {e}"),
            })?;

        Ok(GenerationOutput {
            bytes,
            mime: "image/png",
            provider_job_id: None,
        })
    }
}