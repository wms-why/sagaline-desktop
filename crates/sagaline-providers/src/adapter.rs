//! Core adapter trait — identity + capabilities.
//!
//! Every provider backend implements [`ModelAdapter`]. Capability-
//! specific traits ([`crate::ImageGen`], [`crate::Chat`], etc.) extend
//! it. The registry queries capability flags before routing a call.

use std::path::Path;

/// What a backend can produce. Multiple per backend are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    /// Text → image (synchronous).
    Image,
    /// Image → video (asynchronous task).
    ImageToVideo,
    /// Text → video (asynchronous task). Reserved.
    TextToVideo,
    /// Text → audio.
    Tts,
    /// Chat completions (OpenAI-compatible).
    Chat,
    /// Vision critique (image in, text out). Reserved.
    VisionCritique,
}

/// Provider-agnostic generation request. Common fields only;
/// provider-specific knobs go through [`GenerationRequest::extra`].
#[derive(Debug, Clone)]
pub struct GenerationRequest<'a> {
    /// Natural-language prompt.
    pub prompt: &'a str,
    /// Optional negative prompt (some image providers honour it).
    pub negative_prompt: Option<&'a str>,
    /// Reference images (character consistency, IP-Adapter, etc.).
    pub reference_images: &'a [&'a Path],
    /// Seed for reproducibility, when the provider supports it.
    pub seed: Option<u64>,
    /// Output aspect ratio as a string (e.g. `"16:9"`). OpenAI ignores
    /// it; minimax honours it for image gen.
    pub aspect_ratio: Option<&'a str>,
    /// Escape hatch for provider-specific JSON fields that don't
    /// belong in the common surface.
    pub extra: &'a serde_json::Value,
}

/// Provider-agnostic output for a synchronous capability (image, audio,
/// text). Asynchronous capabilities ([`crate::ImageToVideo`]) use
/// [`crate::TaskHandle`] + [`crate::VideoStatus`] instead.
#[derive(Debug, Clone)]
pub struct GenerationOutput {
    /// Output bytes (PNG, MP3, WAV, …). Caller writes them to disk.
    pub bytes: Vec<u8>,
    /// MIME type, e.g. `"image/png"`, `"audio/mpeg"`.
    pub mime: &'static str,
    /// Provider's own task / generation id. Useful for resume / poll
    /// even on synchronous endpoints (some providers return ids even
    /// when the asset is inline).
    pub provider_job_id: Option<String>,
}

/// Identity + capabilities. Implemented by every provider backend.
pub trait ModelAdapter: Send + Sync {
    /// Stable id used in frontmatter and logs. E.g. `"minimax-image-01"`,
    /// `"openai-gpt-image-1"`.
    fn id(&self) -> &str;

    /// Logical provider name (no version). E.g. `"minimax"`, `"openai"`,
    /// `"deepseek"`. Used to look up keys + base_url in config.
    fn provider_name(&self) -> &str;

    /// Bitmask of supported capabilities.
    fn capabilities(&self) -> &[Capability];
}