//! `proposal_actions` table — the queued-but-not-yet-applied tool
//! calls inside a [`ProposalRow`].
//!
//! Each row is `(seq, tool_name, tool_args)` keyed by the owning
//! proposal. `approve_proposal` replays them in `seq` order,
//! inside one transaction, after `validate_world` has passed.

use rusqlite::Row;
use serde::Serialize;

use crate::error::StoreError;
use crate::world::World;

/// What the agent / tool supplies when appending a proposal action.
#[derive(Debug, Clone)]
pub struct NewProposalAction<'a> {
    pub proposal_id: &'a str,
    pub seq: i64,
    pub tool_name: &'a str,
    /// Already-serialized JSON (a tool args payload). Stored as
    /// TEXT in the DB; we don't re-serialize to keep the recorded
    /// bytes byte-identical to what was proposed.
    pub tool_args_json: &'a str,
}

/// One row of the `proposal_actions` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalActionRow {
    pub id: i64,
    pub proposal_id: String,
    pub seq: i64,
    pub tool_name: String,
    pub tool_args_json: String,
}

impl ProposalActionRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            proposal_id: row.get("proposal_id")?,
            seq: row.get("seq")?,
            tool_name: row.get("tool_name")?,
            tool_args_json: row.get("tool_args")?,
        })
    }
}

pub struct ProposalActionRepo<'w> {
    world: &'w World,
}

impl<'w> ProposalActionRepo<'w> {
    pub fn new(world: &'w World) -> Self {
        Self { world }
    }

    /// Append a single action. Returns the new row's primary key.
    /// The caller assigns `seq` (1-based); the
    /// `UNIQUE(proposal_id, seq)` constraint enforces uniqueness.
    pub fn record(&self, new: NewProposalAction<'_>) -> Result<i64, StoreError> {
        let conn = self.world.conn()?;
        conn.execute(
            "INSERT INTO proposal_actions (proposal_id, seq, tool_name, tool_args)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                new.proposal_id,
                new.seq,
                new.tool_name,
                new.tool_args_json,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Same as [`Self::record`], inside a caller-supplied transaction.
    /// Used by the `propose_change` tool when it bundles multiple
    /// actions into one proposal.
    pub fn record_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewProposalAction<'_>,
    ) -> Result<i64, StoreError> {
        tx.execute(
            "INSERT INTO proposal_actions (proposal_id, seq, tool_name, tool_args)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                new.proposal_id,
                new.seq,
                new.tool_name,
                new.tool_args_json,
            ],
        )?;
        Ok(tx.last_insert_rowid())
    }

    /// List every action belonging to a proposal, oldest (lowest
    /// `seq`) first. The replay order for `approve_proposal`.
    pub fn list_by_proposal(
        &self,
        proposal_id: &str,
    ) -> Result<Vec<ProposalActionRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, proposal_id, seq, tool_name, tool_args
             FROM proposal_actions
             WHERE proposal_id = ?1
             ORDER BY seq ASC",
        )?;
        let rows = stmt
            .query_map([proposal_id], ProposalActionRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete_by_proposal(&self, proposal_id: &str) -> Result<usize, StoreError> {
        let conn = self.world.conn()?;
        let n = conn.execute(
            "DELETE FROM proposal_actions WHERE proposal_id = ?1",
            [proposal_id],
        )?;
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

    fn seed(world: &World) -> (String, String) {
        let sid = world
            .stories()
            .create(NewStory {
                slug: "s",
                title: "Story",
                summary: "",
            })
            .unwrap()
            .id;
        let pid = world
            .proposals()
            .create(crate::NewProposal {
                story_id: &sid,
                agent_id: "a",
                summary: "x",
                diff_json: "",
            })
            .unwrap()
            .id;
        (sid, pid)
    }

    #[test]
    fn record_and_list_by_proposal_orders_by_seq() {
        let world = fresh();
        let (_sid, pid) = seed(&world);

        // Insert out of order to verify the seq-based ORDER BY.
        world
            .proposal_actions()
            .record(NewProposalAction {
                proposal_id: &pid,
                seq: 2,
                tool_name: "create_character",
                tool_args_json: "{\"k\":2}",
            })
            .unwrap();
        world
            .proposal_actions()
            .record(NewProposalAction {
                proposal_id: &pid,
                seq: 1,
                tool_name: "create_chapter",
                tool_args_json: "{\"k\":1}",
            })
            .unwrap();

        let rows = world.proposal_actions().list_by_proposal(&pid).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].seq, 1);
        assert_eq!(rows[0].tool_name, "create_chapter");
        assert_eq!(rows[1].seq, 2);
        assert_eq!(rows[1].tool_name, "create_character");
    }

    #[test]
    fn record_in_tx_rolls_back_when_tx_is_dropped() {
        let world = fresh();
        let (_sid, pid) = seed(&world);

        {
            let mut conn = world.conn().unwrap();
            let tx = conn.transaction().unwrap();
            world
                .proposal_actions()
                .record_in_tx(
                    &tx,
                    NewProposalAction {
                        proposal_id: &pid,
                        seq: 1,
                        tool_name: "create_character",
                        tool_args_json: "{}",
                    },
                )
                .unwrap();
            // tx is dropped here without commit
        }

        let rows = world.proposal_actions().list_by_proposal(&pid).unwrap();
        assert!(rows.is_empty());
    }
}
