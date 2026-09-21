-- Characters + age variants + appearance variants + reference assets.

CREATE TABLE characters (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    name            TEXT NOT NULL,
    occupation      TEXT,
    bio             TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(story_id, slug)
);

CREATE TABLE character_ages (
    id              TEXT PRIMARY KEY,
    character_id    TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    age             INTEGER NOT NULL,
    note            TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    UNIQUE(character_id, age)
);

CREATE TABLE character_appearances (
    id              TEXT PRIMARY KEY,
    character_id    TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    label           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    UNIQUE(character_id, label)
);

CREATE TABLE character_references (
    id              TEXT PRIMARY KEY,
    character_id    TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    label           TEXT NOT NULL,
    asset_path      TEXT,
    external_url    TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX character_references_character_idx ON character_references(character_id);
