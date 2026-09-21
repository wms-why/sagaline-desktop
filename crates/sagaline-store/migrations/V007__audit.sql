-- Agent action audit log. Every tool invocation lands here, regardless
-- of capability tier.

CREATE TABLE agent_actions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id        TEXT NOT NULL,
    agent_id        TEXT NOT NULL,
    tool_name       TEXT NOT NULL,
    args_json       TEXT NOT NULL,
    result_summary  TEXT,
    proposal_id     TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX agent_actions_story_created_idx ON agent_actions(story_id, created_at);
