-- Story versions. snapshot_json is the serialized world state at the
-- moment of snapshot; rollback rebuilds from it.

CREATE TABLE story_versions (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    version         INTEGER NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    snapshot_json   TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    UNIQUE(story_id, version)
);
CREATE INDEX story_versions_story_version_idx ON story_versions(story_id, version DESC);
