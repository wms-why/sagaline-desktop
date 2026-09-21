//! `SceneRepo::create` is the multi-table mutation the discussion in
//! `AGENTS.md` calls out: writing a Scene, the chapter it sits in,
//! and the character / environment assignments all happen in one
//! transaction. Phase 2's `create_scene` Tool will sit on top of this.

use rusqlite::{OptionalExtension as _, Row};
use uuid::Uuid;

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

#[derive(Debug, Clone)]
pub struct NewChapter<'a> {
    pub story_id: &'a str,
    pub slug: &'a str,
    pub ordinal: i64,
    pub title: &'a str,
    pub synopsis: &'a str,
}

#[derive(Debug, Clone)]
pub struct NewScene<'a> {
    pub chapter_id: &'a str,
    pub slug: &'a str,
    pub ordinal: i64,
    pub title: &'a str,
    pub synopsis: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChapterRow {
    pub id: String,
    pub story_id: String,
    pub slug: String,
    pub ordinal: i64,
    pub title: String,
    pub synopsis: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SceneRow {
    pub id: String,
    pub chapter_id: String,
    pub slug: String,
    pub ordinal: i64,
    pub title: String,
    pub synopsis: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ChapterRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            story_id: row.get("story_id")?,
            slug: row.get("slug")?,
            ordinal: row.get("ordinal")?,
            title: row.get("title")?,
            synopsis: row.get("synopsis")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

impl SceneRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            chapter_id: row.get("chapter_id")?,
            slug: row.get("slug")?,
            ordinal: row.get("ordinal")?,
            title: row.get("title")?,
            synopsis: row.get("synopsis")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

pub struct SceneRepo<'w> {
    world: &'w World,
}

impl<'w> SceneRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    pub fn new_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    pub fn create_chapter(&self, new: NewChapter<'_>) -> Result<ChapterRow, StoreError> {
        let conn = self.world.conn()?;
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        conn.execute(
            "INSERT INTO chapters
                (id, story_id, slug, ordinal, title, synopsis, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                id,
                new.story_id,
                new.slug,
                new.ordinal,
                new.title,
                new.synopsis,
                now
            ],
        )?;
        Ok(ChapterRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            ordinal: new.ordinal,
            title: new.title.into(),
            synopsis: new.synopsis.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }


    /// Same as [`Self::create_chapter`], but inside a
    /// caller-supplied transaction. Phase 5's `approve_proposal`
    /// uses this so chapter writes can join the outer tx.
    pub fn create_chapter_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewChapter<'_>,
    ) -> Result<ChapterRow, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        tx.execute(
            "INSERT INTO chapters
                (id, story_id, slug, ordinal, title, synopsis, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                id,
                new.story_id,
                new.slug,
                new.ordinal,
                new.title,
                new.synopsis,
                now
            ],
        )?;
        Ok(ChapterRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            ordinal: new.ordinal,
            title: new.title.into(),
            synopsis: new.synopsis.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn create_scene(&self, new: NewScene<'_>) -> Result<SceneRow, StoreError> {
        let mut conn = self.world.conn()?;
        let tx = conn.transaction()?;
        let row = self.create_scene_in_tx(&tx, new)?;
        tx.commit()?;
        Ok(row)
    }

    /// Create inside a caller-supplied transaction. Phase 2's
    /// `create_scene` Tool will use this so it can write scene +
    /// scene_characters + scene_environments in one go.
    pub fn create_scene_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewScene<'_>,
    ) -> Result<SceneRow, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        tx.execute(
            "INSERT INTO scenes
                (id, chapter_id, slug, ordinal, title, synopsis, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                id,
                new.chapter_id,
                new.slug,
                new.ordinal,
                new.title,
                new.synopsis,
                now
            ],
        )?;
        Ok(SceneRow {
            id,
            chapter_id: new.chapter_id.into(),
            slug: new.slug.into(),
            ordinal: new.ordinal,
            title: new.title.into(),
            synopsis: new.synopsis.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn get_scene(&self, id: &str) -> Result<Option<SceneRow>, StoreError> {
        let conn = self.world.conn()?;
        let row = conn
            .query_row(
                "SELECT id, chapter_id, slug, ordinal, title, synopsis, created_at, updated_at
                 FROM scenes WHERE id = ?1",
                [id],
                SceneRow::from_row,
            )
            .optional()?;
        Ok(row)
    }
    /// All chapters in story order (oldest first).
    pub fn list_chapters_for_story(
        &self,
        story_id: &str,
    ) -> Result<Vec<ChapterRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, story_id, slug, ordinal, title, synopsis, created_at, updated_at
             FROM chapters
             WHERE story_id = ?1
             ORDER BY ordinal ASC, created_at ASC",
        )?;
        let rows = stmt
            .query_map([story_id], ChapterRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// All scenes that belong to any chapter in `story_id`, in
    /// (chapter ordinal, scene ordinal) order. The Phase 2.5
    /// agent loop drives off this.
    pub fn list_scenes_for_story(
        &self,
        story_id: &str,
    ) -> Result<Vec<SceneRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.chapter_id, s.slug, s.ordinal,
                    s.title, s.synopsis, s.created_at, s.updated_at
             FROM scenes s
             JOIN chapters c ON c.id = s.chapter_id
             WHERE c.story_id = ?1
             ORDER BY c.ordinal ASC, s.ordinal ASC, s.created_at ASC",
        )?;
        let rows = stmt
            .query_map([story_id], SceneRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sanity-check helper used by integration tests.
    #[doc(hidden)]
    pub fn _count(&self) -> Result<i64, StoreError> {
        let conn = self.world.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM scenes", [], |row| row.get(0))?;
        Ok(n)
    }
    /// Character IDs assigned to a scene (via `scene_characters`).
    /// Phase 2.5's agent loop uses this to populate the OBSERVE
    /// event's `characters` list without re-parsing frontmatter.
    pub fn character_ids_for_scene(
        &self,
        scene_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT character_id FROM scene_characters
             WHERE scene_id = ?1
             ORDER BY character_id ASC",
        )?;
        let rows = stmt
            .query_map([scene_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Environment IDs assigned to a scene. Phase 2.5 uses this
    /// for the OBSERVE event.
    pub fn environment_ids_for_scene(
        &self,
        scene_id: &str,
    ) -> Result<Vec<String>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT environment_id FROM scene_environments
             WHERE scene_id = ?1
             ORDER BY environment_id ASC",
        )?;
        let rows = stmt
            .query_map([scene_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}


