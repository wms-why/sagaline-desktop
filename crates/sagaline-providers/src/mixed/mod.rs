//! Mixed provider: routes each capability to a different concrete
//! provider (or the same one). Constructed via
//! `ProviderRegistry::register_mixed` so slot resolution can be
//! validated against the registry.
#![allow(refining_impl_trait)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use sagaline_core::provider::{
    ImageRequest, ImageResponse, ModelId, ModelProvider, ProviderCapabilities,
    ProviderError, ProviderId, ProviderKind, TextRequest, TextResponse, TextToImage,
    TextToText, TextImageToVideo, VideoJob, VideoRequest,
};

use crate::provider::Provider;
use crate::registry::ProviderRegistry;

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Slot: a (provider_id, model) pair used to dispatch one capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedSlot {
    pub provider_id: ProviderId,
    pub model: String,
}

impl MixedSlot {
    pub fn new(provider_id: impl Into<ProviderId>, model: impl Into<String>) -> Self {
        Self { provider_id: provider_id.into(), model: model.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedConfig {
    pub text: MixedSlot,
    pub image: MixedSlot,
    pub video: MixedSlot,
}

impl MixedConfig {
    /// All three slots share the same provider + model. Convenience
    /// for the common "one provider handles everything" case.
    pub fn uniform(provider_id: impl Into<ProviderId>, model: impl Into<String>) -> Self {
        let id: ProviderId = provider_id.into();
        let m: String = model.into();
        Self {
            text:  MixedSlot { provider_id: id.clone(), model: m.clone() },
            image: MixedSlot { provider_id: id.clone(), model: m.clone() },
            video: MixedSlot { provider_id: id, model: m },
        }
    }
}

pub struct MixedProvider {
    id: ProviderId,
    text: Arc<Provider>,
    image: Arc<Provider>,
    video: Arc<Provider>,
    text_model: String,
    image_model: String,
    video_model: String,
}

impl MixedProvider {
    pub(crate) fn new(
        id: ProviderId,
        cfg: MixedConfig,
        registry: &ProviderRegistry,
    ) -> Result<Self, ProviderError> {
        let text = registry.resolve_slot(&cfg.text, ProviderCapabilities {
            text_to_text: true, ..Default::default()
        })?;
        let image = registry.resolve_slot(&cfg.image, ProviderCapabilities {
            text_to_image: true, ..Default::default()
        })?;
        let video = registry.resolve_slot(&cfg.video, ProviderCapabilities {
            text_image_to_video: true, ..Default::default()
        })?;
        Ok(Self {
            id,
            text,
            image,
            video,
            text_model: cfg.text.model,
            image_model: cfg.image.model,
            video_model: cfg.video.model,
        })
    }
}

impl ModelProvider for MixedProvider {
    fn id(&self) -> &ProviderId { &self.id }
    fn kind(&self) -> &ProviderKind {
        static K: std::sync::LazyLock<ProviderKind> =
            std::sync::LazyLock::new(|| ProviderKind::Mixed);
        &K
    }
    fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::all() }
    fn label(&self) -> &str { "Mixed" }
}

impl TextToText for MixedProvider {
    fn generate_text(
        &self,
        mut req: TextRequest,
    ) -> BoxFut<'_, Result<TextResponse, ProviderError>> {
        if req.model.is_none() {
            req.model = Some(ModelId::new(&self.text_model));
        }
        let p = Arc::clone(&self.text);
        Box::pin(async move { p.generate_text(req).await })
    }
}

impl TextToImage for MixedProvider {
    fn generate_image(
        &self,
        mut req: ImageRequest,
    ) -> BoxFut<'_, Result<ImageResponse, ProviderError>> {
        if req.model.is_none() {
            req.model = Some(ModelId::new(&self.image_model));
        }
        let p = Arc::clone(&self.image);
        Box::pin(async move { p.generate_image(req).await })
    }
}

impl TextImageToVideo for MixedProvider {
    fn submit_video(
        &self,
        mut req: VideoRequest,
    ) -> BoxFut<'_, Result<VideoJob, ProviderError>> {
        if req.model.is_none() {
            req.model = Some(ModelId::new(&self.video_model));
        }
        let p = Arc::clone(&self.video);
        Box::pin(async move { p.submit_video(req).await })
    }
}