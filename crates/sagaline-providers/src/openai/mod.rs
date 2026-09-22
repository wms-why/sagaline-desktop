//! OpenAI gpt-image-1 image backend.
//!
//! Routes through rig-core's OpenAI provider's
//! `image_generation_model()` against the
//! `/v1/images/generations` endpoint. The endpoint URL is taken
//! from `config.toml`'s `[image.openai].base_url`; we don't
//! hardcode it. The api_key is held at construction (same
//! pattern as [`crate::minimax::MinimaxImage`]).
//!
//! ## Wire shape
//!
//! Rig builds `{"model": "...", "prompt": "...", "size":
//! "WxH"}` and POSTs to `<base_url>/images/generations`. Aspect
//! ratio is mapped to a `(width, height)` pair from
//! gpt-image-1's supported set (`1024x1024`, `1024x1536`,
//! `1536x1024`). Reference images and negative prompts are not
//! supported by gpt-image-1; the trait exposes them for
//! providers that do, but this backend ignores them.
//!
//! ## extras bridge
//!
//! `GenerateImageTool` puts `api_key` and `model_id` in
//! `GenerationRequest::extra`. We use `model_id` to override
//! the construction-time model per call, and strip `api_key`
//! (it never reaches the wire body — same scrubbing as
//! [`crate::minimax::MinimaxImage::generate`]). Any remaining
//! keys are forwarded as rig `additional_params`, which lets a
//! caller inject `quality` / `background` / `output_format` /
//! `user` etc. without an adapter change.
//!
//! ## Pre-registration
//!
//! This module does not pre-register itself in
//! [`crate::ProviderRegistry`]. The desktop app's [`AppEnv`]
//! holds an empty registry today (see the deferred note in
//! `client/AGENTS.md`); wiring the runtime registration belongs
//! to that follow-up.

use async_trait::async_trait;
use rig_core::client::image_generation::ImageGenerationClient;
use rig_core::image_generation::ImageGenerationModel as _;
use secrecy::{ExposeSecret as _, SecretString};

use rig_core::providers::openai;

use crate::adapter::{Capability, GenerationOutput, GenerationRequest, ModelAdapter};
use crate::error::ProviderError;
use crate::image_gen::ImageGen;

/// Default canvas when no aspect ratio is provided. Matches
/// gpt-image-1's smallest "1:1" option.
const DEFAULT_WIDTH: u32 = 1024;
const DEFAULT_HEIGHT: u32 = 1024;

/// OpenAI image backend.
#[derive(Clone)]
pub struct OpenAiImage {
    api_key: SecretString,
    base_url: String,
    model: String,
}

impl OpenAiImage {
    /// Construct a new backend. `base_url` is the API root
    /// (e.g. `https://api.openai.com/v1`); rig appends
    /// `/images/generations`.
    pub fn new(
        api_key: SecretString,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    /// Replace the model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl ModelAdapter for OpenAiImage {
    fn id(&self) -> &str {
        &self.model
    }
    fn provider_name(&self) -> &str {
        "openai"
    }
    fn capabilities(&self) -> &[Capability] {
        &[Capability::Image]
    }
}

#[async_trait]
impl ImageGen for OpenAiImage {
    async fn generate(
        &self,
        req: GenerationRequest<'_>,
    ) -> Result<GenerationOutput, ProviderError> {
        // Build a rig OpenAI client against the configured
        // base_url + api_key. Mirrors `openai_compat::build_chat`
        // for the chat side.
        let secret = self.api_key.expose_secret().to_string();
        let bearer: rig_core::client::BearerAuth = secret.into();
        let client = openai::Client::builder()
            .api_key(bearer)
            .base_url(&self.base_url)
            .build()
            .map_err(|e| ProviderError::Config(format!(
                "openai client build failed: {e}"
            )))?;

        // Resolve the model id: caller-supplied override in
        // `extra.model_id` wins over the construction-time
        // default. The agent tool passes `model_id` through
        // `extra` so a per-call override works without
        // rebuilding the registry.
        let model_id = req
            .extra
            .get("model_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.model)
            .to_string();
        let image_model = client.image_generation_model(&model_id);

        // Derive (width, height) from the aspect ratio.
        // gpt-image-1 accepts 1024x1024, 1024x1536, 1536x1024.
        let (width, height) = dims_for_aspect(req.aspect_ratio);

        // Build the rig request. Extra fields that aren't
        // `api_key` or `model_id` (the bridge) flow through as
        // `additional_params` — lets a caller add `quality` /
        // `background` etc. without an adapter change.
        let mut builder = image_model
            .image_generation_request()
            .prompt(req.prompt)
            .width(width)
            .height(height);

        if let Some(obj) = req.extra.as_object() {
            let mut params = obj.clone();
            params.remove("api_key");
            params.remove("model_id");
            if !params.is_empty() {
                builder = builder.additional_params(serde_json::Value::Object(params));
            }
        }

        let rig_resp = builder
            .send()
            .await
            .map_err(|e| map_rig_error(self.provider_name(), e))?;

        Ok(GenerationOutput {
            bytes: rig_resp.image,
            mime: "image/png",
            provider_job_id: None,
        })
    }
}

/// Map a common aspect ratio string to a `(width, height)` pair
/// from the gpt-image-1 supported set. Unknown / None → 1:1.
fn dims_for_aspect(aspect: Option<&str>) -> (u32, u32) {
    match aspect {
        Some("1:1") => (1024, 1024),
        // Portrait-ish: gpt-image-1 has no 9:16 or 3:4; 2:3 is
        // the closest in its set.
        Some("2:3") | Some("9:16") | Some("3:4") => (1024, 1536),
        // Landscape-ish: same idea, mirrored.
        Some("3:2") | Some("16:9") | Some("4:3") => (1536, 1024),
        _ => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
    }
}

/// Translate rig's
/// [`rig_core::image_generation::ImageGenerationError`] into our
/// [`ProviderError`] so the agent sees consistent shapes
/// regardless of which provider ran. Uses rig's
/// `provider_response_status` / `provider_response_body`
/// helpers to preserve the original HTTP semantics.
fn map_rig_error(
    provider: &str,
    e: rig_core::image_generation::ImageGenerationError,
) -> ProviderError {
    let status = e.provider_response_status().map(|s| s.as_u16());
    let body = e.provider_response_body().unwrap_or("").to_string();
    if let Some(s) = status {
        if s == 429 {
            return ProviderError::RateLimited {
                provider: provider.to_string(),
            };
        }
        return ProviderError::Http {
            provider: provider.to_string(),
            status: s,
            body,
        };
    }
    // No status: rig-generated diagnostic (build / parse /
    // request-construction error). Surface as Other so the
    // agent log still shows the message.
    ProviderError::Other(format!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratio_maps_to_supported_sizes() {
        assert_eq!(dims_for_aspect(Some("1:1")), (1024, 1024));
        assert_eq!(dims_for_aspect(Some("2:3")), (1024, 1536));
        assert_eq!(dims_for_aspect(Some("3:2")), (1536, 1024));
        assert_eq!(dims_for_aspect(Some("9:16")), (1024, 1536));
        assert_eq!(dims_for_aspect(Some("16:9")), (1536, 1024));
        assert_eq!(dims_for_aspect(None), (1024, 1024));
        assert_eq!(dims_for_aspect(Some("garbage")), (1024, 1024));
    }
}