//! Sagaline model provider layer.
//!
//! Provider adapters implement capability-specific traits. The
//! agent depends only on the traits; concrete providers (rig's
//! OpenAI-compatible client for chat; sagaline-owned native
//! backends for image/audio/video) plug in via [`ProviderRegistry`].
//!
//! ## Trait surface
//!
//! - [`ModelAdapter`] — identity (`id`, `provider_name`) and the
//!   capabilities this backend supports.
//! - [`ImageGen`] — synchronous text/image → image bytes. Native
//!   MiniMax image endpoint; rig for OpenAI gpt-image-1.
//! - [`Tts`] — text → audio bytes. Optional in the first phase.
//! - [`ImageToVideo`] — async task submit + poll. Reserved for a
//!   later phase (first phase has no animated video path).
//!
//! ## Chat
//!
//! Chat completions are **not** a sagaline-owned trait anymore.
//! The chat factory lives in [`crate::openai_compat`] and returns
//! a concrete rig `ChatModel` (= `GenericCompletionModel<OpenAICompletionsExt>`).
//! Every OpenAI-compatible provider — OpenAI, DeepSeek, Ollama,
//! MiniMax (chat), Tongyi, etc. — is reachable through that
//! factory, differing only in base_url + model id.
//!
//! ## Configuration vs keys
//!
//! - Public per-provider config (`base_url`, default model) lives
//!   in `~/.sageline/data/config.toml`. Plaintext; no secrets.
//! - Private keys live in `~/.sageline/data/keys.db`, age-encrypted.
//!   See `sagaline_store::repo::key::KeyRepo`.
//!
//! ## Provider selection
//!
//! Frontmatter `provider: { chat: ..., image: ..., video: ..., tts: ... }`
//! resolves against config.toml + key store. The agent reads the
//! resolved config, not the raw frontmatter.

pub mod adapter;
pub mod config;
pub mod error;
pub mod image_gen;
pub mod image_to_video;
pub mod minimax;
pub mod openai;
pub mod openai_compat;
pub mod registry;
pub mod tts;

pub use adapter::{Capability, GenerationOutput, GenerationRequest, ModelAdapter};
pub use config::{ProviderConfig, ProviderConfigError, ProviderConfigSet};
pub use error::ProviderError;
pub use image_gen::ImageGen;
pub use image_to_video::{ImageToVideo, TaskHandle, VideoRequest, VideoStatus};
pub use minimax::{MinimaxImage, MinimaxTts, MinimaxVideo};
pub use openai_compat::{build_chat, build_chat_with_default, BuildError, ChatModel};
pub use registry::{ProviderRegistry, ResolvedProvider};
pub use tts::Tts;
