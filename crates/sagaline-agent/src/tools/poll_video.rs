//! `poll_video` agent tool.
//!
//! Polls a previously-submitted image-to-video task for status.
//! The LLM is expected to invoke this tool repeatedly until the
//! status reaches `ready` (download the asset at `url`) or
//! `failed` (read `reason`). Mirrors the submit/poll split in
//! [`submit_video`](super::submit_video).
//!
//! ## Wire shape
//!
//! Args (JSON):
//!
//! ```json
//! {
//!   "provider":         "minimax",
//!   "provider_task_id": "task-abc",
//!   "key_id":           "default"  // optional
//! }
//! ```
//!
//! Output (one of three shapes):
//!
//! ```json
//! { "status": "running" }
//! ```
//!
//! ```json
//! { "status": "ready", "url": "https://..." }
//! ```
//!
//! ```json
//! { "status": "failed", "reason": "..." }
//! ```
//!
//! The flat enum (rather than nested objects) makes the JSON
//! easier for an LLM to discriminate against.

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::ExposeSecret as _;
use serde::Serialize;
use serde_json::{json, Value};

use sagaline_providers::{
    ProviderError, ProviderRegistry, TaskHandle, VideoStatus,
};
use sagaline_store::{ProviderKeyId, World};

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

/// Tool arguments.
#[derive(Debug, Clone, ::serde::Deserialize, ::serde::Serialize, ::schemars::JsonSchema)]
pub struct PollVideoArgs {
    /// Logical provider name (must match the one used in
    /// `submit_video`).
    pub provider: String,

    /// Task id returned by `submit_video`.
    pub provider_task_id: String,

    /// Optional key id to use when looking up the API key in the
    /// store. Defaults to `"default"` if absent.
    #[serde(default)]
    pub key_id: Option<String>,
}

/// Discriminated output — flattened for LLM consumption.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PollVideoStatusJson {
    /// Still processing; the LLM should re-invoke this tool after
    /// a short delay.
    Running,
    /// Provider finished; `url` carries the asset the caller
    /// should download.
    Ready { url: String },
    /// Provider rejected the task; `reason` is human-readable.
    Failed { reason: String },
}

/// The tool. Cheap to clone.
#[derive(Clone)]
pub struct PollVideoTool {
    registry: Arc<ProviderRegistry>,
    /// Held for symmetry with [`SubmitVideoTool`] — the
    /// binary's [`AppEnv::build_agent`] registers both tools
    /// with the same `(registry, config, store)` tuple. The
    /// poll path resolves the model through the backend
    /// directly (the registry already mapped provider name →
    /// backend with its own key), so `config` isn't read
    /// here. Future phases may consume it for per-call
    /// model overrides.
    #[allow(dead_code)]
    config: Arc<sagaline_providers::ProviderConfigSet>,
    store: Arc<World>,
}

impl PollVideoTool {
    pub fn new(
        registry: Arc<ProviderRegistry>,
        config: Arc<sagaline_providers::ProviderConfigSet>,
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
impl Tool for PollVideoTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<PollVideoArgs>(
            "poll_video",
            "Poll a previously-submitted image-to-video task for status. \
             Returns `{status: \"running\"}`, `{status: \"ready\", url}` (the asset \
             the caller should download), or `{status: \"failed\", reason}`. \
             Pair with `submit_video` — the LLM is in charge of the polling cadence.",
            crate::tool::Capability::Execute,
        )
    }

    async fn execute(
        &self,
        _ctx: crate::tool::ToolContext,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed: PollVideoArgs =
            serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
                name: "poll_video".into(),
                message: e.to_string(),
            })?;

        self.run(parsed).await
    }
}

impl PollVideoTool {
    async fn run(&self, args: PollVideoArgs) -> Result<ToolResult, ToolError> {
        // 1. Pick the backend. We don't actually use it for HTTP,
        //    but the registry dispatch is the source of truth —
        //    asking for an unregistered provider here is the same
        //    failure mode the user would see from
        //    `submit_video`.
        let backend = self
            .registry
            .pick_image_to_video(&args.provider)
            .map_err(|e: ProviderError| ToolError::Execution {
                name: "poll_video".into(),
                source: Box::new(e),
            })?;

        // 2. Validate the key exists. We don't pass the key into
        //    the poll call (the backend's `poll` is already wired
        //    to the right api_key at registration time), but we
        //    want a friendly error before hitting the wire if the
        //    key is missing.
        let key_id = ProviderKeyId::new(
            args.provider.clone(),
            args.key_id.clone().unwrap_or_else(|| "default".to_string()),
        )
        .map_err(|e| ToolError::Execution {
            name: "poll_video".into(),
            source: Box::new(e),
        })?;
        let _key = self
            .store
            .keys()
            .get(&key_id)
            .map_err(|e| ToolError::Execution {
                name: "poll_video".into(),
                source: Box::new(e),
            })?;
        // The api_key is intentionally not pushed into the
        // `TaskHandle` — the registry already mapped the provider
        // name to a backend that holds its own key. We materialise
        // the string here only to keep the variable name for
        // grep-ability / future debug logs.
        let _api_key_str = _key.reveal().expose_secret().to_string();
        let _ = json!({ "api_key": _api_key_str });

        // 3. Rebuild a minimal TaskHandle. The backend only needs
        //    `provider_task_id` to construct the GET URL; `model_id`
        //    and `provider` are echoed back unchanged in the
        //    success output.
        let handle = TaskHandle {
            provider: args.provider.clone(),
            provider_task_id: args.provider_task_id.clone(),
            model_id: backend.id().to_string(),
        };

        // 4. Poll.
        let status = backend.poll(&handle).await.map_err(|e| ToolError::Execution {
            name: "poll_video".into(),
            source: Box::new(e),
        })?;

        // 5. Map to the flat JSON enum.
        let out: PollVideoStatusJson = match status {
            VideoStatus::Running => PollVideoStatusJson::Running,
            VideoStatus::Ready { url } => PollVideoStatusJson::Ready { url },
            VideoStatus::Failed { reason } => PollVideoStatusJson::Failed { reason },
        };
        serde_json::to_value(&out).map_err(|e| ToolError::Execution {
            name: "poll_video".into(),
            source: Box::new(e),
        })
    }
}