//! `validate_world` — cross-table validation of the world DB.
//! Mirrors the wire shape from the old Markdown-era
//! `validate_story` (`{ok, errors}`) so the agent's REFLECT step
//! doesn't need to change.
//!
//! The SQL lives in `sagaline_store::validation::validate_world_in_tx`
//! so the same checks can run inside the `approve_proposal`
//! transaction. This tool just delegates to it on a fresh
//! connection.

use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use sagaline_store::{validate_world_in_tx, ValidationIssue, World};

use crate::tool::{Capability, Tool, ToolContext, ToolDescriptor, ToolError, ToolResult};
use crate::tools::domain;

/// `validate_world` takes no required arguments. Reserved for
/// future filtering (e.g. validate a single story).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ValidateWorldArgs {
    #[serde(default)]
    pub story_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidateWorldOutput {
    pub ok: bool,
    pub errors: Vec<ValidationIssue>,
}

pub struct ValidateWorldTool {
    world: Arc<World>,
}

impl ValidateWorldTool {
    pub fn new(world: Arc<World>) -> Self {
        Self { world }
    }
}

#[async_trait]
impl Tool for ValidateWorldTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ValidateWorldArgs>(
            "validate_world",
            "Cross-table validation of the world DB. SQLite FK \
             constraints cover most references at write time; \
             this tool surfaces references that are technically \
             valid but semantically wrong (e.g. a scene's \
             character lives in a different story). Returns \
             `{ok: bool, errors: [{kind, ...}]}`. Optional \
             `story_id` restricts the check to one story.",
            Capability::Read,
        )
    }

    async fn execute(&self, _ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let parsed: ValidateWorldArgs = domain::parse_args("validate_world", args)?;
        let conn = self
            .world
            .conn()
            .map_err(|e| domain::map_store_err("validate_world", e))?;
        let errors = validate_world_in_tx(&conn, parsed.story_id.as_deref())
            .map_err(|e| domain::map_store_err("validate_world", e))?;
        let ok = errors.is_empty();
        domain::to_result(&ValidateWorldOutput { ok, errors })
    }
}
