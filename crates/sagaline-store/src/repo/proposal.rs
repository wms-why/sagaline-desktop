//! `proposals` table — the agent's batched-mutation ledger.
//!
//! A proposal bundles N `proposal_actions` (tool_name + JSON args)
//! into one atomic unit. Lifecycle:
//!
//! - **pending** — created by `propose_change`; rows in
//!   `proposal_actions` recorded but not yet executed.
//! - **committed** — `approve_proposal` ran the actions inside one
//!   transaction and validation passed.
//! - **rejected** — `reject_proposal` marked the batch dead; the
//!   rows in `proposal_actions` stay around for audit.
//! - **superseded** — reserved for future use (e.g. when a newer
//!   proposal lands for the same story before the older one is
//!   decided).

use rusqlite::{OptionalExtension as _, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

/// Lifecycle states for [`ProposalRow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
    Committed,
    Superseded,
}

impl ProposalStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ProposalStatus::Pending => "pending",
            ProposalStatus::Approved => "approved",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Committed => "committed",
            ProposalStatus::Superseded => "superseded",
        }
    }

    pub fn parse(s: &str) -> Result<Self, StoreError> {
        match s {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "committed" => Ok(Self::Committed),
            "superseded" => Ok(Self::Superseded),
            other => Err(StoreError::Other(format!(
                "unknown proposal status `{other}`"
            ))),
        }
    }
}

/// What the agent supplies when proposing a batch of changes.
#[derive(Debug, Clone)]
pub struct NewProposal<'a> {
    pub story_id: &'a str,
    pub agent_id: &'a str,
    pub summary: &'a str,
    /// Free-form JSON payload describing the diff (kept for the
    /// UI's pending-proposals view). Today the tool records an
    /// empty string; future turns may populate it from
    /// `sagaline-core::diff`.
    pub diff_json: &'a str,
}

/// One row of the `proposals` table. Stable id is a UUID v7 string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalRow {
    pub id: String,
    pub story_id: String,
    pub agent_id: String,
    pub status: ProposalStatus,
    pub summary: String,
    pub diff_json: String,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub decided_by: Option<String>,
}

impl ProposalRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let status_str: String = row.get("status")?;
        Ok(Self {
            id: row.get("id")?,
            story_id: row.get("story_id")?,
            agent_id: row.get("agent_id")?,
            status: ProposalStatus::parse(&status_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::other(e.to_string())),
                )
            })?,
            summary: row.get("summary")?,
            diff_json: row.get("diff_json")?,
            created_at: row.get("created_at")?,
            decided_at: row.get("decided_at")?,
            decided_by: row.get("decided_by")?,
        })
    }
}

pub struct ProposalRepo<'w> {
    world: &'w World,
}

impl<'w> ProposalRepo<'w> {
    pub fn new(world: &'w World) -> Self {
        Self { world }
    }

    /// Mint a fresh UUID v7. Convenience for callers / tests.
    pub fn new_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    pub fn conn(&self) -> Result<crate::pool::PooledConn, StoreError> {
        self.world.conn()
    }

    /// Create a new pending proposal.
    pub fn create(&self, new: NewProposal<'_>) -> Result<ProposalRow, StoreError> {
        let conn = self.conn()?;
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        conn.execute(
            "INSERT INTO proposals
                (id, story_id, agent_id, status, summary, diff_json,
                 created_at, decided_at, decided_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL)",
            rusqlite::params![
                id,
                new.story_id,
                new.agent_id,
                ProposalStatus::Pending.as_str(),
                new.summary,
                new.diff_json,
                now,
            ],
        )?;
        Ok(ProposalRow {
            id,
            story_id: new.story_id.into(),
            agent_id: new.agent_id.into(),
            status: ProposalStatus::Pending,
            summary: new.summary.into(),
            diff_json: new.diff_json.into(),
            created_at: now,
            decided_at: None,
            decided_by: None,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<ProposalRow>, StoreError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, story_id, agent_id, status, summary, diff_json,
                        created_at, decided_at, decided_by
                 FROM proposals WHERE id = ?1",
                [id],
                ProposalRow::from_row,
            )
            .optional()?;
        Ok(row)
    }

    /// List proposals, optionally filtered by story and status.
    /// Newest first.
    pub fn list(
        &self,
        story_id: Option<&str>,
        status: Option<ProposalStatus>,
    ) -> Result<Vec<ProposalRow>, StoreError> {
        let conn = self.conn()?;
        // Four-branch dispatcher keeps the prepared-statement
        // bindings honest (no temporary `&str`s outliving the
        // match arm).
        match (story_id, status) {
            (Some(sid), Some(st)) => {
                let st_str = st.as_str();
                let mut stmt = conn.prepare(
                    "SELECT id, story_id, agent_id, status, summary, diff_json,
                            created_at, decided_at, decided_by
                     FROM proposals
                     WHERE story_id = ?1 AND status = ?2
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![sid, st_str], ProposalRow::from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
            (Some(sid), None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, story_id, agent_id, status, summary, diff_json,
                            created_at, decided_at, decided_by
                     FROM proposals
                     WHERE story_id = ?1
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map([sid], ProposalRow::from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
            (None, Some(st)) => {
                let st_str = st.as_str();
                let mut stmt = conn.prepare(
                    "SELECT id, story_id, agent_id, status, summary, diff_json,
                            created_at, decided_at, decided_by
                     FROM proposals
                     WHERE status = ?1
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map([st_str], ProposalRow::from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
            (None, None) => {
                let mut stmt = conn.prepare(
                    "SELECT id, story_id, agent_id, status, summary, diff_json,
                            created_at, decided_at, decided_by
                     FROM proposals
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map([], ProposalRow::from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
        }
    }

    /// Convenience: list every pending proposal, regardless of story.
    pub fn list_pending(&self) -> Result<Vec<ProposalRow>, StoreError> {
        self.list(None, Some(ProposalStatus::Pending))
    }

    /// Transition a proposal to a new terminal/intermediate status.
    /// Returns `true` if a row was updated.
    pub fn set_status(
        &self,
        id: &str,
        status: ProposalStatus,
        decided_by: &str,
    ) -> Result<bool, StoreError> {
        let conn = self.conn()?;
        let now = now_iso();
        let updated = conn.execute(
            "UPDATE proposals
             SET status = ?1, decided_at = ?2, decided_by = ?3
             WHERE id = ?4",
            rusqlite::params![status.as_str(), now, decided_by, id],
        )?;
        Ok(updated > 0)
    }

    /// Same as [`Self::set_status`], but inside a caller-supplied
    /// transaction. Used by `approve_proposal` so the status flip
    /// and the world-state writes commit atomically.
    pub fn set_status_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        id: &str,
        status: ProposalStatus,
        decided_by: &str,
    ) -> Result<bool, StoreError> {
        let now = now_iso();
        let updated = tx.execute(
            "UPDATE proposals
             SET status = ?1, decided_at = ?2, decided_by = ?3
             WHERE id = ?4",
            rusqlite::params![status.as_str(), now, decided_by, id],
        )?;
        Ok(updated > 0)
    }

    pub fn delete(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.conn()?;
        let removed = conn.execute("DELETE FROM proposals WHERE id = ?1", [id])?;
        Ok(removed > 0)
    }

    /// Sanity-check helper used by integration tests.
    #[doc(hidden)]
    pub fn _count(&self) -> Result<i64, StoreError> {
        let conn = self.conn()?;
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM proposals", [], |row| row.get(0))?;
        Ok(n)
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
    fn create_and_get_round_trips() {
        let world = fresh();
        let sid = seed_story(&world);
        let row = world
            .proposals()
            .create(NewProposal {
                story_id: &sid,
                agent_id: "agent-1",
                summary: "first batch",
                diff_json: "",
            })
            .unwrap();
        assert_eq!(row.status, ProposalStatus::Pending);
        assert_eq!(row.story_id, sid);
        assert_eq!(row.agent_id, "agent-1");

        let fetched = world.proposals().get(&row.id).unwrap().unwrap();
        assert_eq!(fetched.id, row.id);
        assert_eq!(fetched.status, ProposalStatus::Pending);
    }

    #[test]
    fn set_status_flips_terminal() {
        let world = fresh();
        let sid = seed_story(&world);
        let row = world
            .proposals()
            .create(NewProposal {
                story_id: &sid,
                agent_id: "a",
                summary: "x",
                diff_json: "",
            })
            .unwrap();

        let updated = world
            .proposals()
            .set_status(&row.id, ProposalStatus::Rejected, "user-1")
            .unwrap();
        assert!(updated);
        let fetched = world.proposals().get(&row.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProposalStatus::Rejected);
        assert_eq!(fetched.decided_by.as_deref(), Some("user-1"));
        assert!(fetched.decided_at.is_some());
    }

    #[test]
    fn list_filters_by_story_and_status() {
        let world = fresh();
        let sa = seed_story(&world);
        let sb = world
            .stories()
            .create(NewStory {
                slug: "b",
                title: "B",
                summary: "",
            })
            .unwrap()
            .id;

        let pa = world
            .proposals()
            .create(NewProposal {
                story_id: &sa,
                agent_id: "a",
                summary: "a",
                diff_json: "",
            })
            .unwrap();
        let _pb = world
            .proposals()
            .create(NewProposal {
                story_id: &sb,
                agent_id: "a",
                summary: "b",
                diff_json: "",
            })
            .unwrap();
        let _pc = world
            .proposals()
            .create(NewProposal {
                story_id: &sa,
                agent_id: "a",
                summary: "a2",
                diff_json: "",
            })
            .unwrap();

        // All pending
        let all = world.proposals().list_pending().unwrap();
        assert_eq!(all.len(), 3);

        // Filter by story
        let by_story = world.proposals().list(Some(&sa), None).unwrap();
        assert_eq!(by_story.len(), 2);

        // Filter by status (none of them rejected yet)
        let pending = world.proposals().list(None, Some(ProposalStatus::Pending)).unwrap();
        assert_eq!(pending.len(), 3);

        // Reject one and re-filter
        world
            .proposals()
            .set_status(&pa.id, ProposalStatus::Rejected, "user-1")
            .unwrap();
        let pending = world.proposals().list(Some(&sa), Some(ProposalStatus::Pending)).unwrap();
        assert_eq!(pending.len(), 1);
        let rejected = world.proposals().list(None, Some(ProposalStatus::Rejected)).unwrap();
        assert_eq!(rejected.len(), 1);
    }

    #[test]
    fn set_status_in_tx_updates_inside_transaction() {
        let world = fresh();
        let sid = seed_story(&world);
        let row = world
            .proposals()
            .create(NewProposal {
                story_id: &sid,
                agent_id: "a",
                summary: "x",
                diff_json: "",
            })
            .unwrap();

        let mut conn = world.conn().unwrap();
        let tx = conn.transaction().unwrap();
        let updated = world
            .proposals()
            .set_status_in_tx(&tx, &row.id, ProposalStatus::Committed, "user-1")
            .unwrap();
        assert!(updated);
        tx.commit().unwrap();

        let fetched = world.proposals().get(&row.id).unwrap().unwrap();
        assert_eq!(fetched.status, ProposalStatus::Committed);
    }
}
