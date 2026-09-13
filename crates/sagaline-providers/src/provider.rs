//! Closed `enum Provider` that statically dispatches to the right
//! concrete provider. Consumers hold an `Arc<Provider>` or just `&Provider`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sagaline_core::provider::{
    ImageRequest, ImageResponse, ModelProvider, ProviderError, ProviderId, ProviderKind,
    TextRequest, TextResponse, TextToImage, TextToText, TextImageToVideo, VideoJob,
    VideoRequest,
};

use crate::minimax::MiniMaxProvider;
use crate::mixed::MixedProvider;

/// Type alias for a Send + 'static boxed future used by the enum
/// dispatch arms. Each `match` arm returns its own opaque type, so
/// boxing normalizes them to a single signature.
pub type SendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Closed dispatch enum. New providers add a variant here + a match
/// arm in every impl below.
pub enum Provider {
    MiniMax(MiniMaxProvider),
    Mixed(MixedProvider),
}

impl Provider {
    pub fn arc(self) -> Arc<Self> { Arc::new(self) }
}

impl ModelProvider for Provider {
    fn id(&self) -> &ProviderId {
        match self {
            Self::MiniMax(p) => p.id(),
            Self::Mixed(p) => p.id(),
        }
    }
    fn kind(&self) -> &ProviderKind {
        match self {
            Self::MiniMax(p) => p.kind(),
            Self::Mixed(p) => p.kind(),
        }
    }
    fn capabilities(&self) -> sagaline_core::provider::ProviderCapabilities {
        match self {
            Self::MiniMax(p) => p.capabilities(),
            Self::Mixed(p) => p.capabilities(),
        }
    }
}
impl TextToText for Provider {
    #[allow(refining_impl_trait)]
    fn generate_text(&self, req: TextRequest) -> SendFuture<'_, Result<TextResponse, ProviderError>> {
        match self {
            Self::MiniMax(p) => Box::pin(p.generate_text(req)),
            Self::Mixed(p) => Box::pin(p.generate_text(req)),
        }
    }
}

impl TextToImage for Provider {
    #[allow(refining_impl_trait)]
    fn generate_image(&self, req: ImageRequest) -> SendFuture<'_, Result<ImageResponse, ProviderError>> {
        match self {
            Self::MiniMax(p) => Box::pin(p.generate_image(req)),
            Self::Mixed(p) => Box::pin(p.generate_image(req)),
        }
    }
}

impl TextImageToVideo for Provider {
    #[allow(refining_impl_trait)]
    fn submit_video(&self, req: VideoRequest) -> SendFuture<'_, Result<VideoJob, ProviderError>> {
        match self {
            Self::MiniMax(p) => Box::pin(p.submit_video(req)),
            Self::Mixed(p) => Box::pin(p.submit_video(req)),
        }
    }
}