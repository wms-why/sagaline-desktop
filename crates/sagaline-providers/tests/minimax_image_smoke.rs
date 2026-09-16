//! Smoke test for minimax image generation.
//!
//! Gated on the `MINIMAX_API_KEY` environment variable so it doesn't
//! run by default. To run:
//!
//! ```bash
//! MINIMAX_API_KEY=sk-xxxxx \
//!   cargo test -p sagaline-providers --test minimax_image_smoke -- --nocapture
//! ```
//!
//! The test:
//!
//! 1. Constructs a `MinimaxImage` against the global endpoint.
//! 2. Asks for a small 1:1 image of a "blank" subject.
//! 3. Writes the bytes to a temp file.
//! 4. Asserts the file is non-empty and starts with the PNG magic.
//!
//! Failures here indicate either a real API error (network, auth,
//! quota) or a wire-shape change in the provider's response. Both
//! need to be triaged; the test is intentionally loud.

use std::io::Write as _;

use sagaline_providers::minimax::MinimaxImage;
use sagaline_providers::{GenerationRequest, ImageGen, ModelAdapter};
use secrecy::SecretString;

const DEFAULT_BASE_URL: &str = "https://api.minimax.chat/v1";
const DEFAULT_MODEL: &str = "image-01";

#[tokio::test]
async fn minimax_image_smoke() {
    let key = match std::env::var("MINIMAX_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!(
                "MINIMAX_API_KEY not set; skipping smoke test. \
                 Set it and re-run to actually hit the API."
            );
            return;
        }
    };

    let base_url = std::env::var("MINIMAX_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let model = std::env::var("MINIMAX_IMAGE_MODEL")
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    let img = MinimaxImage::new(
        SecretString::new(key.into_boxed_str()),
        base_url.clone(),
        model.clone(),
    );

    let prompt = "a single solid grey square, 16x16, no other content";
    let req = GenerationRequest {
        prompt,
        negative_prompt: None,
        reference_images: &[],
        seed: Some(42),
        aspect_ratio: Some("1:1"),
        extra: &serde_json::json!({}),
    };

    let out = img
        .generate(req)
        .await
        .expect("MinimaxImage::generate failed");

    assert!(!out.bytes.is_empty(), "got empty bytes from minimax");
    assert!(
        out.bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "bytes don't start with PNG magic"
    );

    let mut tmp = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("tempfile");
    tmp.write_all(&out.bytes).expect("write png");
    eprintln!(
        "minimax smoke OK: {} bytes, wrote to {} (provider={}, model={})",
        out.bytes.len(),
        tmp.path().display(),
        img.provider_name(),
        model,
    );
}