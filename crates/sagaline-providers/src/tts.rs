//! Text-to-speech capability.

use async_trait::async_trait;

use crate::adapter::{GenerationOutput, ModelAdapter};
use crate::error::ProviderError;

/// One-shot text-to-speech.
#[async_trait]
pub trait Tts: ModelAdapter {
    /// `text` is the text to speak; `voice_id` is provider-specific
    /// (e.g. `"English_expressive_narrator"`).
    async fn synthesize(
        &self,
        text: &str,
        voice_id: &str,
    ) -> Result<GenerationOutput, ProviderError>;
}