//! minimax native providers (image / tts / image-to-video).
//!
//! Chat goes through `openai_compat::OpenAiCompatChat` (minimax exposes
//! an OpenAI-compatible chat endpoint). Image / tts / image-to-video
//! use minimax's own protocol because their body shapes don't match
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
//! `[image.<provider>].base_url`; we don't hardcode it.
//!
//! ## Tts (this module)
//!
//! One-shot text → audio. Endpoint shape mirrors OpenAI's
//! `/audio/speech` style: POST `{base_url}/audio/speech` with a
//! JSON body carrying `model`, `text`, `voice_id`. The response
//! is a binary audio/mpeg payload (no JSON envelope). Voice ids
//! are provider-specific (`English_expressive_narrator`,
//! `male-qn-jingying`, …); the backend does not validate them.
//!
//! ## Image-to-video (this module)
//!
//! Asynchronous: caller calls [`MinimaxVideo::submit`] and gets
//! back a [`crate::TaskHandle`], then polls via
//! [`MinimaxVideo::poll`] until [`crate::VideoStatus::Ready`] (or
//! [`crate::VideoStatus::Failed`]). Wire shape:
//!
//! - `POST {base_url}/video/generations` → `{task_id: "..."}`
//! - `GET  {base_url}/tasks/{task_id}`  → `{status, video_url?}`
//!
//! `first_frame` is uploaded as a base64-encoded image attached
//! to the submit body. The wire field name is
//! `first_frame_b64`.

use async_trait::async_trait;
use base64::Engine as _;
use secrecy::{ExposeSecret as _, SecretString};
use serde::Deserialize;

use crate::adapter::{Capability, GenerationOutput, GenerationRequest, ModelAdapter};
use crate::error::ProviderError;
use crate::image_gen::ImageGen;
use crate::image_to_video::{ImageToVideo, TaskHandle, VideoRequest, VideoStatus};
use crate::tts::Tts;

/// Shared client bits — every backend in this module hits the
/// same API root with the same bearer auth, so the http client +
/// auth header are DRY here.
#[derive(Clone)]
struct MinimaxClient {
    api_key: SecretString,
    base_url: String,
    http: reqwest::Client,
}

impl MinimaxClient {
    fn new(api_key: SecretString, base_url: impl Into<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                .user_agent(concat!("sagaline/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client builder"),
        }
    }

    /// Strip a trailing slash on `base_url` so `format!("{root}/{path}")`
    /// produces exactly one `/`.
    fn root(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }
}

/// minimax image backend.
#[derive(Clone)]
pub struct MinimaxImage {
    client: MinimaxClient,
    model: String,
}

impl MinimaxImage {
    pub fn new(
        api_key: SecretString,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: MinimaxClient::new(api_key, base_url),
            model: model.into(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl ModelAdapter for MinimaxImage {
    fn id(&self) -> &str {
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
    data: Vec<MinimaxImageEntry>,
}

#[derive(Debug, Deserialize)]
struct MinimaxImageEntry {
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

        let url = format!("{}/image/generations", self.client.root());
        let resp = self
            .client
            .http
            .post(&url)
            .bearer_auth(self.client.api_key.expose_secret())
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

// ────────────────────────────────────────────────────────────────
//  Tts
// ────────────────────────────────────────────────────────────────

/// minimax text-to-speech backend. One-shot — the trait doesn't
/// model streaming synthesis yet.
#[derive(Clone)]
pub struct MinimaxTts {
    client: MinimaxClient,
    model: String,
}

impl MinimaxTts {
    pub fn new(
        api_key: SecretString,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: MinimaxClient::new(api_key, base_url),
            model: model.into(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl ModelAdapter for MinimaxTts {
    fn id(&self) -> &str {
        &self.model
    }
    fn provider_name(&self) -> &str {
        "minimax"
    }
    fn capabilities(&self) -> &[Capability] {
        &[Capability::Tts]
    }
}

#[async_trait]
impl Tts for MinimaxTts {
    async fn synthesize(
        &self,
        text: &str,
        voice_id: &str,
    ) -> Result<GenerationOutput, ProviderError> {
        let body = serde_json::json!({
            "model": self.model,
            "text": text,
            "voice_id": voice_id,
        });
        let url = format!("{}/audio/speech", self.client.root());
        let resp = self
            .client
            .http
            .post(&url)
            .bearer_auth(self.client.api_key.expose_secret())
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(if status.as_u16() == 429 {
                ProviderError::RateLimited {
                    provider: self.provider_name().to_string(),
                }
            } else {
                ProviderError::Http {
                    provider: self.provider_name().to_string(),
                    status: status.as_u16(),
                    body,
                }
            });
        }

        let bytes = resp.bytes().await?;
        // Audio/mpeg (mp3) is what the wire returns when the body
        // omits `response_format` — adjust if the account
        // configures WAV. The trait caller (`sagaline-agent`)
        // writes the file with the right extension.
        Ok(GenerationOutput {
            bytes: bytes.to_vec(),
            mime: "audio/mpeg",
            provider_job_id: None,
        })
    }
}

// ────────────────────────────────────────────────────────────────
//  ImageToVideo
// ────────────────────────────────────────────────────────────────

/// minimax image-to-video backend. Asynchronous submit + poll.
#[derive(Clone)]
pub struct MinimaxVideo {
    client: MinimaxClient,
    model: String,
}

impl MinimaxVideo {
    pub fn new(
        api_key: SecretString,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: MinimaxClient::new(api_key, base_url),
            model: model.into(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

impl ModelAdapter for MinimaxVideo {
    fn id(&self) -> &str {
        &self.model
    }
    fn provider_name(&self) -> &str {
        "minimax"
    }
    fn capabilities(&self) -> &[Capability] {
        &[Capability::ImageToVideo]
    }
}

/// Response of `POST /video/generations`. The provider returns
/// just the task id; status arrives later via `GET /tasks/{id}`.
#[derive(Debug, Deserialize)]
struct MinimaxVideoSubmitResponse {
    task_id: String,
}

/// One row of `GET /tasks/{task_id}`. `status` is one of
/// `running` / `succeeded` / `failed`; on `succeeded`,
/// `video_url` carries the asset URL the caller must download.
#[derive(Debug, Deserialize)]
struct MinimaxVideoPollResponse {
    status: String,
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[async_trait]
impl ImageToVideo for MinimaxVideo {
    async fn submit(
        &self,
        req: VideoRequest<'_>,
    ) -> Result<TaskHandle, ProviderError> {
        // Upload the first frame as base64. Files are expected to
        // be small (a keyframe); we read them synchronously here —
        // the caller should have already staged them on disk and
        // `tokio::fs` already ran them through the bridge.
        let frame_bytes = std::fs::read(req.first_frame).map_err(|e| {
            ProviderError::Other(format!(
                "read first_frame {}: {e}",
                req.first_frame.display()
            ))
        })?;
        let frame_b64 = base64::engine::general_purpose::STANDARD.encode(&frame_bytes);

        let mut body = serde_json::json!({
            "model": self.model,
            "first_frame_b64": frame_b64,
            "prompt": req.prompt,
        });
        if let Some(d) = req.duration_seconds {
            body["duration_seconds"] = serde_json::json!(d);
        }
        if let Some(obj) = req.extra.as_object() {
            if let Some(body_obj) = body.as_object_mut() {
                for (k, v) in obj {
                    if k == "api_key" {
                        continue;
                    }
                    body_obj.insert(k.clone(), v.clone());
                }
            }
        }

        let url = format!("{}/video/generations", self.client.root());
        let resp = self
            .client
            .http
            .post(&url)
            .bearer_auth(self.client.api_key.expose_secret())
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(if status.as_u16() == 429 {
                ProviderError::RateLimited {
                    provider: self.provider_name().to_string(),
                }
            } else {
                ProviderError::Http {
                    provider: self.provider_name().to_string(),
                    status: status.as_u16(),
                    body: body_text,
                }
            });
        }

        let parsed: MinimaxVideoSubmitResponse = serde_json::from_str(&body_text)
            .map_err(|e| ProviderError::BadResponse {
                provider: self.provider_name().to_string(),
                message: format!("JSON parse: {e}; body head: {}",
                    &body_text.chars().take(200).collect::<String>()),
            })?;
        Ok(TaskHandle {
            provider: self.provider_name().to_string(),
            provider_task_id: parsed.task_id,
            model_id: self.model.clone(),
        })
    }

    async fn poll(
        &self,
        handle: &TaskHandle,
    ) -> Result<VideoStatus, ProviderError> {
        let url = format!("{}/tasks/{}", self.client.root(), handle.provider_task_id);
        let resp = self
            .client
            .http
            .get(&url)
            .bearer_auth(self.client.api_key.expose_secret())
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(if status.as_u16() == 429 {
                ProviderError::RateLimited {
                    provider: self.provider_name().to_string(),
                }
            } else {
                ProviderError::Http {
                    provider: self.provider_name().to_string(),
                    status: status.as_u16(),
                    body: body_text,
                }
            });
        }

        let parsed: MinimaxVideoPollResponse = serde_json::from_str(&body_text)
            .map_err(|e| ProviderError::BadResponse {
                provider: self.provider_name().to_string(),
                message: format!("JSON parse: {e}; body head: {}",
                    &body_text.chars().take(200).collect::<String>()),
            })?;
        Ok(match parsed.status.as_str() {
            "running" | "queued" | "pending" => VideoStatus::Running,
            "succeeded" | "success" | "ready" => match parsed.video_url {
                Some(url) => VideoStatus::Ready { url },
                // Provider claims success but didn't send a URL —
                // treat as a bad response so the caller sees the
                // inconsistency rather than spinning forever.
                None => VideoStatus::Failed {
                    reason: "provider reported success but no video_url".into(),
                },
            },
            "failed" | "error" => VideoStatus::Failed {
                reason: parsed.reason.unwrap_or_else(|| "unknown".into()),
            },
            other => VideoStatus::Failed {
                reason: format!("unknown status string from provider: {other:?}"),
            },
        })
    }
}