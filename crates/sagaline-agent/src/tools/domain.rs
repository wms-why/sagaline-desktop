//! Shared helpers for domain tools: arg parsing, error mapping,
//! JSON output. Each tool file still implements [`Tool`] directly
//! so it can put its own blurb in the descriptor; this module just
//! removes the boilerplate that would otherwise be duplicated.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::tool::{ToolError, ToolResult};

/// Parse JSON args into a typed `Args` struct. Maps the
/// deserialization error into [`ToolError::BadArgs`] tagged with
/// the tool's name.
pub fn parse_args<Args: DeserializeOwned>(
    tool_name: &'static str,
    args: Value,
) -> Result<Args, ToolError> {
    serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
        name: tool_name.to_string(),
        message: e.to_string(),
    })
}

/// Serialize a typed result to a [`ToolResult`] (a JSON value). Used
/// at the end of every tool's `execute` to produce the
/// chain-of-thought-friendly JSON the LLM sees.
pub fn to_result<T: Serialize>(value: &T) -> Result<ToolResult, ToolError> {
    serde_json::to_value(value).map_err(|e| ToolError::Execution {
        name: "serialize".into(),
        source: Box::new(e),
    })
}

/// Map any error convertible into [`sagaline_store::StoreError`] into a
/// [`ToolError::Execution`] tagged with the calling tool's name.
/// Lets call sites forward either a raw [`rusqlite::Error`] or a
/// [`sagaline_store::StoreError`] without a manual conversion.
pub fn map_store_err<E>(tool_name: &'static str, e: E) -> ToolError
where
    E: Into<sagaline_store::StoreError>,
{
    ToolError::Execution {
        name: tool_name.to_string(),
        source: Box::new(e.into()),
    }
}
