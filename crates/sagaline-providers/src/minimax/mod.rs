//! MiniMax direct provider.
#![allow(refining_impl_trait)]
//!
//! Implements all three sub-traits against the public MiniMax API:
//!
//! | Sub-trait          | Endpoint                                                  |
//! | ------------------ | --------------------------------------------------------- |
//! | `TextToText`       | `POST {base}/anthropic/v1/messages`                       |
//! | `TextToImage`      | `POST {base}/v1/image_generation`                         |
use std::future::Future;
use std::pin::Pin;


use sagaline_core::provider::{
    ImageRequest, ImageResponse, ModelProvider, ProviderCapabilities, ProviderError,
    ProviderId, ProviderKind, TextRequest, TextResponse, TextToImage, TextToText,
    TextImageToVideo, VideoJob, VideoRequest,
};

use crate::http::Client;
use crate::key::ApiKey;

mod text;
mod image;
mod video;

pub use text::TEXT_DEFAULT_MODEL;
pub use image::IMAGE_DEFAULT_MODEL;
pub use video::VIDEO_DEFAULT_MODEL;

#[derive(Debug, Clone)]
pub struct MiniMaxConfig {
    /// API base URL. Defaults to MiniMax official endpoint.
    pub base_url: String,
    /// Default model when the request leaves `model` unset.
    pub default_text_model: String,
    pub default_image_model: String,
    pub default_video_model: String,
    /// HTTP timeout for one request. The video poll is a separate
    /// endpoint and uses the same client timeout.
    pub timeout: std::time::Duration,
}

impl Default for MiniMaxConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.minimax.cn".to_string(),
            default_text_model: TEXT_DEFAULT_MODEL.to_string(),
            default_image_model: IMAGE_DEFAULT_MODEL.to_string(),
            default_video_model: VIDEO_DEFAULT_MODEL.to_string(),
            timeout: std::time::Duration::from_secs(120),
        }
    }
}

pub struct MiniMaxProvider {
    id: ProviderId,
    cfg: MiniMaxConfig,
    http: Client,
}

impl MiniMaxProvider {
    pub fn new(
        id: ProviderId,
        cfg: MiniMaxConfig,
        key: ApiKey,
    ) -> Result<Self, ProviderError> {
        if key.as_str().is_empty() {
            return Err(ProviderError::Auth);
        }
        let http = Client::new(&cfg.base_url, key, cfg.timeout)?;
        Ok(Self { id, cfg, http })
    }

    pub fn http(&self) -> &Client { &self.http }
    pub fn cfg(&self) -> &MiniMaxConfig { &self.cfg }
}

impl ModelProvider for MiniMaxProvider {
    fn id(&self) -> &ProviderId { &self.id }
    fn kind(&self) -> &ProviderKind {
        static K: std::sync::LazyLock<ProviderKind> =
            std::sync::LazyLock::new(|| ProviderKind::MiniMax);
        &K
    }
    fn capabilities(&self) -> ProviderCapabilities { ProviderCapabilities::all() }
    fn label(&self) -> &str { "MiniMax" }
}

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

impl TextToText for MiniMaxProvider {
    fn generate_text(&self, req: TextRequest) -> BoxFut<'_, Result<TextResponse, ProviderError>> {
        Box::pin(text::generate_text(self, req))
    }
}

impl TextToImage for MiniMaxProvider {
    fn generate_image(&self, req: ImageRequest) -> BoxFut<'_, Result<ImageResponse, ProviderError>> {
        Box::pin(image::generate_image(self, req))
    }
}

impl TextImageToVideo for MiniMaxProvider {
    fn submit_video(&self, req: VideoRequest) -> BoxFut<'_, Result<VideoJob, ProviderError>> {
        Box::pin(video::submit_video(self, req))
    }
}