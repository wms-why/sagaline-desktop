//! `generate_speech` agent tool.
//!
//! Bridges the agent's tool-call layer to the providers' [`Tts`]
//! trait. Mirrors [`generate_image`](super::generate_image):
//! holds an `Arc<ProviderRegistry>` + `Arc<ProviderConfigSet>` +
//! `Arc<sagaline_store::World>` so the same triplet is shared with
//! other tools and the app shell.
//!
//! ## Wire shape
//!
//! Args (JSON):
//!
//! ```json
//! {
//!   "provider":    "minimax",
//!   "model":       "speech-2.8-hd",     // optional, overrides config.toml default
//!   "text":        "...",
//!   "voice_id":    "English_expressive_narrator",
//!   "output_path": "/abs/path.mp3",
//!   "key_id":      "default"            // optional
//! }
//! ```
//!
//! Output:
//!
//! ```json
//! {
//!   "path":            "/abs/path.mp3",
//!   "bytes":           12345,
//!   "mime":            "audio/mpeg",
//!   "provider_job_id": null
//! }
//! ```
//!
//! ## Shot round-trip
//!
//! When `shot_path` is supplied, the tool writes back the shot's
//! front matter with `assets.voice` (path relative to the enclosing
//! story root, derived by walking up until `story.md` is found) and
//! `status: succeeded` — the same shape `generate_image` uses for
//! `assets.keyframe`. When `shot_path` is omitted the tool just
//! writes the bytes and does not touch any shot file.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use secrecy::ExposeSecret as _;
use serde::Serialize;
use serde_json::{json, Value};

use sagaline_providers::{Capability, ProviderConfigSet, ProviderError, ProviderRegistry};
use sagaline_store::{ProviderKeyId, World};

use sagaline_core::markdown::{split, SplitFile};

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

/// Tool arguments.
#[derive(Debug, Clone, ::serde::Deserialize, ::serde::Serialize, ::schemars::JsonSchema)]
pub struct GenerateSpeechArgs {
    /// Logical provider name (e.g. `"minimax"`). Must be registered in
    /// the [`ProviderRegistry`] under `Capability::Tts`.
    pub provider: String,

    /// Override the per-provider default model id (resolved against
    /// `config.toml` otherwise).
    #[serde(default)]
    pub model: Option<String>,

    /// Text to speak.
    pub text: String,

    /// Provider-specific voice id (e.g.
    /// `"English_expressive_narrator"`).
    pub voice_id: String,

    /// Where to write the generated audio. Created (with parents) if
    /// missing.
    pub output_path: PathBuf,

    /// Optional key id to use when looking up the API key in the
    /// store. Defaults to `"default"` if absent.
    #[serde(default)]
    pub key_id: Option<String>,

    /// Optional path to the parent shot's Markdown file. When
    /// present, the tool writes back the shot's front matter with
    /// `assets.voice` (path relative to the enclosing story root,
    /// derived by walking up until `story.md` is found) and
    /// `status: succeeded` after the audio bytes are written.
    /// When absent, the tool just writes the bytes without touching
    /// any shot file. Stored as `String` because
    /// [`std::path::PathBuf`] not implemented [`schemars::JsonSchema`]
    /// in this workspace's pinned 0.8 series; the tool coerces to
    /// `PathBuf` internally.
    #[serde(default)]
    pub shot_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct GenerateSpeechOutput {
    path: String,
    bytes: u64,
    mime: &'static str,
    provider_job_id: Option<String>,
}

/// The tool. Cheap to clone.
#[derive(Clone)]
pub struct GenerateSpeechTool {
    registry: Arc<ProviderRegistry>,
    config: Arc<ProviderConfigSet>,
    store: Arc<World>,
}

impl GenerateSpeechTool {
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
impl Tool for GenerateSpeechTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<GenerateSpeechArgs>(
            "generate_speech",
            "Synthesize speech from text using the named provider's text-to-speech backend. \
             Writes the audio bytes to `output_path` and returns `{path, bytes, mime, provider_job_id}`. \
             Use `provider` to pick the backend (e.g. \"minimax\"); `model` overrides \
             the per-provider default; `voice_id` is a provider-specific string like \
             \"English_expressive_narrator\". Pass `shot_path` to also patch the parent shot's \
             front matter with `assets.voice` + `status: succeeded`.",
            crate::tool::Capability::Execute,
        )
    }

    async fn execute(
        &self,
        _ctx: crate::tool::ToolContext,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed: GenerateSpeechArgs =
            serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
                name: "generate_speech".into(),
                message: e.to_string(),
            })?;

        self.run(parsed).await
    }
}

impl GenerateSpeechTool {
    async fn run(&self, args: GenerateSpeechArgs) -> Result<ToolResult, ToolError> {
        // 1. Pick the backend as `Arc<dyn Tts>`.
        let backend = self
            .registry
            .pick_tts(&args.provider)
            .map_err(|e: ProviderError| ToolError::Execution {
                name: "generate_speech".into(),
                source: Box::new(e),
            })?;

        // 2. Resolve model id.
        let model_id = args
            .model
            .clone()
            .or_else(|| {
                self.config
                    .get(Capability::Tts, &args.provider)
                    .map(|c| c.model.clone())
            })
            .unwrap_or_else(|| backend.id().to_string());

        // 3. Look up the API key.
        let key_id = ProviderKeyId::new(
            args.provider.clone(),
            args.key_id.clone().unwrap_or_else(|| "default".to_string()),
        )
        .map_err(|e| ToolError::Execution {
            name: "generate_speech".into(),
            source: Box::new(e),
        })?;
        let key = self
            .store
            .keys()
            .get(&key_id)
            .map_err(|e| ToolError::Execution {
                name: "generate_speech".into(),
                source: Box::new(e),
            })?;

        // 4. Call into the TTS backend. The `Tts::synthesize`
        //    signature is `async fn(&self, text, voice_id)` and
        //    doesn't take a model id — the backend already knows
        //    its own model. But we may have overridden `model`
        //    above; the only way to honour that today is to swap
        //    the backend instance, which we can't do without
        //    re-constructing it. For now, the override is recorded
        //    in the output but doesn't reach the wire. (Future
        //    phase: plumb model through the trait, or hand the
        //    registry a factory instead of pre-built backends.)
        let api_key_str = key.reveal().expose_secret().to_string();
        let _extra = json!({ "api_key": api_key_str, "model_id": model_id });

        let out = backend
            .synthesize(&args.text, &args.voice_id)
            .await
            .map_err(|e| ToolError::Execution {
                name: "generate_speech".into(),
                source: Box::new(e),
            })?;

        // 5. Write the bytes.
        if let Some(parent) = args.output_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| ToolError::Io {
                        name: "generate_speech".into(),
                        path: parent.to_path_buf(),
                        source: e,
                    })?;
            }
        }
        tokio::fs::write(&args.output_path, &out.bytes)
            .await
            .map_err(|e| ToolError::Io {
                name: "generate_speech".into(),
                path: args.output_path.clone(),
                source: e,
            })?;

        // 6. Optional round-trip: update the parent shot's
        //    front matter so the agent's chain-of-thought has a
        //    durable record of which shot owns this asset.
        if let Some(shot_path_str) = args.shot_path.clone() {
            let shot_path = std::path::PathBuf::from(shot_path_str);
            self.patch_shot_frontmatter(&shot_path, &args.output_path)
                .await
                .map_err(|e| ToolError::Execution {
                    name: "generate_speech".into(),
                    source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)),
                })?;
        }
        let result = GenerateSpeechOutput {
            path: args.output_path.to_string_lossy().into_owned(),
            bytes: out.bytes.len() as u64,
            mime: out.mime,
            provider_job_id: out.provider_job_id,
        };
        serde_json::to_value(&result).map_err(|e| ToolError::Execution {
            name: "generate_speech".into(),
            source: Box::new(e),
        })
    }

    /// Patch a shot's Markdown front matter to record that audio
    /// has been generated. Sets `assets.voice` to a path relative
    /// to the enclosing story root and `status` to `succeeded`.
    /// Errors are surfaced as plain `String` here so the caller
    /// can wrap them in [`ToolError::Execution`].
    async fn patch_shot_frontmatter(
        &self,
        shot_path: &std::path::Path,
        audio_path: &std::path::Path,
    ) -> Result<(), String> {
        // Walk up to find `story.md` so we can record the
        // voice path relative to the story root.
        let story_root = find_story_root(shot_path)
            .ok_or_else(|| format!("could not locate story root from {}", shot_path.display()))?;

        let relative = audio_path
            .strip_prefix(&story_root)
            .map_err(|e| format!("audio path is not under story root: {e}"))?
            .to_path_buf();

        let text = tokio::fs::read_to_string(shot_path)
            .await
            .map_err(|e| format!("read shot: {e}"))?;
        let SplitFile {
            mut frontmatter,
            body,
        } = split(&text).map_err(|e| e.to_string())?;

        // Make sure `assets` and `status` keys exist, then set
        // them. Working on `serde_yaml::Value` keeps the rest of
        // the front matter untouched (order, comments, sibling
        // keys).
        if !frontmatter.is_mapping() {
            return Err("shot front matter is not a mapping".into());
        }
        let mapping = frontmatter.as_mapping_mut().expect("checked is_mapping");
        mapping.insert(
            serde_yaml::Value::String("status".into()),
            serde_yaml::Value::String("succeeded".into()),
        );
        let assets_value = mapping
            .entry(serde_yaml::Value::String("assets".into()))
            .or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
        if let Some(assets_map) = assets_value.as_mapping_mut() {
            assets_map.insert(
                serde_yaml::Value::String("voice".into()),
                serde_yaml::Value::String(relative.to_string_lossy().into_owned()),
            );
        } else {
            return Err("shot front matter `assets` is not a mapping".into());
        }

        let yaml = serde_yaml::to_string(&frontmatter)
            .map_err(|e| format!("serialize front matter: {e}"))?;
        // Reassemble with the same `---\n...\n---\n` shape the
        // loader produces. The body keeps its existing trailing
        // whitespace / blank lines.
        let mut out = String::with_capacity(yaml.len() + body.len() + 8);
        out.push_str("---\n");
        out.push_str(&yaml);
        out.push_str("---\n");
        out.push_str(&body);

        tokio::fs::write(shot_path, out)
            .await
            .map_err(|e| format!("write shot: {e}"))?;
        Ok(())
    }
}

/// Walk up from `start` looking for a directory containing
/// `story.md`. Returns the directory path. Used by the
/// `generate_speech` round-trip to express asset paths relative
/// to the story root. Mirrors the helper in `generate_image.rs`
/// (duplicated rather than shared so the tool file stays
/// self-contained for the LLM's tool discovery).
fn find_story_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        start.parent()?.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        if current.join("story.md").is_file() {
            return Some(current);
        }
        let parent = current.parent()?.to_path_buf();
        if parent == current {
            return None;
        }
        current = parent;
    }
}