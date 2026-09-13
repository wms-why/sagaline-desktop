//! Text generation via MiniMax's Anthropic-compatible endpoint.
//!
//! Endpoint: `POST {base}/anthropic/v1/messages`
//! Auth: `Authorization: Bearer <key>` (handled by `http::Client`).
//!
//! We translate `TextRequest` to the Anthropic Messages shape; the
//! response is parsed minimally — we only extract the concatenated
//! text blocks. Tool calls / thinking blocks are ignored in v1.

use serde::{Deserialize, Serialize};

use sagaline_core::provider::{ModelId, ProviderError, TextRequest, TextResponse, Usage};

use super::MiniMaxProvider;

/// Default model when the request leaves `model` unset. Latest MiniMax M-series.
pub const TEXT_DEFAULT_MODEL: &str = "MiniMax-M3";

#[derive(Debug, Serialize)]
struct MessagesReq<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: Vec<Msg<'a>>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stop_sequences: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Msg<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct MessagesResp {
    #[serde(default)]
    content: Vec<Block>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
}

pub async fn generate_text(
    p: &MiniMaxProvider,
    req: TextRequest,
) -> Result<TextResponse, ProviderError> {
    if req.prompt.trim().is_empty() {
        return Err(ProviderError::Config("prompt is required".into()));
    }
    let model = req
        .model
        .as_ref()
        .map(|m| m.as_str())
        .unwrap_or(&p.cfg().default_text_model);

    let body = MessagesReq {
        model,
        system: req.system.as_deref(),
        messages: vec![Msg { role: "user", content: &req.prompt }],
        // Anthropic requires max_tokens; default to a sensible value.
        max_tokens: req.max_tokens.unwrap_or(2048),
        temperature: req.temperature,
        top_p: req.top_p,
        stop_sequences: req.stop,
    };

    let req = p.http().post("/anthropic/v1/messages")?.json(&body);
    let resp = p.http().send(req).await?;
    let parsed: MessagesResp = resp
        .json()
        .await
        .map_err(|e| ProviderError::Decode(format!("text: {e}")))?;

    // Concatenate all `text` blocks; skip `thinking` blocks for v1.
    let mut text = String::new();
    for b in parsed.content {
        if b.kind == "text" && !b.text.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&b.text);
        }
    }

    let usage = parsed.usage.map(|u| Usage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: match (u.input_tokens, u.output_tokens) {
            (Some(i), Some(o)) => Some(i + o),
            _ => None,
        },
    });

    Ok(TextResponse {
        text,
        model: ModelId::new(parsed.model.unwrap_or_else(|| model.to_string())),
        stop_reason: parsed.stop_reason,
        usage,
    })
}