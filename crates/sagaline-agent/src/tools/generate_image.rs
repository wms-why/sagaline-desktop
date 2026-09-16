//! `generate_image` agent tool.
//!
//! Bridges the agent's tool-call layer to the providers' [`ImageGen`]
//! trait. Holds an `Arc<ProviderRegistry>` + an `Arc<ProviderConfigSet>`
//! + an `Arc<SagalineStore>` so the same triplet can be shared with
//! other tools and the app shell.
//!
//! ## Wire shape
//!
//! Args (JSON):
//!
//! ```json
//! {
//!   "provider":     "minimax",
//!   "model":        "image-01",     // optional, overrides config.toml default
//!   "prompt":       "...",
//!   "negative_prompt": "...",        // optional
//!   "aspect_ratio":   "16:9",         // optional
//!   "output_path":    "/abs/path.png",
//!   "key_id":         "default"       // optional
//! }
//! ```
//!
//! Output:
//!
//! ```json
//! {
//!   "path":            "/abs/path.png",
//!   "bytes":           12345,
//!   "mime":            "image/png",
//!   "provider_job_id": "..."
//! }
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use secrecy::ExposeSecret as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use sagaline_keys::{ProviderKeyId, SagalineStore};
use sagaline_providers::{
    Capability, GenerationRequest, ProviderConfigSet, ProviderError, ProviderRegistry,
};

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

/// Tool arguments.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateImageArgs {
    /// Logical provider name (e.g. `"minimax"`, `"openai"`). Must be
    /// registered in the [`ProviderRegistry`].
    pub provider: String,

    /// Override the per-provider default model id (resolved against
    /// `config.toml` otherwise).
    #[serde(default)]
    pub model: Option<String>,

    /// Natural-language prompt.
    pub prompt: String,

    /// Optional negative prompt (some providers honour it).
    #[serde(default)]
    pub negative_prompt: Option<String>,

    /// Optional aspect ratio (e.g. `"16:9"`). Ignored by providers
    /// that don't understand it.
    #[serde(default)]
    pub aspect_ratio: Option<String>,

    /// Where to write the generated file. Created (with parents) if
    /// missing.
    pub output_path: PathBuf,

    /// Optional key id to use when looking up the API key in the
    /// store. Defaults to `"default"` if absent.
    #[serde(default)]
    pub key_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerateImageOutput {
    path: String,
    bytes: u64,
    mime: &'static str,
    provider_job_id: Option<String>,
}

/// The tool. Cheap to clone.
#[derive(Clone)]
pub struct GenerateImageTool {
    registry: Arc<ProviderRegistry>,
    config: Arc<ProviderConfigSet>,
    store: Arc<SagalineStore>,
}

impl GenerateImageTool {
    pub fn new(
        registry: Arc<ProviderRegistry>,
        config: Arc<ProviderConfigSet>,
        store: Arc<SagalineStore>,
    ) -> Self {
        Self {
            registry,
            config,
            store,
        }
    }
}

#[async_trait]
impl Tool for GenerateImageTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<GenerateImageArgs>(
            "generate_image",
            "Generate a single image from a prompt using the named provider's image backend. \
             Writes the bytes to `output_path` and returns `{path, bytes, mime, provider_job_id}`. \
             Use `provider` to pick the backend (e.g. \"minimax\", \"openai\"); `model` overrides \
             the per-provider default; `aspect_ratio` is honoured by providers that support it.",
        )
    }

    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: GenerateImageArgs = serde_json::from_value(args).map_err(|e| {
            ToolError::BadArgs {
                name: "generate_image".into(),
                message: e.to_string(),
            }
        })?;

        self.run(parsed).await
    }
}

impl GenerateImageTool {
    async fn run(&self, args: GenerateImageArgs) -> Result<ToolResult, ToolError> {
        // 1. Pick the backend as `Arc<dyn ImageGen>`.
        let backend = self
            .registry
            .pick_image(&args.provider)
            .map_err(|e: ProviderError| ToolError::Execution {
                name: "generate_image".into(),
                source: Box::new(e),
            })?;

        // 2. Resolve model id.
        let model_id = args
            .model
            .clone()
            .or_else(|| {
                self.config
                    .get(Capability::Image, &args.provider)
                    .map(|c| c.model.clone())
            })
            .unwrap_or_else(|| backend.id().to_string());

        // 3. Look up the API key.
        let key_id = ProviderKeyId::new(
            args.provider.clone(),
            args.key_id.clone().unwrap_or_else(|| "default".to_string()),
        )
        .map_err(|e| ToolError::Execution {
            name: "generate_image".into(),
            source: Box::new(e),
        })?;
        let key = self.store.keys().get(&key_id).map_err(|e| ToolError::Execution {
            name: "generate_image".into(),
            source: Box::new(e),
        })?;

        // 4. Build the request. The api_key flows through `extra` —
        //    the bridge passes it through to the backend's HTTP
        //    request at runtime.
        let api_key_str = key.reveal().expose_secret().to_string();
        let extra = json!({ "api_key": api_key_str, "model_id": model_id });
        let prompt = args.prompt.clone();
        let negative = args.negative_prompt.clone();
        let aspect = args.aspect_ratio.clone();
        let req = GenerationRequest {
            prompt: &prompt,
            negative_prompt: negative.as_deref(),
            reference_images: &[],
            seed: None,
            aspect_ratio: aspect.as_deref(),
            extra: &extra,
        };

        // 5. Invoke via the ImageGen trait.
        let out = backend.generate(req).await.map_err(|e| ToolError::Execution {
            name: "generate_image".into(),
            source: Box::new(e),
        })?;

        // 6. Write the bytes.
        if let Some(parent) = args.output_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| ToolError::Io {
                    name: "generate_image".into(),
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }
        }
        tokio::fs::write(&args.output_path, &out.bytes)
            .await
            .map_err(|e| ToolError::Io {
                name: "generate_image".into(),
                path: args.output_path.clone(),
                source: e,
            })?;

        let result = GenerateImageOutput {
            path: args.output_path.to_string_lossy().into_owned(),
            bytes: out.bytes.len() as u64,
            mime: out.mime,
            provider_job_id: out.provider_job_id,
        };
        serde_json::to_value(&result).map_err(|e| ToolError::Execution {
            name: "generate_image".into(),
            source: Box::new(e),
        })
    }
}
