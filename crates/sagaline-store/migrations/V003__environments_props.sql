-- Environments and props (single-file entities) plus their reference assets.

CREATE TABLE environments (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(story_id, slug)
);

CREATE TABLE environment_references (
    id              TEXT PRIMARY KEY,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    label           TEXT NOT NULL,
    asset_path      TEXT,
    external_url    TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX environment_references_env_idx ON environment_references(environment_id);

CREATE TABLE props (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(story_id, slug)
);

CREATE TABLE prop_references (
    id              TEXT PRIMARY KEY,
    prop_id         TEXT NOT NULL REFERENCES props(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    label           TEXT NOT NULL,
    asset_path      TEXT,
    external_url    TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX prop_references_prop_idx ON prop_references(prop_id);
