//! OpenAI-compatible chat facade.
//!
//! Every chat backend sagaline targets today exposes an
//! OpenAI-compatible `/v1/chat/completions` endpoint — OpenAI,
//! DeepSeek, Ollama, MiniMax (chat), Tongyi, and any other
//! provider speaking the same wire. We used to hand-roll the
//! request body, the auth header, and the response parse here.
//! Since adopting rig-core we no longer do: this module is a thin
//! factory that constructs a rig chat model from a base URL,
//! model id, and an API key sourced from the local key store.
//!
//! Wire-level details — POST shape, retry, error mapping, JSON
//! parsing — live in `rig_core::providers::openai`. The agent
//! receives a `rig_core::completion::CompletionResponse` and
//! walks its `choice` vector for assistant text / tool calls.

pub mod chat;

pub use chat::{build_chat, build_chat_with_default, ChatModel};

/// Errors raised by chat-model construction. Distinct from
/// [`crate::ProviderError`] because the only failure modes here
/// are local (config + rig builder), not network.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The caller passed an empty base URL. Almost always a
    /// `config.toml` typo (`[chat.openai]\nbase_url = ""`).
    #[error("provider base_url is empty (check ~/.sageline/data/config.toml)")]
    EmptyBaseUrl,

    /// Rig's `ClientBuilder::build` failed. Almost always a
    /// missing or malformed API key.
    #[error("rig client build failed: {0}")]
    RigClient(String),
}
