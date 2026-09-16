//! The agent's tool registry.
//!
//! Every action the agent can take — reading a file, generating an image,
//! validating the story graph, etc. — is a [`Tool`]. Tools describe
//! themselves with a JSON Schema so a future LLM client can advertise
//! them via function-calling without further translation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use schemars::{schema::RootSchema, schema_for};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

/// What a tool reports back to the agent. Tool outputs are JSON values;
/// the agent's prompt assembly (`crate::prompt`) is the only place that
/// knows how to render them.
pub type ToolResult = Value;

/// Errors a tool can raise. Kept narrow — anything else is a `Bug`.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool `{name}` is not registered")]
    UnknownTool { name: String },

    #[error("tool `{name}` received invalid arguments: {message}")]
    BadArgs { name: String, message: String },

    #[error("tool `{name}` failed: {source}")]
    Execution {
        name: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("I/O error in tool `{name}` at {path}: {source}")]
    Io {
        name: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Static descriptor a tool exposes to the agent (and, later, to an LLM).
///
/// Mirrors the shape of an OpenAI function-calling tool definition:
/// name, human-readable description, and a JSON Schema for arguments.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub parameters: RootSchema,
}

impl ToolDescriptor {
    /// Build a descriptor from a name, description, and a sample argument
    /// type whose JSON Schema the tool wants to advertise.
    pub fn for_args<T>(name: impl Into<String>, description: impl Into<String>) -> Self
    where
        T: schemars::JsonSchema,
    {
        Self {
            name: name.into(),
            description: description.into(),
            parameters: schema_for!(T),
        }
    }
}

/// A single agent tool. Implementations are stateless and cheap to
/// clone; the registry holds `Box<dyn Tool>`. `execute` is async so the
/// trait can wrap rig `Tool` invocations and HTTP I/O without spawning
/// a per-call runtime.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Static metadata. Must be cheap — called once at registration.
    fn descriptor(&self) -> ToolDescriptor;

    /// Execute with JSON arguments. Argument shape is the one advertised
    /// in [`ToolDescriptor::parameters`]. Returns a [`ToolResult`] on
    /// success; on failure returns a [`ToolError`].
    async fn execute(&self, args: Value) -> Result<ToolResult, ToolError>;
}

/// A registry of tools, keyed by name. The agent looks up tools here
/// before invoking them.
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Overwrites any existing tool with the same name;
    /// the agent does not support duplicate names by design.
    pub fn register<T: Tool + 'static>(&mut self, tool: T) -> &mut Self {
        let descriptor = tool.descriptor();
        self.tools.insert(descriptor.name, Box::new(tool));
        self
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// All registered tool descriptors, in name order. Used to assemble
    /// the LLM tool list once a real client lands.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.values().map(|t| t.descriptor()).collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}
