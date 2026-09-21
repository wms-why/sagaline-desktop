-- Many-to-many relation tables. Composite primary keys keep uniqueness;
-- FK ON DELETE CASCADE means removing a scene scrubs its assignments.

CREATE TABLE scene_characters (
    scene_id            TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    character_id        TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    character_age_id    TEXT REFERENCES character_ages(id) ON DELETE SET NULL,
    appearance_id       TEXT REFERENCES character_appearances(id) ON DELETE SET NULL,
    PRIMARY KEY (scene_id, character_id)
);

CREATE TABLE scene_environments (
    scene_id        TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    environment_id  TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    PRIMARY KEY (scene_id, environment_id)
);

CREATE TABLE shot_characters (
    shot_id         TEXT NOT NULL REFERENCES shots(id) ON DELETE CASCADE,
    character_id    TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    PRIMARY KEY (shot_id, character_id)
);

CREATE TABLE shot_references (
    shot_id         TEXT NOT NULL REFERENCES shots(id) ON DELETE CASCADE,
    reference_id    TEXT NOT NULL,
    reference_kind  TEXT NOT NULL,
    PRIMARY KEY (shot_id, reference_id, reference_kind)
);
