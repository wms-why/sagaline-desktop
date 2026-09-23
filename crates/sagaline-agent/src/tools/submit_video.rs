//! `submit_video` agent tool.
//!
//! Submits an asynchronous image-to-video job to a registered
//! provider backend and returns a `provider_task_id`. The actual
//! result lands via a follow-up call to [`poll_video`](super::poll_video).
//!
//! ## Wire shape
//!
//! Args (JSON):
//!
//! ```json
//! {
//!   "provider":          "minimax",
//!   "model":             "video-01",   // optional, overrides config.toml default
//!   "prompt":            "...",
//!   "first_frame_path":  "/abs/keyframe.png",
//!   "duration_seconds":  3,             // optional
//!   "key_id":            "default"      // optional
//! }
//! ```
//!
//! Output:
//!
//! ```json
//! {
//!   "provider":         "minimax",
//!   "provider_task_id": "task-abc",
//!   "model_id":         "video-01"
//! }
//! ```
//!
//! ## Lifecycle
//!
//! The tool does NOT download the produced video. Callers are
//! expected to invoke [`poll_video`](super::poll_video) with the
//! returned `provider_task_id` until the status is `ready` (or
//! `failed`). The LLM is in charge of the polling cadence; a
//! blocking convenience wrapper is a future phase.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use secrecy::ExposeSecret as _;
use serde::Serialize;
use serde_json::{json, Value};

use sagaline_providers::{
    Capability, ProviderConfigSet, ProviderError, ProviderRegistry, VideoRequest,
};
use sagaline_store::{ProviderKeyId, World};

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

/// Tool arguments.
#[derive(Debug, Clone, ::serde::Deserialize, ::serde::Serialize, ::schemars::JsonSchema)]
pub struct SubmitVideoArgs {
    /// Logical provider name (e.g. `"minimax"`). Must be registered in
    /// the [`ProviderRegistry`] under `Capability::ImageToVideo`.
    pub provider: String,

    /// Override the per-provider default model id (resolved against
    /// `config.toml` otherwise).
    #[serde(default)]
    pub model: Option<String>,

    /// Motion / camera / mood prompt.
    pub prompt: String,

    /// Absolute path to the first-frame image (a keyframe we
    /// already generated). Uploaded as base64 by the backend.
    pub first_frame_path: PathBuf,

    /// Optional target duration in seconds. Some providers ignore it;
    /// others fail if it's missing.
    #[serde(default)]
    pub duration_seconds: Option<u32>,

    /// Optional key id to use when looking up the API key in the
    /// store. Defaults to `"default"` if absent.
    #[serde(default)]
    pub key_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubmitVideoOutput {
    provider: String,
    provider_task_id: String,
    model_id: String,
}

/// The tool. Cheap to clone.
#[derive(Clone)]
pub struct SubmitVideoTool {
    registry: Arc<ProviderRegistry>,
    config: Arc<ProviderConfigSet>,
    store: Arc<World>,
}

impl SubmitVideoTool {
    pub fn new(
        registry: Arc<ProviderRegistry>,
        config: Arc<ProviderConfigSet>,
        store: Arc<World>,
    ) -> Self {
        Self {
            registry,
            config,
            store,
        }
    }
}

#[async_trait]
impl Tool for SubmitVideoTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<SubmitVideoArgs>(
            "submit_video",
            "Submit an asynchronous image-to-video job to the named provider's \
             image-to-video backend using the image at `first_frame_path` as the \
             starting frame. Returns `{provider, provider_task_id, model_id}` — \
             the caller is expected to follow up with `poll_video` using the \
             returned `provider_task_id` until the task reaches `ready` or \
             `failed`. Use `provider` to pick the backend (e.g. \"minimax\"); \
             `model` overrides the per-provider default. The first frame is \
             uploaded as base64; keep it to a single keyframe.",
            crate::tool::Capability::Execute,
        )
    }

    async fn execute(
        &self,
        _ctx: crate::tool::ToolContext,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed: SubmitVideoArgs =
            serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
                name: "submit_video".into(),
                message: e.to_string(),
            })?;

        self.run(parsed).await
    }
}

impl SubmitVideoTool {
    async fn run(&self, args: SubmitVideoArgs) -> Result<ToolResult, ToolError> {
        // 1. Pick the backend as `Arc<dyn ImageToVideo>`.
        let backend = self
            .registry
            .pick_image_to_video(&args.provider)
            .map_err(|e: ProviderError| ToolError::Execution {
                name: "submit_video".into(),
                source: Box::new(e),
            })?;

        // 2. Resolve model id.
        let model_id = args
            .model
            .clone()
            .or_else(|| {
                self.config
                    .get(Capability::ImageToVideo, &args.provider)
                    .map(|c| c.model.clone())
            })
            .unwrap_or_else(|| backend.id().to_string());

        // 3. Look up the API key.
        let key_id = ProviderKeyId::new(
            args.provider.clone(),
            args.key_id.clone().unwrap_or_else(|| "default".to_string()),
        )
        .map_err(|e| ToolError::Execution {
            name: "submit_video".into(),
            source: Box::new(e),
        })?;
        let key = self
            .store
            .keys()
            .get(&key_id)
            .map_err(|e| ToolError::Execution {
                name: "submit_video".into(),
                source: Box::new(e),
            })?;

        // 4. Sanity-check the first frame exists; the backend
        //    reads it synchronously, but a friendlier error here
        //    beats a cryptic I/O message from inside the
        //    provider's `std::fs::read`.
        if !args.first_frame_path.is_file() {
            return Err(ToolError::Io {
                name: "submit_video".into(),
                path: args.first_frame_path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "first_frame_path is not a file: {}",
                        args.first_frame_path.display()
                    ),
                ),
            });
        }

        // 5. Build the VideoRequest. The `extra` JSON carries the
        //    resolved model id and the api_key for backends that
        //    want to re-bind (e.g. via `with_model`); the bare
        //    `api_key` field is intentionally filtered by the
        //    provider's submit handler so it doesn't leak into
        //    the wire body.
        let api_key_str = key.reveal().expose_secret().to_string();
        let extra = json!({ "api_key": api_key_str, "model_id": model_id });
        let prompt = args.prompt.clone();
        let duration = args.duration_seconds;
        let req = VideoRequest {
            first_frame: &args.first_frame_path,
            prompt: &prompt,
            duration_seconds: duration,
            extra: &extra,
        };

        // 6. Submit.
        let handle = backend.submit(req).await.map_err(|e| ToolError::Execution {
            name: "submit_video".into(),
            source: Box::new(e),
        })?;

        let result = SubmitVideoOutput {
            provider: handle.provider.clone(),
            provider_task_id: handle.provider_task_id.clone(),
            model_id: handle.model_id.clone(),
        };
        serde_json::to_value(&result).map_err(|e| ToolError::Execution {
            name: "submit_video".into(),
            source: Box::new(e),
        })
    }
}