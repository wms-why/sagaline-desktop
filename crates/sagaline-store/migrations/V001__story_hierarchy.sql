-- Story hierarchy: stories → chapters → scenes → shots.
-- All IDs are UUID v7 strings (TEXT). Slugs are unique within their parent
-- for stable exporter paths.

CREATE TABLE stories (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    summary         TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE chapters (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    ordinal         INTEGER NOT NULL,
    title           TEXT NOT NULL,
    synopsis        TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(story_id, slug),
    UNIQUE(story_id, ordinal)
);
CREATE INDEX chapters_story_ordinal_idx ON chapters(story_id, ordinal);

CREATE TABLE scenes (
    id              TEXT PRIMARY KEY,
    chapter_id      TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    ordinal         INTEGER NOT NULL,
    title           TEXT NOT NULL,
    synopsis        TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(chapter_id, slug),
    UNIQUE(chapter_id, ordinal)
);
CREATE INDEX scenes_chapter_ordinal_idx ON scenes(chapter_id, ordinal);

CREATE TABLE shots (
    id              TEXT PRIMARY KEY,
    scene_id        TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    slug            TEXT NOT NULL,
    ordinal         INTEGER NOT NULL,
    title           TEXT NOT NULL,
    duration_sec    REAL,
    prompt          TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE(scene_id, slug),
    UNIQUE(scene_id, ordinal)
);
CREATE INDEX shots_scene_ordinal_idx ON shots(scene_id, ordinal);
