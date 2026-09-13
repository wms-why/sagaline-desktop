//! Image generation via MiniMax.
//!
//! Endpoint: `POST {base}/v1/image_generation`
//!
//! Important: when `response_format` is the default `url`, the API
//! returns 24h-expiring CDN URLs. We always request `base64` so the
//! returned bytes are ours to keep. We also auto-detect image
//! dimensions from the PNG/JPEG/WEBP header.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use sagaline_core::provider::{
    AspectRatio, ImageArtifact, ImageRequest, ImageResponse, ProviderError,
};

use super::MiniMaxProvider;

/// Default image model.
pub const IMAGE_DEFAULT_MODEL: &str = "image-01";

#[derive(Debug, Serialize)]
struct ImageReq<'a> {
    model: &'a str,
    prompt: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    aspect_ratio: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    /// Always request base64 so we don't depend on the 24h-expiring URL.
    response_format: &'static str,
    n: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_optimizer: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ImageResp {
    #[serde(default)]
    data: DataBlock,
    base_resp: Option<BaseResp>,
    #[serde(default)]
    #[allow(dead_code)]
    id: Option<String>,
}
#[derive(Debug, Deserialize, Default)]
struct DataBlock {
    #[serde(default)]
    image_urls: Vec<String>,
    #[serde(default)]
    image_base64: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BaseResp {
    status_code: i32,
    status_msg: Option<String>,
}

pub async fn generate_image(
    p: &MiniMaxProvider,
    req: ImageRequest,
) -> Result<ImageResponse, ProviderError> {
    if req.prompt.trim().is_empty() {
        return Err(ProviderError::Config("prompt is required".into()));
    }
    let model = req
        .model
        .as_ref()
        .map(|m| m.as_str())
        .unwrap_or(&p.cfg().default_image_model);

    // Translate aspect_ratio enum → MiniMax string.
    let aspect = req.aspect_ratio.map(|a| match a {
        AspectRatio::R1x1 => "1:1",
        AspectRatio::R3x2 => "3:2",
        AspectRatio::R2x3 => "2:3",
        AspectRatio::R16x9 => "16:9",
        AspectRatio::R9x16 => "9:16",
        AspectRatio::R4x3 => "4:3",
        AspectRatio::R3x4 => "3:4",
        AspectRatio::R21x9 => "21:9",
        AspectRatio::Adaptive => "1:1", // provider requires concrete; default
    });

    let n = if req.n == 0 { 1 } else { req.n };
    let body = ImageReq {
        model,
        prompt: &req.prompt,
        aspect_ratio: aspect,
        width: req.width,
        height: req.height,
        seed: req.seed,
        response_format: "base64",
        n,
        prompt_optimizer: None,
    };

    let http_req = p.http().post("/v1/image_generation")?.json(&body);
    let resp = p.http().send(http_req).await?;
    let parsed: ImageResp = resp
        .json()
        .await
        .map_err(|e| ProviderError::Decode(format!("image: {e}")))?;

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

    // Collect bytes: prefer inline base64, fall back to downloading each URL.
    let mut images = Vec::with_capacity(parsed.data.image_base64.len() + parsed.data.image_urls.len());
    for b64str in parsed.data.image_base64 {
        let bytes = B64
            .decode(b64str.trim())
            .map_err(|e| ProviderError::Decode(format!("image base64: {e}")))?;
        let (w, h, mime) = sniff_image(&bytes).unwrap_or((0, 0, "image/png"));
        images.push(ImageArtifact { mime: mime.to_string(), bytes, width: w, height: h });
    }
    for url in parsed.data.image_urls {
        let bytes = p.http().download_bytes(&url).await?;
        let (w, h, mime) = sniff_image(&bytes).unwrap_or((0, 0, "image/png"));
        images.push(ImageArtifact { mime: mime.to_string(), bytes, width: w, height: h });
    }

    if images.is_empty() {
        return Err(ProviderError::Decode("image: empty data".into()));
    }

    Ok(ImageResponse {
        images,
        seed: req.seed,
        usage: None,
    })
}

/// Sniff common image headers (PNG, JPEG, WEBP) to extract size + mime.
/// Returns `None` for anything we don't recognize.
fn sniff_image(b: &[u8]) -> Option<(u32, u32, &'static str)> {
    // PNG: 8-byte signature then IHDR with width/height at bytes 16..24.
    if b.len() >= 24 && &b[..8] == b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return Some((w, h, "image/png"));
    }
    // JPEG: scan for SOF0/SOF2 marker.
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] == 0xFF {
                let marker = b[i + 1];
                if marker == 0xC0 || marker == 0xC2 {
                    return Some((
                        u32::from(u16::from_be_bytes([b[i + 7], b[i + 8]])),
                        u32::from(u16::from_be_bytes([b[i + 5], b[i + 6]])),
                        "image/jpeg",
                    ));
                }
                // Skip this segment.
                let seg_len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
                i += 2 + seg_len;
            } else {
                i += 1;
            }
        }
        // Couldn't find SOF; return dimensions-unknown.
        return Some((0, 0, "image/jpeg"));
    }
    // WEBP: RIFF/WEBP then VP8/VP8L/VP8X chunk.
    if b.len() >= 30 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        let fourcc = &b[12..16];
        if fourcc == b"VP8 " {
            let w = u16::from_le_bytes([b[26], b[27]]) & 0x3FFF;
            let h = u16::from_le_bytes([b[28], b[29]]) & 0x3FFF;
            return Some((u32::from(w), u32::from(h), "image/webp"));
        } else if fourcc == b"VP8L" {
            let w = ((u32::from(b[24]) | (u32::from(b[25]) << 8) | (u32::from(b[26]) << 16)) & 0x3FFF) + 1;
            let h = (((u32::from(b[26]) >> 6) | (u32::from(b[27]) << 2) | (u32::from(b[28]) << 10)) & 0x3FFF) + 1;
            return Some((w, h, "image/webp"));
        } else if fourcc == b"VP8X" {
            let w = u32::from_le_bytes([b[24], b[25], b[26], b[27]]);
            let h = u32::from_le_bytes([b[28], b[29], b[30], b[31]]);
            return Some((w + 1, h + 1, "image/webp"));
        }
        return Some((0, 0, "image/webp"));
    }
    None
}