//! SQLite persistence for the story workspace.
//!
//! One database file per project (the AGENTS.md plan). The schema is created
//! idempotently on open via `CREATE TABLE IF NOT EXISTS`, so opening an empty
//! path is enough to bootstrap a project.
//!
//! The layer is sync `rusqlite` — the desktop app calls it from `cx.spawn`
//! tasks, not from inside the render closure. UI reads are expected to be
//! cached in a `WorkspaceState` and refreshed after writes.

use std::path::Path;

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::model::*;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(&'static str),
    #[error("invalid join: {0}")]
    InvalidJoin(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open an in-memory database. Useful for tests and for the first-launch
/// bootstrap before the user picks a project path.
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS stories (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    cover           TEXT,
    genre           TEXT,
    style           TEXT,
    language        TEXT,
    target_audience TEXT,
    visual_style    TEXT,
    status          TEXT NOT NULL DEFAULT 'draft',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS story_bibles (
    story_id    TEXT PRIMARY KEY REFERENCES stories(id) ON DELETE CASCADE,
    world       TEXT NOT NULL DEFAULT '',
    rules       TEXT NOT NULL DEFAULT '',
    timeline    TEXT NOT NULL DEFAULT '',
    lore        TEXT NOT NULL DEFAULT '',
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS characters (
    id          TEXT PRIMARY KEY,
    story_id    TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    profile     TEXT NOT NULL DEFAULT '',
    personality TEXT NOT NULL DEFAULT '',
    background  TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_characters_story ON characters(story_id);

CREATE TABLE IF NOT EXISTS character_ages (
    id                TEXT PRIMARY KEY,
    character_id      TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    age               INTEGER NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    default_reference TEXT,
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ages_character ON character_ages(character_id);

CREATE TABLE IF NOT EXISTS character_appearances (
    id           TEXT PRIMARY KEY,
    age_id       TEXT NOT NULL REFERENCES character_ages(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    clothing     TEXT NOT NULL DEFAULT '',
    hairstyle    TEXT NOT NULL DEFAULT '',
    accessories  TEXT NOT NULL DEFAULT '',
    emotion      TEXT NOT NULL DEFAULT '',
    body_state   TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_appearances_age ON character_appearances(age_id);

CREATE TABLE IF NOT EXISTS environments (
    id          TEXT PRIMARY KEY,
    story_id    TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    architecture TEXT NOT NULL DEFAULT '',
    lighting    TEXT NOT NULL DEFAULT '',
    weather     TEXT NOT NULL DEFAULT '',
    time_of_day TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_environments_story ON environments(story_id);

CREATE TABLE IF NOT EXISTS props (
    id          TEXT PRIMARY KEY,
    story_id    TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    appearance  TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_props_story ON props(story_id);

CREATE TABLE IF NOT EXISTS relationships (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    from_character  TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    to_character    TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'active',
    note            TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_relationships_story ON relationships(story_id);

CREATE TABLE IF NOT EXISTS reference_images (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL,
    label       TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS reference_links (
    reference_id TEXT NOT NULL REFERENCES reference_images(id) ON DELETE CASCADE,
    target_kind  TEXT NOT NULL,
    target_id    TEXT NOT NULL,
    PRIMARY KEY (reference_id, target_kind, target_id)
);
CREATE INDEX IF NOT EXISTS idx_reflinks_target ON reference_links(target_kind, target_id);

CREATE TABLE IF NOT EXISTS chapters (
    id              TEXT PRIMARY KEY,
    story_id        TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    chapter_number  INTEGER NOT NULL,
    title           TEXT NOT NULL,
    summary         TEXT NOT NULL DEFAULT '',
    story           TEXT NOT NULL DEFAULT '',
    timeline        TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'draft',
    generated_video TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chapters_story ON chapters(story_id, chapter_number);

CREATE TABLE IF NOT EXISTS scenes (
    id                TEXT PRIMARY KEY,
    chapter_id        TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    scene_number      INTEGER NOT NULL,
    title             TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    environment_id    TEXT REFERENCES environments(id) ON DELETE SET NULL,
    emotion           TEXT NOT NULL DEFAULT '',
    duration_seconds  INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_scenes_chapter ON scenes(chapter_id, scene_number);

CREATE TABLE IF NOT EXISTS shots (
    id                  TEXT PRIMARY KEY,
    scene_id            TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    shot_number         INTEGER NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    camera              TEXT NOT NULL DEFAULT '',
    camera_movement     TEXT NOT NULL DEFAULT '',
    composition         TEXT NOT NULL DEFAULT '',
    action              TEXT NOT NULL DEFAULT '',
    dialogue            TEXT NOT NULL DEFAULT '',
    emotion             TEXT NOT NULL DEFAULT '',
    duration_seconds    INTEGER NOT NULL DEFAULT 0,
    environment_id      TEXT REFERENCES environments(id) ON DELETE SET NULL,
    first_frame         TEXT,
    last_frame          TEXT,
    video_model         TEXT,
    generation_settings TEXT NOT NULL DEFAULT '',
    generated_video     TEXT,
    created_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_shots_scene ON shots(scene_id, shot_number);

CREATE TABLE IF NOT EXISTS shot_characters (
    shot_id          TEXT NOT NULL REFERENCES shots(id) ON DELETE CASCADE,
    character_id     TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    age_id           TEXT NOT NULL REFERENCES character_ages(id) ON DELETE CASCADE,
    appearance_id    TEXT NOT NULL REFERENCES character_appearances(id) ON DELETE CASCADE,
    PRIMARY KEY (shot_id, character_id, appearance_id)
);

CREATE TABLE IF NOT EXISTS shot_props (
    shot_id   TEXT NOT NULL REFERENCES shots(id) ON DELETE CASCADE,
    prop_id   TEXT NOT NULL REFERENCES props(id) ON DELETE CASCADE,
    PRIMARY KEY (shot_id, prop_id)
);

CREATE TABLE IF NOT EXISTS chapter_characters (
    chapter_id   TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    character_id TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    PRIMARY KEY (chapter_id, character_id)
);

CREATE TABLE IF NOT EXISTS chapter_environments (
    chapter_id     TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    environment_id TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
    PRIMARY KEY (chapter_id, environment_id)
);
"#;

// ---------------------------------------------------------------------------
// Row → struct helpers
// ---------------------------------------------------------------------------

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn parse_status<S: AsRef<str>>(s: S) -> StoryStatus {
    match s.as_ref() {
        "active" => StoryStatus::Active,
        "completed" => StoryStatus::Completed,
        "archived" => StoryStatus::Archived,
        _ => StoryStatus::Draft,
    }
}

fn parse_chapter_status<S: AsRef<str>>(s: S) -> ChapterStatus {
    match s.as_ref() {
        "planning" => ChapterStatus::Planning,
        "generating" => ChapterStatus::Generating,
        "generated" => ChapterStatus::Generated,
        "published" => ChapterStatus::Published,
        _ => ChapterStatus::Draft,
    }
}

fn parse_ref_kind<S: AsRef<str>>(s: S) -> ReferenceKind {
    match s.as_ref() {
        "character" => ReferenceKind::Character,
        "environment" => ReferenceKind::Environment,
        "prop" => ReferenceKind::Prop,
        _ => ReferenceKind::Shot,
    }
}

fn parse_rel_status<S: AsRef<str>>(s: S) -> RelationshipStatus {
    match s.as_ref() {
        "broken" => RelationshipStatus::Broken,
        "ended" => RelationshipStatus::Ended,
        "unknown" => RelationshipStatus::Unknown,
        _ => RelationshipStatus::Active,
    }
}

fn row_to_story(row: &rusqlite::Row<'_>) -> rusqlite::Result<Story> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let status: String = row.get("status")?;
    Ok(Story {
        id: StoryId(row.get("id")?),
        title: row.get("title")?,
        description: row.get("description")?,
        cover: row.get("cover")?,
        genre: row.get("genre")?,
        style: row.get("style")?,
        language: row.get("language")?,
        target_audience: row.get("target_audience")?,
        visual_style: row.get("visual_style")?,
        status: parse_status(status),
        created_at: parse_dt(&created_at),
        updated_at: parse_dt(&updated_at),
    })
}

fn parse_dt(s: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

// ---------------------------------------------------------------------------
// Stories
// ---------------------------------------------------------------------------

pub fn create_story(conn: &Connection, title: &str) -> Result<Story> {
    let story = Story {
        id: StoryId::new(),
        title: title.to_string(),
        description: String::new(),
        cover: None,
        genre: None,
        style: None,
        language: None,
        target_audience: None,
        visual_style: None,
        status: StoryStatus::Draft,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    conn.execute(
        "INSERT INTO stories (id, title, description, status, created_at, updated_at)
         VALUES (?1, ?2, '', 'draft', ?3, ?3)",
        params![story.id.as_str(), story.title, now()],
    )?;
    // Bootstrap an empty bible so the right-pane has something to render.
    conn.execute(
        "INSERT INTO story_bibles (story_id, world, rules, timeline, lore, updated_at)
         VALUES (?1, '', '', '', '', ?2)",
        params![story.id.as_str(), now()],
    )?;
    Ok(story)
}

pub fn list_stories(conn: &Connection) -> Result<Vec<Story>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, description, cover, genre, style, language, \
                target_audience, visual_style, status, created_at, updated_at \
         FROM stories ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_story)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_story(conn: &Connection, id: &StoryId) -> Result<Story> {
    conn.query_row(
        "SELECT id, title, description, cover, genre, style, language, \
                target_audience, visual_style, status, created_at, updated_at \
         FROM stories WHERE id = ?1",
        params![id.as_str()],
        row_to_story,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound("story"),
        other => Error::Sqlite(other),
    })
}

pub fn update_story(conn: &Connection, story: &Story) -> Result<()> {
    conn.execute(
        "UPDATE stories SET title = ?1, description = ?2, cover = ?3, genre = ?4, \
                style = ?5, language = ?6, target_audience = ?7, visual_style = ?8, \
                status = ?9, updated_at = ?10 WHERE id = ?11",
        params![
            story.title,
            story.description,
            story.cover,
            story.genre,
            story.style,
            story.language,
            story.target_audience,
            story.visual_style,
            story_status_str(story.status),
            now(),
            story.id.as_str(),
        ],
    )?;
    Ok(())
}

fn story_status_str(s: StoryStatus) -> &'static str {
    match s {
        StoryStatus::Draft => "draft",
        StoryStatus::Active => "active",
        StoryStatus::Completed => "completed",
        StoryStatus::Archived => "archived",
    }
}

fn chapter_status_str(s: ChapterStatus) -> &'static str {
    match s {
        ChapterStatus::Draft => "draft",
        ChapterStatus::Planning => "planning",
        ChapterStatus::Generating => "generating",
        ChapterStatus::Generated => "generated",
        ChapterStatus::Published => "published",
    }
}

// ---------------------------------------------------------------------------
// Story Bible
// ---------------------------------------------------------------------------

pub fn get_bible(conn: &Connection, story_id: &StoryId) -> Result<StoryBible> {
    let row = conn
        .query_row(
            "SELECT story_id, world, rules, timeline, lore, updated_at \
             FROM story_bibles WHERE story_id = ?1",
            params![story_id.as_str()],
            |row| {
                let updated_at: String = row.get("updated_at")?;
                Ok(StoryBible {
                    story_id: StoryId(row.get("story_id")?),
                    world: row.get("world")?,
                    rules: row.get("rules")?,
                    timeline: row.get("timeline")?,
                    lore: row.get("lore")?,
                    updated_at: parse_dt(&updated_at),
                })
            },
        )
        .optional()?;
    row.ok_or(Error::NotFound("story_bible"))
}

pub fn save_bible(conn: &Connection, bible: &StoryBible) -> Result<()> {
    conn.execute(
        "INSERT INTO story_bibles (story_id, world, rules, timeline, lore, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(story_id) DO UPDATE SET \
            world = excluded.world, rules = excluded.rules, \
            timeline = excluded.timeline, lore = excluded.lore, \
            updated_at = excluded.updated_at",
        params![
            bible.story_id.as_str(),
            bible.world,
            bible.rules,
            bible.timeline,
            bible.lore,
            now(),
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Characters
// ---------------------------------------------------------------------------

pub fn add_character(conn: &Connection, character: &Character) -> Result<()> {
    conn.execute(
        "INSERT INTO characters (id, story_id, name, profile, personality, background, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            character.id.as_str(),
            character.story_id.as_str(),
            character.name,
            character.profile,
            character.personality,
            character.background,
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_characters(conn: &Connection, story_id: &StoryId) -> Result<Vec<Character>> {
    let mut stmt = conn.prepare(
        "SELECT id, story_id, name, profile, personality, background, created_at, updated_at \
         FROM characters WHERE story_id = ?1 ORDER BY name",
    )?;
    let rows = stmt.query_map([story_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let updated_at: String = row.get("updated_at")?;
        Ok(Character {
            id: CharacterId(row.get("id")?),
            story_id: StoryId(row.get("story_id")?),
            name: row.get("name")?,
            profile: row.get("profile")?,
            personality: row.get("personality")?,
            background: row.get("background")?,
            created_at: parse_dt(&created_at),
            updated_at: parse_dt(&updated_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_character_age(conn: &Connection, age: &CharacterAge) -> Result<()> {
    conn.execute(
        "INSERT INTO character_ages (id, character_id, age, description, default_reference, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            age.id.as_str(),
            age.character_id.as_str(),
            age.age,
            age.description,
            age.default_reference.as_ref().map(|r| r.as_str()),
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_ages(conn: &Connection, character_id: &CharacterId) -> Result<Vec<CharacterAge>> {
    let mut stmt = conn.prepare(
        "SELECT id, character_id, age, description, default_reference, created_at \
         FROM character_ages WHERE character_id = ?1 ORDER BY age",
    )?;
    let rows = stmt.query_map([character_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let default_reference: Option<String> = row.get("default_reference")?;
        Ok(CharacterAge {
            id: CharacterAgeId(row.get("id")?),
            character_id: CharacterId(row.get("character_id")?),
            age: row.get::<_, u32>("age")?,
            description: row.get("description")?,
            default_reference: default_reference.map(ReferenceImageId),
            created_at: parse_dt(&created_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_character_appearance(conn: &Connection, app: &CharacterAppearance) -> Result<()> {
    conn.execute(
        "INSERT INTO character_appearances (id, age_id, name, description, clothing, hairstyle, \
                                            accessories, emotion, body_state, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            app.id.as_str(),
            app.age_id.as_str(),
            app.name,
            app.description,
            app.clothing,
            app.hairstyle,
            app.accessories,
            app.emotion,
            app.body_state,
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_appearances(conn: &Connection, age_id: &CharacterAgeId) -> Result<Vec<CharacterAppearance>> {
    let mut stmt = conn.prepare(
        "SELECT id, age_id, name, description, clothing, hairstyle, accessories, emotion, body_state, created_at \
         FROM character_appearances WHERE age_id = ?1 ORDER BY name",
    )?;
    let rows = stmt.query_map([age_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        Ok(CharacterAppearance {
            id: CharacterAppearanceId(row.get("id")?),
            age_id: CharacterAgeId(row.get("age_id")?),
            name: row.get("name")?,
            description: row.get("description")?,
            clothing: row.get("clothing")?,
            hairstyle: row.get("hairstyle")?,
            accessories: row.get("accessories")?,
            emotion: row.get("emotion")?,
            body_state: row.get("body_state")?,
            created_at: parse_dt(&created_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

pub fn add_environment(conn: &Connection, env: &Environment) -> Result<()> {
    conn.execute(
        "INSERT INTO environments (id, story_id, name, description, architecture, lighting, weather, \
                                   time_of_day, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            env.id.as_str(),
            env.story_id.as_str(),
            env.name,
            env.description,
            env.architecture,
            env.lighting,
            env.weather,
            env.time_of_day,
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_environments(conn: &Connection, story_id: &StoryId) -> Result<Vec<Environment>> {
    let mut stmt = conn.prepare(
        "SELECT id, story_id, name, description, architecture, lighting, weather, time_of_day, created_at, updated_at \
         FROM environments WHERE story_id = ?1 ORDER BY name",
    )?;
    let rows = stmt.query_map([story_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let updated_at: String = row.get("updated_at")?;
        Ok(Environment {
            id: EnvironmentId(row.get("id")?),
            story_id: StoryId(row.get("story_id")?),
            name: row.get("name")?,
            description: row.get("description")?,
            architecture: row.get("architecture")?,
            lighting: row.get("lighting")?,
            weather: row.get("weather")?,
            time_of_day: row.get("time_of_day")?,
            created_at: parse_dt(&created_at),
            updated_at: parse_dt(&updated_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

pub fn add_prop(conn: &Connection, prop: &Prop) -> Result<()> {
    conn.execute(
        "INSERT INTO props (id, story_id, name, description, appearance, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            prop.id.as_str(),
            prop.story_id.as_str(),
            prop.name,
            prop.description,
            prop.appearance,
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_props(conn: &Connection, story_id: &StoryId) -> Result<Vec<Prop>> {
    let mut stmt = conn.prepare(
        "SELECT id, story_id, name, description, appearance, created_at, updated_at \
         FROM props WHERE story_id = ?1 ORDER BY name",
    )?;
    let rows = stmt.query_map([story_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let updated_at: String = row.get("updated_at")?;
        Ok(Prop {
            id: PropId(row.get("id")?),
            story_id: StoryId(row.get("story_id")?),
            name: row.get("name")?,
            description: row.get("description")?,
            appearance: row.get("appearance")?,
            created_at: parse_dt(&created_at),
            updated_at: parse_dt(&updated_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Relationships
// ---------------------------------------------------------------------------

pub fn add_relationship(conn: &Connection, rel: &Relationship) -> Result<()> {
    let status = match rel.status {
        RelationshipStatus::Active => "active",
        RelationshipStatus::Broken => "broken",
        RelationshipStatus::Ended => "ended",
        RelationshipStatus::Unknown => "unknown",
    };
    conn.execute(
        "INSERT INTO relationships (id, story_id, from_character, to_character, kind, status, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            rel.id.as_str(),
            rel.story_id.as_str(),
            rel.from_character.as_str(),
            rel.to_character.as_str(),
            rel.kind,
            status,
            rel.note,
        ],
    )?;
    Ok(())
}

pub fn list_relationships(conn: &Connection, story_id: &StoryId) -> Result<Vec<Relationship>> {
    let mut stmt = conn.prepare(
        "SELECT id, story_id, from_character, to_character, kind, status, note \
         FROM relationships WHERE story_id = ?1",
    )?;
    let rows = stmt.query_map([story_id.as_str()], |row| {
        let status: String = row.get("status")?;
        Ok(Relationship {
            id: RelationshipId(row.get("id")?),
            story_id: StoryId(row.get("story_id")?),
            from_character: CharacterId(row.get("from_character")?),
            to_character: CharacterId(row.get("to_character")?),
            kind: row.get("kind")?,
            status: parse_rel_status(status),
            note: row.get("note")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Reference images
// ---------------------------------------------------------------------------

pub fn add_reference(conn: &Connection, r: &ReferenceImage, target: ReferenceTarget<'_>) -> Result<()> {
    let kind = match r.kind {
        ReferenceKind::Character => "character",
        ReferenceKind::Environment => "environment",
        ReferenceKind::Prop => "prop",
        ReferenceKind::Shot => "shot",
    };
    conn.execute(
        "INSERT INTO reference_images (id, kind, label, source, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![r.id.as_str(), kind, r.label, r.source, now()],
    )?;
    conn.execute(
        "INSERT INTO reference_links (reference_id, target_kind, target_id) VALUES (?1, ?2, ?3)",
        params![r.id.as_str(), target.kind_str(), target.id()],
    )?;
    Ok(())
}

/// What a reference image hangs off of. Single dispatch — keeps the public
/// API honest about which targets are valid.
pub enum ReferenceTarget<'a> {
    CharacterAge(&'a CharacterAgeId),
    Environment(&'a EnvironmentId),
    Prop(&'a PropId),
    Shot(&'a ShotId),
}

impl ReferenceTarget<'_> {
    fn kind_str(&self) -> &'static str {
        match self {
            ReferenceTarget::CharacterAge(_) => "character_age",
            ReferenceTarget::Environment(_) => "environment",
            ReferenceTarget::Prop(_) => "prop",
            ReferenceTarget::Shot(_) => "shot",
        }
    }
    fn id(&self) -> &str {
        match self {
            ReferenceTarget::CharacterAge(id) => id.as_str(),
            ReferenceTarget::Environment(id) => id.as_str(),
            ReferenceTarget::Prop(id) => id.as_str(),
            ReferenceTarget::Shot(id) => id.as_str(),
        }
    }
}

pub fn list_references(conn: &Connection, kind: &str, target_id: &str) -> Result<Vec<ReferenceImage>> {
    let mut stmt = conn.prepare(
        "SELECT ri.id, ri.kind, ri.label, ri.source, ri.created_at \
         FROM reference_images ri \
         JOIN reference_links rl ON rl.reference_id = ri.id \
         WHERE rl.target_kind = ?1 AND rl.target_id = ?2 \
         ORDER BY ri.created_at",
    )?;
    let rows = stmt.query_map([kind, target_id], |row| {
        let kind: String = row.get("kind")?;
        let created_at: String = row.get("created_at")?;
        Ok(ReferenceImage {
            id: ReferenceImageId(row.get("id")?),
            kind: parse_ref_kind(kind),
            label: row.get("label")?,
            source: row.get("source")?,
            created_at: parse_dt(&created_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// Chapters → Scenes → Shots
// ---------------------------------------------------------------------------

pub fn add_chapter(conn: &Connection, chapter: &Chapter) -> Result<()> {
    conn.execute(
        "INSERT INTO chapters (id, story_id, chapter_number, title, summary, story, timeline, status, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            chapter.id.as_str(),
            chapter.story_id.as_str(),
            chapter.chapter_number,
            chapter.title,
            chapter.summary,
            chapter.story,
            chapter.timeline,
            chapter_status_str(chapter.status),
            now(),
        ],
    )?;
    Ok(())
}

pub fn link_chapter_character(
    conn: &Connection,
    chapter_id: &ChapterId,
    character_id: &CharacterId,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO chapter_characters (chapter_id, character_id) VALUES (?1, ?2)",
        params![chapter_id.as_str(), character_id.as_str()],
    )?;
    Ok(())
}

pub fn link_chapter_environment(
    conn: &Connection,
    chapter_id: &ChapterId,
    environment_id: &EnvironmentId,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO chapter_environments (chapter_id, environment_id) VALUES (?1, ?2)",
        params![chapter_id.as_str(), environment_id.as_str()],
    )?;
    Ok(())
}

pub fn list_chapters(conn: &Connection, story_id: &StoryId) -> Result<Vec<Chapter>> {
    let mut stmt = conn.prepare(
        "SELECT id, story_id, chapter_number, title, summary, story, timeline, status, \
                generated_video, created_at, updated_at \
         FROM chapters WHERE story_id = ?1 ORDER BY chapter_number",
    )?;
    let rows = stmt.query_map([story_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let updated_at: String = row.get("updated_at")?;
        let status: String = row.get("status")?;
        Ok(Chapter {
            id: ChapterId(row.get("id")?),
            story_id: StoryId(row.get("story_id")?),
            chapter_number: row.get::<_, u32>("chapter_number")?,
            title: row.get("title")?,
            summary: row.get("summary")?,
            story: row.get("story")?,
            timeline: row.get("timeline")?,
            status: parse_chapter_status(status),
            generated_video: row.get("generated_video")?,
            created_at: parse_dt(&created_at),
            updated_at: parse_dt(&updated_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_scene(conn: &Connection, scene: &Scene) -> Result<()> {
    conn.execute(
        "INSERT INTO scenes (id, chapter_id, scene_number, title, description, environment_id, emotion, duration_seconds, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            scene.id.as_str(),
            scene.chapter_id.as_str(),
            scene.scene_number,
            scene.title,
            scene.description,
            scene.environment_id.as_ref().map(|e| e.as_str()),
            scene.emotion,
            scene.duration_seconds,
            now(),
        ],
    )?;
    Ok(())
}

pub fn list_scenes(conn: &Connection, chapter_id: &ChapterId) -> Result<Vec<Scene>> {
    let mut stmt = conn.prepare(
        "SELECT id, chapter_id, scene_number, title, description, environment_id, emotion, duration_seconds, created_at \
         FROM scenes WHERE chapter_id = ?1 ORDER BY scene_number",
    )?;
    let rows = stmt.query_map([chapter_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let env_id: Option<String> = row.get("environment_id")?;
        Ok(Scene {
            id: SceneId(row.get("id")?),
            chapter_id: ChapterId(row.get("chapter_id")?),
            scene_number: row.get::<_, u32>("scene_number")?,
            title: row.get("title")?,
            description: row.get("description")?,
            environment_id: env_id.map(EnvironmentId),
            emotion: row.get("emotion")?,
            duration_seconds: row.get::<_, u32>("duration_seconds")?,
            created_at: parse_dt(&created_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn add_shot(conn: &Connection, shot: &Shot) -> Result<()> {
    conn.execute(
        "INSERT INTO shots (id, scene_id, shot_number, description, camera, camera_movement, composition, \
                            action, dialogue, emotion, duration_seconds, environment_id, first_frame, last_frame, \
                            video_model, generation_settings, generated_video, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            shot.id.as_str(),
            shot.scene_id.as_str(),
            shot.shot_number,
            shot.description,
            shot.camera,
            shot.camera_movement,
            shot.composition,
            shot.action,
            shot.dialogue,
            shot.emotion,
            shot.duration_seconds,
            shot.environment_id.as_ref().map(|e| e.as_str()),
            shot.first_frame,
            shot.last_frame,
            shot.video_model,
            shot.generation_settings,
            shot.generated_video,
            now(),
        ],
    )?;
    Ok(())
}

pub fn link_shot_character(conn: &Connection, sc: &ShotCharacter) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO shot_characters (shot_id, character_id, age_id, appearance_id) \
         VALUES (?1, ?2, ?3, ?4)",
        params![
            sc.shot_id.as_str(),
            sc.character_id.as_str(),
            sc.age_id.as_str(),
            sc.appearance_id.as_str(),
        ],
    )?;
    Ok(())
}

pub fn link_shot_prop(conn: &Connection, shot_id: &ShotId, prop_id: &PropId) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO shot_props (shot_id, prop_id) VALUES (?1, ?2)",
        params![shot_id.as_str(), prop_id.as_str()],
    )?;
    Ok(())
}

pub fn list_shots(conn: &Connection, scene_id: &SceneId) -> Result<Vec<Shot>> {
    let mut stmt = conn.prepare(
        "SELECT id, scene_id, shot_number, description, camera, camera_movement, composition, \
                action, dialogue, emotion, duration_seconds, environment_id, first_frame, last_frame, \
                video_model, generation_settings, generated_video, created_at \
         FROM shots WHERE scene_id = ?1 ORDER BY shot_number",
    )?;
    let rows = stmt.query_map([scene_id.as_str()], |row| {
        let created_at: String = row.get("created_at")?;
        let env_id: Option<String> = row.get("environment_id")?;
        Ok(Shot {
            id: ShotId(row.get("id")?),
            scene_id: SceneId(row.get("scene_id")?),
            shot_number: row.get::<_, u32>("shot_number")?,
            description: row.get("description")?,
            camera: row.get("camera")?,
            camera_movement: row.get("camera_movement")?,
            composition: row.get("composition")?,
            action: row.get("action")?,
            dialogue: row.get("dialogue")?,
            emotion: row.get("emotion")?,
            duration_seconds: row.get::<_, u32>("duration_seconds")?,
            environment_id: env_id.map(EnvironmentId),
            first_frame: row.get("first_frame")?,
            last_frame: row.get("last_frame")?,
            video_model: row.get("video_model")?,
            generation_settings: row.get("generation_settings")?,
            generated_video: row.get("generated_video")?,
            created_at: parse_dt(&created_at),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn list_shot_characters(conn: &Connection, shot_id: &ShotId) -> Result<Vec<ShotCharacter>> {
    let mut stmt = conn.prepare(
        "SELECT shot_id, character_id, age_id, appearance_id FROM shot_characters WHERE shot_id = ?1",
    )?;
    let rows = stmt.query_map([shot_id.as_str()], |row| {
        Ok(ShotCharacter {
            shot_id: ShotId(row.get("shot_id")?),
            character_id: CharacterId(row.get("character_id")?),
            age_id: CharacterAgeId(row.get("age_id")?),
            appearance_id: CharacterAppearanceId(row.get("appearance_id")?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// The whole workspace in one read.
// ---------------------------------------------------------------------------

pub fn load_workspace(conn: &Connection, story_id: &StoryId) -> Result<StoryWorkspace> {
    let story = get_story(conn, story_id).ok();
    let bible = get_bible(conn, story_id).ok();
    let characters = list_characters(conn, story_id)?;
    let mut characters_full = Vec::with_capacity(characters.len());
    for c in &characters {
        let ages = list_ages(conn, &c.id)?;
        let mut ages_full = Vec::with_capacity(ages.len());
        for a in &ages {
            let appearances = list_appearances(conn, &a.id)?;
            let references = list_references(conn, "character_age", a.id.as_str())?;
            ages_full.push(CharacterAgeWithAssets {
                age: a.clone(),
                appearances,
                references,
            });
        }
        characters_full.push(CharacterWithAssets {
            character: c.clone(),
            ages: ages_full,
        });
    }

    let envs = list_environments(conn, story_id)?;
    let mut envs_full = Vec::with_capacity(envs.len());
    for e in &envs {
        let refs = list_references(conn, "environment", e.id.as_str())?;
        envs_full.push(EnvironmentWithAssets {
            environment: e.clone(),
            references: refs,
        });
    }

    let props = list_props(conn, story_id)?;
    let mut props_full = Vec::with_capacity(props.len());
    for p in &props {
        let refs = list_references(conn, "prop", p.id.as_str())?;
        props_full.push(PropWithAssets { prop: p.clone(), references: refs });
    }

    let chapters = list_chapters(conn, story_id)?;
    let mut chapters_full = Vec::with_capacity(chapters.len());
    for ch in &chapters {
        let scenes = list_scenes(conn, &ch.id)?;
        let mut scenes_full = Vec::with_capacity(scenes.len());
        for sc in &scenes {
            let shots = list_shots(conn, &sc.id)?;
            let mut shots_full = Vec::with_capacity(shots.len());
            for sh in &shots {
                let shot_chars = list_shot_characters(conn, &sh.id)?;
                let refs = list_references(conn, "shot", sh.id.as_str())?;
                shots_full.push(ShotWithAssets {
                    shot: sh.clone(),
                    characters: shot_chars,
                    props: Vec::new(),
                    references: refs,
                });
            }
            scenes_full.push(SceneWithAssets { scene: sc.clone(), shots: shots_full });
        }
        chapters_full.push(ChapterWithAssets { chapter: ch.clone(), scenes: scenes_full });
    }

    let relationships = list_relationships(conn, story_id)?;
    Ok(StoryWorkspace {
        story,
        bible,
        characters: characters_full,
        environments: envs_full,
        props: props_full,
        chapters: chapters_full,
        relationships,
    })
}

// Touch `params_from_iter` so we keep it in the imports if we ever want a
// dynamic IN (...) clause later. Right now every list_* is a simple one-key
// query, but the workspace's shot-prop resolution will likely use it.
#[allow(dead_code)]
fn _keep_params_from_iter_imported<I>(it: I) -> rusqlite::Result<()>
where
    I: IntoIterator,
    I::Item: rusqlite::ToSql,
{
    let _ = params_from_iter(it);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    #[test]
    fn schema_round_trip() {
        let conn = open_memory().unwrap();
        let story = create_story(&conn, "末日之城").unwrap();
        assert_eq!(story.title, "末日之城");
        let bible = get_bible(&conn, &story.id).unwrap();
        assert_eq!(bible.world, "");
    }

    #[test]
    fn character_age_appearance_chain_persists() {
        let conn = open_memory().unwrap();
        let story = create_story(&conn, "末日之城").unwrap();

        let c = Character {
            id: CharacterId::new(),
            story_id: story.id.clone(),
            name: "林默".into(),
            profile: "28 岁前 AI 工程师".into(),
            personality: "冷静、理性".into(),
            background: "曾参与秘密 AI 项目".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        add_character(&conn, &c).unwrap();

        let age = CharacterAge {
            id: CharacterAgeId::new(),
            character_id: c.id.clone(),
            age: 28,
            description: "成年".into(),
            default_reference: None,
            created_at: chrono::Utc::now(),
        };
        add_character_age(&conn, &age).unwrap();

        let app = CharacterAppearance {
            id: CharacterAppearanceId::new(),
            age_id: age.id.clone(),
            name: "战斗服".into(),
            description: "黑色战术外套".into(),
            clothing: "战术外套".into(),
            hairstyle: "短发".into(),
            accessories: "手枪".into(),
            emotion: "警惕".into(),
            body_state: "轻微受伤".into(),
            created_at: chrono::Utc::now(),
        };
        add_character_appearance(&conn, &app).unwrap();

        let ages = list_ages(&conn, &c.id).unwrap();
        assert_eq!(ages.len(), 1);
        let apps = list_appearances(&conn, &ages[0].id).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "战斗服");
    }

    #[test]
    fn chapter_to_shot_chain_persists_with_join_rows() {
        let conn = open_memory().unwrap();
        let story = create_story(&conn, "末日之城").unwrap();
        let env = Environment {
            id: EnvironmentId::new(),
            story_id: story.id.clone(),
            name: "AI 实验室".into(),
            description: "冰冷的服务器机房".into(),
            architecture: "金属风".into(),
            lighting: "蓝色背光".into(),
            weather: String::new(),
            time_of_day: "夜".into(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        add_environment(&conn, &env).unwrap();

        let chapter = Chapter {
            id: ChapterId::new(),
            story_id: story.id.clone(),
            chapter_number: 1,
            title: "灾难开始".into(),
            summary: "林默发现能源系统异常".into(),
            story: "晚上 10:30 ...".into(),
            timeline: "2057-03-01".into(),
            status: ChapterStatus::Draft,
            generated_video: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        add_chapter(&conn, &chapter).unwrap();
        link_chapter_environment(&conn, &chapter.id, &env.id).unwrap();

        let scene = Scene {
            id: SceneId::new(),
            chapter_id: chapter.id.clone(),
            scene_number: 1,
            title: "实验室".into(),
            description: "林默发现能源系统出现异常".into(),
            environment_id: Some(env.id.clone()),
            emotion: "紧张".into(),
            duration_seconds: 45,
            created_at: chrono::Utc::now(),
        };
        add_scene(&conn, &scene).unwrap();

        let shot = Shot {
            id: ShotId::new(),
            scene_id: scene.id.clone(),
            shot_number: 1,
            description: "实验室全景".into(),
            camera: "Wide".into(),
            camera_movement: "Static".into(),
            composition: "Center".into(),
            action: "服务器机柜亮起警示灯".into(),
            dialogue: String::new(),
            emotion: "紧张".into(),
            duration_seconds: 5,
            environment_id: Some(env.id.clone()),
            first_frame: None,
            last_frame: None,
            video_model: None,
            generation_settings: String::new(),
            generated_video: None,
            created_at: chrono::Utc::now(),
        };
        add_shot(&conn, &shot).unwrap();

        let ws = load_workspace(&conn, &story.id).unwrap();
        assert_eq!(ws.chapters.len(), 1);
        assert_eq!(ws.chapters[0].scenes.len(), 1);
        assert_eq!(ws.chapters[0].scenes[0].shots.len(), 1);
        assert_eq!(ws.environments.len(), 1);
    }
}
