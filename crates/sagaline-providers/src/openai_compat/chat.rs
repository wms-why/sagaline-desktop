//! OpenAI-compatible chat client (thin wrapper over rig-core).
//!
//! Replaces the hand-rolled `OpenAiCompatChat` that lived here
//! before rig-core was adopted. The only sagaline-owned logic left
//! is keying: we never read the API key from the environment —
//! every call takes a `&sagaline_keys::KeyHandle` and feeds the
//! secret straight into rig's bearer-auth slot.
//!
//! Wire details (POST shape, error mapping, retry, streaming) are
//! all owned by `rig_core::providers::openai`; sagaline just picks
//! the model id + base_url for the provider being addressed.
//!
//! ## Provider coverage
//!
//! Every backend that exposes an OpenAI-compatible
//! `/v1/chat/completions` endpoint is reachable through this
//! module — OpenAI, DeepSeek, Ollama, MiniMax (chat), and any
//! other OpenAI-compat server. They differ only in base_url + the
//! model id passed at call time.
//!
//! ## Result type
//!
//! All chat completions in this crate surface as
//! `rig_core::completion::CompletionResponse`, rig's
//! provider-agnostic envelope. Callers in `sagaline-agent` walk
//! the `choice` vector to extract assistant text / tool calls.

use rig_core::client::CompletionClient;
use rig_core::providers::openai;
use rig_core::providers::openai::completion::GenericCompletionModel;
use secrecy::ExposeSecret as _;
use std::sync::Arc;

use sagaline_keys::KeyHandle;

/// The single concrete chat-backend type used by sagaline. All
/// OpenAI-compatible providers share it; the rig `Completions`
/// extension is `pub`, so the alias sits at the boundary and the
/// agent doesn't have to import the generics.
pub type ChatModel = GenericCompletionModel<openai::OpenAICompletionsExt>;

/// Build a rig chat model for an OpenAI-compatible provider.
///
/// `base_url` is the provider's OpenAI-compatible endpoint
/// (e.g. `"https://api.openai.com/v1"` for OpenAI,
/// `"https://api.deepseek.com/v1"` for DeepSeek,
/// `"http://localhost:11434/v1"` for Ollama).
///
/// `model_id` is the provider-specific model string
/// (e.g. `"gpt-4o-mini"`, `"deepseek-chat"`, `"llama3.1:8b"`).
///
/// The API key is sourced from the supplied `KeyHandle`. It never
/// touches the environment — `expose_secret()` is consumed once
/// and handed straight into rig's `BearerAuth`, which owns it
/// internally.
///
/// # Errors
///
/// Returns `BuildError::EmptyBaseUrl` if `base_url` is empty.
/// Returns `BuildError::RigClient` if rig's client builder rejects
/// the inputs (almost always a malformed API key).
pub fn build_chat(
    base_url: &str,
    model_id: &str,
    key: &KeyHandle,
) -> Result<Arc<ChatModel>, super::BuildError> {
    if base_url.is_empty() {
        return Err(super::BuildError::EmptyBaseUrl);
    }

    // expose_secret() — the one place secrets leave the wrapper.
    // The temporary String feeds straight into rig's BearerAuth,
    // which owns its key internally. The String drops at the end
    // of this scope; rig has already moved it into its auth slot.
    let secret_string = key.reveal().expose_secret().to_string();
    let bearer: rig_core::client::BearerAuth = secret_string.into();

    let client = openai::Client::builder()
        .api_key(bearer)
        .base_url(base_url)
        .build()
        .map_err(|e| super::BuildError::RigClient(e.to_string()))?
        .completions_api();

    Ok(Arc::new(client.completion_model(model_id)))
}

/// Build a chat model pre-configured for use by the agent. Same as
/// [`build_chat`] but takes the model id from a `Option<&str>`
/// override; if absent, falls back to `default_model_id`.
pub fn build_chat_with_default(
    base_url: &str,
    model_override: Option<&str>,
    default_model_id: &str,
    key: &KeyHandle,
) -> Result<Arc<ChatModel>, super::BuildError> {
    let model_id = model_override.unwrap_or(default_model_id);
    build_chat(base_url, model_id, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sagaline_keys::KeyHandle;

    #[test]
    fn empty_base_url_is_rejected() {
        let h = KeyHandle::from_static_for_test("sk-test");
        let r = build_chat("", "gpt-4o-mini", &h);
        assert!(matches!(r, Err(super::super::BuildError::EmptyBaseUrl)));
    }

    #[test]
    fn non_empty_base_url_builds() {
        let h = KeyHandle::from_static_for_test("sk-test");
        let r = build_chat("https://api.openai.com/v1", "gpt-4o-mini", &h);
        assert!(r.is_ok(), "build should succeed");
    }

    #[test]
    fn smoke_signature_is_stable() {
        // Compile-only check; locks the public factory shape.
        let _: fn(&str, &str, &KeyHandle) -> Result<Arc<ChatModel>, super::super::BuildError> =
            build_chat;
    }
}
