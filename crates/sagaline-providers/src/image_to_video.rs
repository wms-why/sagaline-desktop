//! Image-to-video capability (asynchronous).
//!
//! Reserved for a later phase (the first phase produces static
//! keyframes + voice only). The shape is locked in here so the agent
//! can already type-check against it.

use async_trait::async_trait;
use std::path::Path;

use crate::adapter::ModelAdapter;
use crate::error::ProviderError;

/// A submitted async task.
#[derive(Debug, Clone)]
pub struct TaskHandle {
    pub provider: String,
    pub provider_task_id: String,
    pub model_id: String,
}

/// Inputs for image-to-video. Kept minimal; per-provider knobs go
/// through `extra` (provider-specific JSON).
#[derive(Debug, Clone)]
pub struct VideoRequest<'a> {
    /// First-frame image (a keyframe we already generated).
    pub first_frame: &'a Path,
    /// Motion / camera / mood prompt.
    pub prompt: &'a str,
    /// Target duration in seconds.
    pub duration_seconds: Option<u32>,
    /// Provider-specific knobs.
    pub extra: &'a serde_json::Value,
}

/// Result of polling a submitted task.
#[derive(Debug, Clone)]
pub enum VideoStatus {
    /// Still processing; caller should sleep and re-poll.
    Running,
    /// Provider has produced an asset at `url` (caller downloads).
    Ready { url: String },
    /// Provider rejected the task.
    Failed { reason: String },
}

#[async_trait]
pub trait ImageToVideo: ModelAdapter {
    async fn submit(&self, req: VideoRequest<'_>) -> Result<TaskHandle, ProviderError>;
    async fn poll(&self, handle: &TaskHandle) -> Result<VideoStatus, ProviderError>;
}