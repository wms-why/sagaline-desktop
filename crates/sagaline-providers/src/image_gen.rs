//! Image generation capability.

use async_trait::async_trait;

use crate::adapter::{GenerationOutput, GenerationRequest, ModelAdapter};
use crate::error::ProviderError;

/// Synchronous image generation: text + (optional) reference images →
/// image bytes.
#[async_trait]
pub trait ImageGen: ModelAdapter {
    /// Generate one image. Most providers return one; the trait returns
    /// a single [`GenerationOutput`] for simplicity — batching is the
    /// caller's job at a higher layer.
    async fn generate(
        &self,
        req: GenerationRequest<'_>,
    ) -> Result<GenerationOutput, ProviderError>;
}