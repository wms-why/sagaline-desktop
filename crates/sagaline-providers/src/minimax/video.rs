//! Video generation via MiniMax.
//!
//! Two endpoints:
//!
//! | Step       | Endpoint                                            |
//! | ---------- | --------------------------------------------------- |
//! | Submit     | `POST {base}/v2/video_generation`  (returns task_id) |
//! | Poll       | `GET  {base}/v2/query/video_generation/{task_id}`   |
//!
//! On submit success we return a [`VideoJob`] whose state machine is
//! `Running`; the caller polls via `VideoJobHandle::poll`. The handle
//! owns a clone of the shared HTTP client and the task_id so polling
//! is cheap (no cloning the provider).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use sagaline_core::provider::{
    AspectRatio, ProviderError, VideoArtifact, VideoJob, VideoJobHandle, VideoJobId,
    VideoJobState, VideoRequest, VideoResolution, VideoResponse, VideoUsage,
};

use super::MiniMaxProvider;
use crate::http::Client;

/// Default video model. H3 supports text / first-last frame / multimodal refs.
pub const VIDEO_DEFAULT_MODEL: &str = "MiniMax-H3";

// --- Submit request schema -------------------------------------------------

#[derive(Debug, Serialize)]
struct VideoSubmitReq {
    model: String,
    content: Vec<ContentItem>,
    resolution: String,
    duration: u32,
    ratio: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentItem {
    Text { text: String },
    ImageUrl {
        image_url: ImageUrlInner,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<Role>,
    },
}

#[derive(Debug, Serialize)]
struct ImageUrlInner {
    url: String,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Role {
    FirstFrame,
    LastFrame,
    ReferenceImage,
}

#[derive(Debug, Deserialize)]
struct VideoSubmitResp {
    task_id: String,
    #[serde(default)]
    base_resp: Option<BaseResp>,
}

#[derive(Debug, Deserialize)]
struct BaseResp {
    status_code: i32,
    status_msg: Option<String>,
}

// --- Poll response schema --------------------------------------------------

#[derive(Debug, Deserialize)]
struct VideoQueryResp {
    task: VideoTask,
}

#[derive(Debug, Deserialize)]
struct VideoTask {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: Option<String>,
    status: String,
    #[serde(default)]
    error: Option<VideoTaskError>,
    #[serde(default)]
    content: Option<VideoTaskContent>,
    #[serde(default)]
    #[allow(dead_code)]
    resolution: Option<String>,
    #[serde(default)]
    duration: Option<u32>,
    #[serde(default)]
    usage: Option<VideoTaskUsage>,
    #[serde(default)]
    #[allow(dead_code)]
    ratio: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VideoTaskError {
    #[allow(dead_code)]
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VideoTaskContent {
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct VideoTaskUsage {
    #[allow(dead_code)]
    total_seconds: Option<u32>,
    input_seconds: Option<u32>,
    output_seconds: Option<u32>,
    input_image_count: Option<u32>,
    #[allow(dead_code)]
    total_tokens: Option<u32>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

// --- Polling handle --------------------------------------------------------

#[derive(Debug)]
pub(crate) struct MiniMaxVideoHandle {
    http: Client,
    task_id: String,
    polls: std::sync::atomic::AtomicU32,
}

impl MiniMaxVideoHandle {
    pub(crate) fn new(http: Client, task_id: String) -> Self {
        Self {
            http,
            task_id,
            polls: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

impl VideoJobHandle for MiniMaxVideoHandle {
    fn poll(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<VideoJobState, ProviderError>> + Send + '_>> {
        let http = self.http.clone();
        let task_id = self.task_id.clone();
        let polls = &self.polls;
        Box::pin(async move {
            polls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = format!("/v2/query/video_generation/{task_id}");
            let req = http.get(&path)?.header(
                reqwest::header::ACCEPT,
                "application/json",
            );
            let resp = http.send(req).await?;
            let parsed: VideoQueryResp = resp
                .json()
                .await
                .map_err(|e| ProviderError::Decode(format!("video poll: {e}")))?;

            let task = parsed.task;
            match task.status.as_str() {
                "succeeded" => {
                    let url = task
                        .content
                        .and_then(|c| c.url)
                        .ok_or_else(|| ProviderError::Decode(
                            "video succeeded but no content url".into(),
                        ))?;
                    let bytes = http.download_bytes(&url).await?;
                    let (width, height, fps, mime) = sniff_video(&bytes)
                        .unwrap_or((0, 0, 30, "video/mp4"));
                    let usage = task.usage.map(|u| VideoUsage {
                        output_seconds: u.output_seconds,
                        input_image_count: u.input_image_count,
                        input_seconds: u.input_seconds,
                        total_tokens: u.total_tokens,
                        prompt_tokens: u.prompt_tokens,
                        completion_tokens: u.completion_tokens,
                    });
                    let video = VideoArtifact {
                        mime: mime.to_string(),
                        bytes,
                        duration_secs: task.duration.unwrap_or(0) as f32,
                        width,
                        height,
                        fps,
                    };
                    Ok(VideoJobState::Done(VideoResponse {
                        video,
                        seed: None,
                        usage,
                    }))
                }
                "failed" => {
                    let msg = task
                        .error
                        .as_ref()
                        .and_then(|e| e.message.clone())
                        .unwrap_or_else(|| "unknown".into());
                    Ok(VideoJobState::Failed(msg))
                }
                "cancelled" => Ok(VideoJobState::Cancelled),
                // queued / running → keep polling
                _ => Ok(VideoJobState::Running(Box::new(MiniMaxVideoHandle::new(
                    http, task_id,
                )))),
            }
        })
    }

    fn suggested_poll_after(&self) -> Duration {
        let n = self.polls.load(std::sync::atomic::Ordering::Relaxed);
        let secs = 2u64.saturating_mul(1u64 << n.min(3)).min(10);
        Duration::from_secs(secs)
    }
}

// --- Submit entry point ----------------------------------------------------

pub async fn submit_video(
    p: &MiniMaxProvider,
    req: VideoRequest,
) -> Result<VideoJob, ProviderError> {
    if req.prompt.trim().is_empty() {
        return Err(ProviderError::Config("prompt is required".into()));
    }
    let model = req
        .model
        .as_ref()
        .map(|m| m.as_str())
        .unwrap_or(&p.cfg().default_video_model)
        .to_string();

    let resolution = match req.resolution.unwrap_or(VideoResolution::R768p) {
        VideoResolution::R480p => "480P",
        VideoResolution::R768p => "768P",
        VideoResolution::R1080p => "1080P",
        VideoResolution::R2k => "2K",
        VideoResolution::R4k => "4K",
    };
    let duration = req.duration_secs.unwrap_or(5);
    let ratio = match req.aspect_ratio.unwrap_or(AspectRatio::R16x9) {
        AspectRatio::R21x9 => "21:9",
        AspectRatio::R16x9 => "16:9",
        AspectRatio::R4x3 => "4:3",
        AspectRatio::R1x1 => "1:1",
        AspectRatio::R3x4 => "3:4",
        AspectRatio::R9x16 => "9:16",
        AspectRatio::R3x2 | AspectRatio::R2x3 | AspectRatio::Adaptive => "16:9",
    };

    // Build content array. MiniMax requires at least one text item.
    let mut content: Vec<ContentItem> = vec![ContentItem::Text { text: req.prompt.clone() }];
    let refs = &req.reference_images;
    if !refs.is_empty() {
        for (idx, img) in refs.iter().enumerate() {
            let role = match (refs.len(), idx) {
                (1, _) => Role::FirstFrame,
                (2, 0) => Role::FirstFrame,
                (2, 1) => Role::LastFrame,
                (_, 0) => Role::FirstFrame,
                (_, 1) => Role::LastFrame,
                (_, _) => Role::ReferenceImage,
            };
            let data_uri = image_to_data_uri(img)?;
            content.push(ContentItem::ImageUrl {
                image_url: ImageUrlInner { url: data_uri },
                role: Some(role),
            });
        }
    }

    let body = VideoSubmitReq {
        model,
        content,
        resolution: resolution.to_string(),
        duration,
        ratio: ratio.to_string(),
    };

    let http = p.http().clone();
    let http_req = http.post("/v2/video_generation")?.json(&body);
    let resp = http.send(http_req).await?;
    let parsed: VideoSubmitResp = resp
        .json()
        .await
        .map_err(|e| ProviderError::Decode(format!("video submit: {e}")))?;

    if let Some(br) = &parsed.base_resp {
        if br.status_code != 0 {
            return Err(ProviderError::HttpStatus {
                status: 200,
                body: format!(
                    "MiniMax base_resp {}: {}",
                    br.status_code,
                    br.status_msg.clone().unwrap_or_default()
                ),
            });
        }
    }

    let handle = MiniMaxVideoHandle::new(http, parsed.task_id.clone());
    let job = VideoJob::new(
        VideoJobId::new(parsed.task_id),
        VideoJobState::Running(Box::new(handle)),
    );
    Ok(job)
}

// --- helpers ---------------------------------------------------------------

fn image_to_data_uri(img: &sagaline_core::provider::ImageArtifact) -> Result<String, ProviderError> {
    let format = match img.mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/webp" => "webp",
        m if m.starts_with("image/") => &m[6..],
        other => {
            return Err(ProviderError::Config(format!(
                "unsupported image mime {other} for video reference"
            )))
        }
    };
    let b64 = B64.encode(&img.bytes);
    Ok(format!("data:image/{format};base64,{b64}"))
}

/// Best-effort sniff of MP4 — only confirms ftyp + returns mime. Real
/// dimension parsing (tkhd box walk) is non-trivial; left as `0` for v1.
fn sniff_video(b: &[u8]) -> Option<(u32, u32, u32, &'static str)> {
    if b.len() >= 32 && b.windows(4).any(|w| w == b"ftyp") {
        return Some((0, 0, 30, "video/mp4"));
    }
    None
}