-- Proposal → Commit workflow (Phase 3). Schema is defined now so the
-- tools crate can be wired against it; full repo impl lands in Phase 3.

CREATE TABLE proposals (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    agent_id        TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN
                        ('pending','approved','rejected','committed','superseded')),
    summary         TEXT NOT NULL,
    diff_json       TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    decided_at      TEXT,
    decided_by      TEXT
);
CREATE INDEX proposals_story_status_idx ON proposals(story_id, status);

CREATE TABLE proposal_actions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    proposal_id     TEXT NOT NULL REFERENCES proposals(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    tool_name       TEXT NOT NULL,
    tool_args       TEXT NOT NULL,
    UNIQUE(proposal_id, seq)
);
