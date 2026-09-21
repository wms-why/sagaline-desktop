//! `agent_actions` table — the audit log of every tool invocation.
//!
//! Every tool call lands here, regardless of capability tier. The
//! `proposal_id` column is `NULL` for `Read`/`Execute` calls
//! (no proposal context) and points at the owning [`ProposalRow`]
//! for `Mutate` calls that are part of a proposal's replay.

use rusqlite::{OptionalExtension as _, Row};
use serde::Serialize;

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

/// What callers / tests supply when writing an audit row.
#[derive(Debug, Clone)]
pub struct NewAgentAction<'a> {
    pub story_id: &'a str,
    pub agent_id: &'a str,
    pub tool_name: &'a str,
    /// JSON-serialized tool args.
    pub args_json: &'a str,
    /// Short, human-readable summary of the tool result (typically
    /// the result's `summary` field). `None` if the call failed
    /// before producing a result.
    pub result_summary: Option<&'a str>,
    /// `Some(proposal_id)` if this call is part of a proposal's
    /// replay; `None` for standalone calls.
    pub proposal_id: Option<&'a str>,
}

/// One row of the `agent_actions` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentActionRow {
    pub id: i64,
    pub story_id: String,
    pub agent_id: String,
    pub tool_name: String,
    pub args_json: String,
    pub result_summary: Option<String>,
    pub proposal_id: Option<String>,
    pub created_at: String,
}

impl AgentActionRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            story_id: row.get("story_id")?,
            agent_id: row.get("agent_id")?,
            tool_name: row.get("tool_name")?,
            args_json: row.get("args_json")?,
            result_summary: row.get("result_summary")?,
            proposal_id: row.get("proposal_id")?,
            created_at: row.get("created_at")?,
        })
    }
}

pub struct AgentActionRepo<'w> {
    world: &'w World,
}

impl<'w> AgentActionRepo<'w> {
    pub fn new(world: &'w World) -> Self {
        Self { world }
    }

    /// Append one audit row.
    pub fn record(&self, new: NewAgentAction<'_>) -> Result<i64, StoreError> {
        let conn = self.world.conn()?;
        let now = now_iso();
        conn.execute(
            "INSERT INTO agent_actions
                (story_id, agent_id, tool_name, args_json,
                 result_summary, proposal_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                new.story_id,
                new.agent_id,
                new.tool_name,
                new.args_json,
                new.result_summary,
                new.proposal_id,
                now,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Same as [`Self::record`], inside a caller-supplied
    /// transaction. Used by `approve_proposal` so each replayed
    /// action's audit row lands atomically with the world-state
    /// writes.
    pub fn record_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewAgentAction<'_>,
    ) -> Result<i64, StoreError> {
        let now = now_iso();
        tx.execute(
            "INSERT INTO agent_actions
                (story_id, agent_id, tool_name, args_json,
                 result_summary, proposal_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                new.story_id,
                new.agent_id,
                new.tool_name,
                new.args_json,
                new.result_summary,
                new.proposal_id,
                now,
            ],
        )?;
        // rusqlite exposes last_insert_rowid on Transaction.
        let id = tx.last_insert_rowid();
        // SQLite returns 0 when no ROWID was assigned; surface as
        // an explicit error so callers don't quietly lose audit rows.
        if id == 0 {
            return Err(StoreError::Other(
                "agent_actions: last_insert_rowid returned 0".into(),
            ));
        }
        Ok(id)
    }

    pub fn list_by_story(&self, story_id: &str) -> Result<Vec<AgentActionRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, story_id, agent_id, tool_name, args_json,
                    result_summary, proposal_id, created_at
             FROM agent_actions
             WHERE story_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([story_id], AgentActionRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_by_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<Vec<AgentActionRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, story_id, agent_id, tool_name, args_json,
                    result_summary, proposal_id, created_at
             FROM agent_actions
             WHERE proposal_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([proposal_id], AgentActionRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn latest_for_tool(
        &self,
        story_id: &str,
        tool_name: &str,
    ) -> Result<Option<AgentActionRow>, StoreError> {
        let conn = self.world.conn()?;
        let row = conn
            .query_row(
                "SELECT id, story_id, agent_id, tool_name, args_json,
                        result_summary, proposal_id, created_at
                 FROM agent_actions
                 WHERE story_id = ?1 AND tool_name = ?2
                 ORDER BY created_at DESC, id DESC
                 LIMIT 1",
                rusqlite::params![story_id, tool_name],
                AgentActionRow::from_row,
            )
            .optional()?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::NewStory;

    fn fresh() -> World {
        World::in_memory().expect("world")
    }

    fn seed_story(world: &World) -> String {
        world
            .stories()
            .create(NewStory {
                slug: "s",
                title: "Story",
                summary: "",
            })
            .unwrap()
            .id
    }

    #[test]
    fn record_and_list_by_story() {
        let world = fresh();
        let sid = seed_story(&world);

        world
            .agent_actions()
            .record(NewAgentAction {
                story_id: &sid,
                agent_id: "a",
                tool_name: "get_story",
                args_json: "{}",
                result_summary: Some("ok"),
                proposal_id: None,
            })
            .unwrap();
        world
            .agent_actions()
            .record(NewAgentAction {
                story_id: &sid,
                agent_id: "a",
                tool_name: "create_character",
                args_json: "{}",
                result_summary: Some("created"),
                proposal_id: None,
            })
            .unwrap();

        let rows = world.agent_actions().list_by_story(&sid).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tool_name, "get_story");
        assert_eq!(rows[1].tool_name, "create_character");
    }

    #[test]
    fn latest_for_tool_picks_most_recent() {
        let world = fresh();
        let sid = seed_story(&world);

        world
            .agent_actions()
            .record(NewAgentAction {
                story_id: &sid,
                agent_id: "a",
                tool_name: "get_story",
                args_json: "{\"v\":1}",
                result_summary: Some("first"),
                proposal_id: None,
            })
            .unwrap();
        world
            .agent_actions()
            .record(NewAgentAction {
                story_id: &sid,
                agent_id: "a",
                tool_name: "get_story",
                args_json: "{\"v\":2}",
                result_summary: Some("second"),
                proposal_id: None,
            })
            .unwrap();

        let latest = world
            .agent_actions()
            .latest_for_tool(&sid, "get_story")
            .unwrap()
            .unwrap();
        assert_eq!(latest.result_summary.as_deref(), Some("second"));
    }
}
