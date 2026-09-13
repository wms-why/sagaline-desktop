//! Provider abstractions for BYOK model access.
//!
//! This module is pure data + trait. It does **not** import any HTTP, I/O,
//! or async runtime implementation detail. Concrete providers (MiniMax,
//! OpenAI, …) live in the `sagaline-providers` crate and implement these
//! traits against the real network APIs.
//!
//! Three independent capabilities are split into sub-traits so providers
//! can opt-in per ability:
//!
//! | Sub-trait              | What it does                                  |
//! | ---------------------- | --------------------------------------------- |
//! | [`TextToText`]         | Prompt + optional history → generated text     |
//! | [`TextToImage`]        | Prompt + optional refs → one-or-more images    |
//! | [`TextImageToVideo`]   | Prompt + optional image refs → video artifact  |
//!
//! A provider implementing all three sub-traits also implements
//! [`ModelProvider`], which exposes identity + capability flags.
//!
//! Video generation on real providers (e.g. MiniMax) is asynchronous: the
//! caller submits a job and polls until it completes. [`TextImageToVideo`]
//! therefore has a **two-stage shape**:
//!
//! 1. [`TextImageToVideo::submit_video`] — returns a [`VideoJob`] handle
//!    immediately. No blocking wait.
//! 2. The caller drives [`VideoJob::poll`] on whatever schedule it likes
//!    (typically every few seconds with backoff) until the provider
//!    reports a terminal status, then reads the video bytes.
//!
//! This deliberately keeps the trait unopinionated about retry policy —
//! that belongs in the app layer.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------

macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            #[inline]
            pub fn new(s: impl Into<String>) -> Self { Self(s.into()) }
            #[inline]
            pub fn as_str(&self) -> &str { &self.0 }
        }

        impl From<&str> for $name { fn from(s: &str) -> Self { Self(s.to_string()) } }
        impl From<String> for $name { fn from(s: String) -> Self { Self(s) } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    }
}
id_newtype!(
    /// Stable string identifier for a provider, unique within a registry.
    /// Built-ins use `"minimax"` / `"mixed"`; user-defined mixes use the
    /// name they were created under.
    ProviderId
);
id_newtype!(
    /// A model name as understood by the target provider (e.g. `"MiniMax-H3"`,
    /// `"image-01"`, `"MiniMax-M3"`).
    ModelId
);
id_newtype!(
    /// Handle returned from an async video submission. Opaque to callers.
    VideoJobId
);

// ---------------------------------------------------------------------------
// Provider identity
// ---------------------------------------------------------------------------

/// What *kind* of provider this is. Used for UI labels and the unique-key
/// rule in the registry (`Mixed` ids are reserved).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProviderKind {
    /// Concrete direct provider — MiniMax official API.
    MiniMax,
    /// A routing provider that delegates each capability to a different
    /// concrete provider. The slots are stored in [`ProviderKind::Mixed`]
    /// via [`crate::provider::MixedConfig`] when the provider is built.
    Mixed,
    /// Reserved for future direct providers (OpenAI, Gemini, Kling, …).
    /// Never matched at runtime in v1.
    Other { name: String },
}

impl ProviderKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::MiniMax => "MiniMax",
            Self::Mixed => "Mixed",
            Self::Other { .. } => "Other",
        }
    }
}

/// Capability flags. Computed once at provider construction; used by
/// `MixedProvider` to validate slot assignments and by the UI to gray out
/// unsupported menu items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub text_to_text: bool,
    pub text_to_image: bool,
    pub text_image_to_video: bool,
}

impl ProviderCapabilities {
    pub const fn all() -> Self {
        Self { text_to_text: true, text_to_image: true, text_image_to_video: true }
    }
}

// ---------------------------------------------------------------------------
// Top-level trait
// ---------------------------------------------------------------------------

/// Identity + capabilities. Implemented by every concrete provider, plus
/// the Mixed router. The three sub-traits are independent — providers
/// only implement the ones they support.
pub trait ModelProvider: TextToText + TextToImage + TextImageToVideo + Send + Sync {
    fn id(&self) -> &ProviderId;
    fn kind(&self) -> &ProviderKind;
    fn capabilities(&self) -> ProviderCapabilities;

    /// Short human-readable name for UI / logs. Defaults to the provider id.
    fn label(&self) -> &str { self.id().as_str() }
}

// ---------------------------------------------------------------------------
// Sub-traits
// ---------------------------------------------------------------------------

/// Text → text generation (scriptwriting, story bible expansion, etc.).
pub trait TextToText: Send + Sync {
    fn generate_text(
        &self,
        req: TextRequest,
    ) -> impl Future<Output = Result<TextResponse, ProviderError>> + Send;
}

/// Text + optional reference images → one or more images.
pub trait TextToImage: Send + Sync {
    fn generate_image(
        &self,
        req: ImageRequest,
    ) -> impl Future<Output = Result<ImageResponse, ProviderError>> + Send;
}

/// Text + optional image refs → a video. Asynchronous: returns a job
/// handle; caller polls via [`VideoJob::poll`].
pub trait TextImageToVideo: Send + Sync {
    fn submit_video(
        &self,
        req: VideoRequest,
    ) -> impl Future<Output = Result<VideoJob, ProviderError>> + Send;
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// A request for text generation. Modeled loosely on the Anthropic
/// Messages shape so the same struct fits MiniMax (Anthropic-compat),
/// OpenAI, Gemini, and most other LLM APIs after a per-provider
/// translation layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextRequest {
    /// Model to use. If `None`, the provider picks its default.
    pub model: Option<ModelId>,
    /// Optional system prompt.
    pub system: Option<String>,
    /// User prompt. Required (callers validate).
    pub prompt: String,
    /// Max output tokens. `None` → provider default.
    pub max_tokens: Option<u32>,
    /// 0.0 – 2.0. `None` → provider default.
    pub temperature: Option<f32>,
    /// Nucleus sampling. `None` → provider default.
    pub top_p: Option<f32>,
    /// Stop sequences. Empty means none.
    pub stop: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResponse {
    pub text: String,
    pub model: ModelId,
    /// Free-form: `"end_turn"`, `"max_tokens"`, `"stop_sequence"`, …
    pub stop_reason: Option<String>,
    pub usage: Option<Usage>,
}

/// Image generation request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageRequest {
    pub model: Option<ModelId>,
    /// ≤ 1500 chars for MiniMax image-01.
    pub prompt: String,
    /// Negative prompt (provider-specific support).
    pub negative_prompt: Option<String>,
    /// `width × height` is preferred for providers that take pixel dims;
    /// `aspect_ratio` is preferred when the provider takes named ratios.
    /// Providers translate / pick one of these as appropriate.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub aspect_ratio: Option<AspectRatio>,
    pub seed: Option<u64>,
    /// How many images to generate. 1 by default.
    pub n: u32,
    /// Optional reference images (character sheet, environment, etc.).
    /// Interpretation is provider-specific.
    pub reference_images: Vec<ImageArtifact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AspectRatio {
    #[serde(rename = "1:1")]   R1x1,
    #[serde(rename = "3:2")]   R3x2,
    #[serde(rename = "2:3")]   R2x3,
    #[serde(rename = "16:9")]  R16x9,
    #[serde(rename = "9:16")]  R9x16,
    #[serde(rename = "4:3")]   R4x3,
    #[serde(rename = "3:4")]   R3x4,
    #[serde(rename = "21:9")]  R21x9,
    #[serde(rename = "adaptive")] Adaptive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResponse {
    pub images: Vec<ImageArtifact>,
    pub seed: Option<u64>,
    pub usage: Option<Usage>,
}

// ---------------------------------------------------------------------------
// Video types — async two-stage shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoRequest {
    pub model: Option<ModelId>,
    /// Required text prompt (≤ 7000 chars for MiniMax).
    pub prompt: String,
    pub negative_prompt: Option<String>,
    /// Reference images. Interpreted by each provider per its role scheme
    /// (first frame / last frame / character reference). Provider picks
    /// the role assignment based on its own capability.
    pub reference_images: Vec<ImageArtifact>,
    pub duration_secs: Option<u32>,
    pub resolution: Option<VideoResolution>,
    pub aspect_ratio: Option<AspectRatio>,
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoResolution {
    R480p,
    R768p,
    R1080p,
    R2k,
    R4k,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoResponse {
    pub video: VideoArtifact,
    pub seed: Option<u64>,
    pub usage: Option<VideoUsage>,
}

/// A submitted video job. The caller drives `poll` until `is_terminal()`
/// reports true, then reads `final_result()`.
#[derive(Debug)]
pub struct VideoJob {
    /// Internal provider-side handle (e.g. MiniMax task_id).
    pub id: VideoJobId,
    /// Provider that owns this job (used by the poll impl).
    state: VideoJobState,
}

impl VideoJob {
    /// Construct a new job. Only provider implementations should call this.
    pub fn new(id: VideoJobId, state: VideoJobState) -> Self { Self { id, state } }

    /// Current status (most recent poll or initial submitted).
    pub fn status(&self) -> VideoJobStatus { self.state.status() }

    /// True iff the job is in a terminal state (success, failed, or
    /// cancelled). Caller should stop polling.
    pub fn is_terminal(&self) -> bool { self.state.is_terminal() }

    /// If status is Succeeded, return the response. Otherwise None.
    pub fn final_result(&self) -> Option<&VideoResponse> { self.state.final_result() }

    /// If status is Failed, return the error string from the provider.
    pub fn failure(&self) -> Option<&str> { self.state.failure() }

    /// How long the next caller should wait before polling. Zero means
    /// "poll immediately". Providers pick a sane default based on job
    /// age and prior poll timing.
    pub fn suggested_poll_after(&self) -> Duration { self.state.suggested_poll_after() }

    /// Replace internal state after a poll. Provider-internal use only.
    pub fn update_state(&mut self, state: VideoJobState) { self.state = state; }
}
/// Provider-owned state machine for an in-flight video job. Concrete
/// providers implement this against their own API (MiniMax uses task_id
/// + GET /v2/query/video_generation/{id}). Lives behind a Box in
/// `VideoJobState::Running` so the trait can stay object-safe without
/// pulling HTTP deps into core.
pub trait VideoJobHandle: std::fmt::Debug + Send + Sync {
    /// Poll the provider once. Returns the next state. Implementations
    /// decide what "next state" means — the poll happens synchronously
    /// (the actual network IO is performed inside).
    fn poll(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<VideoJobState, ProviderError>> + Send + '_>>;

    /// Hint for the caller about how long to wait before the next poll.
    /// Implementations should consider backoff when many polls return
    /// "still running".
    fn suggested_poll_after(&self) -> Duration;
}

/// Internal state of a video job. Concrete providers own the actual
/// implementation; the enum allows the trait object to carry it
/// without leaking provider-specific types into core.
#[derive(Debug)]
pub enum VideoJobState {
    Submitted,
    /// Polled once or more; still running. Carries provider-specific
    /// state needed to continue polling (e.g. provider-owned client).
    Running(Box<dyn VideoJobHandle + Send + Sync>),

    /// Terminal success — `VideoResponse` is the final output.
    Done(VideoResponse),
    /// Terminal failure — provider message stored as String.
    Failed(String),
    /// Cancelled by the user / provider.
    Cancelled,
}

impl VideoJobState {
    pub fn status(&self) -> VideoJobStatus {
        match self {
            Self::Submitted => VideoJobStatus::Queued,
            Self::Running(_) => VideoJobStatus::Running,
            Self::Done(_) => VideoJobStatus::Succeeded,
            Self::Failed(_) => VideoJobStatus::Failed,
            Self::Cancelled => VideoJobStatus::Cancelled,
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed(_) | Self::Cancelled)
    }
    pub fn final_result(&self) -> Option<&VideoResponse> {
        match self { Self::Done(r) => Some(r), _ => None }
    }
    pub fn failure(&self) -> Option<&str> {
        match self { Self::Failed(m) => Some(m.as_str()), _ => None }
    }
    pub fn suggested_poll_after(&self) -> Duration {
        match self {
            Self::Running(h) => h.suggested_poll_after(),
            _ => Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}


// ---------------------------------------------------------------------------
// Media artifacts
// ---------------------------------------------------------------------------

/// A decoded image. Bytes are the raw encoded file (PNG / JPEG / WEBP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub mime: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// A decoded video file. Bytes are the raw encoded container (mp4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoArtifact {
    pub mime: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub duration_secs: f32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

// ---------------------------------------------------------------------------
// Usage accounting
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoUsage {
    pub output_seconds: Option<u32>,
    pub input_image_count: Option<u32>,
    pub input_seconds: Option<u32>,
    pub total_tokens: Option<u32>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider config invalid: {0}")]
    Config(String),
    #[error("auth failed (missing or invalid API key)")]
    Auth,
    #[error("rate limited; retry after {retry_after_secs:?}s")]
    RateLimit { retry_after_secs: Option<u32> },
    #[error("http status {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("invalid response from provider: {0}")]
    Decode(String),
    #[error("provider {provider} does not support capability {capability}")]
    Unsupported {
        provider: ProviderId,
        capability: &'static str,
    },
    #[error("video job failed: {0}")]
    VideoJobFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
// serde helper for `Vec<u8>` in ImageArtifact / VideoArtifact.
// Uses base64 string representation (compact, standard across serde).
mod serde_bytes {
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;
    use serde::{Deserialize, Deserializer, Serializer};
    use serde::de::Error as _;

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&B64.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        B64.decode(&s).map_err(D::Error::custom)
    }
}