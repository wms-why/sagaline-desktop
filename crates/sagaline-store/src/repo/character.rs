use rusqlite::{OptionalExtension as _, Row};
use uuid::Uuid;

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

#[derive(Debug, Clone)]
pub struct NewCharacter<'a> {
    pub story_id: &'a str,
    pub slug: &'a str,
    pub name: &'a str,
    pub occupation: Option<&'a str>,
    pub bio: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CharacterRow {
    pub id: String,
    pub story_id: String,
    pub slug: String,
    pub name: String,
    pub occupation: Option<String>,
    pub bio: String,
    pub created_at: String,
    pub updated_at: String,
}

impl CharacterRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            story_id: row.get("story_id")?,
            slug: row.get("slug")?,
            name: row.get("name")?,
            occupation: row.get("occupation")?,
            bio: row.get("bio")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

pub struct CharacterRepo<'w> {
    world: &'w World,
}

impl<'w> CharacterRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    pub fn new_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    pub fn create(&self, new: NewCharacter<'_>) -> Result<CharacterRow, StoreError> {
        let conn = self.world.conn()?;
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        conn.execute(
            "INSERT INTO characters
                (id, story_id, slug, name, occupation, bio, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                id,
                new.story_id,
                new.slug,
                new.name,
                new.occupation,
                new.bio,
                now
            ],
        )?;
        Ok(CharacterRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            name: new.name.into(),
            occupation: new.occupation.map(str::to_owned),
            bio: new.bio.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }


    /// Same as [`Self::create`], but inside a caller-supplied
    /// transaction. Phase 5's `approve_proposal` uses this so a
    /// multi-action proposal commits (or rolls back) atomically.
    pub fn create_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewCharacter<'_>,
    ) -> Result<CharacterRow, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        tx.execute(
            "INSERT INTO characters
                (id, story_id, slug, name, occupation, bio, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                id,
                new.story_id,
                new.slug,
                new.name,
                new.occupation,
                new.bio,
                now
            ],
        )?;
        Ok(CharacterRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            name: new.name.into(),
            occupation: new.occupation.map(str::to_owned),
            bio: new.bio.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<CharacterRow>, StoreError> {
        let conn = self.world.conn()?;
        let row = conn
            .query_row(
                "SELECT id, story_id, slug, name, occupation, bio, created_at, updated_at
                 FROM characters WHERE id = ?1",
                [id],
                CharacterRow::from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_for_story(&self, story_id: &str) -> Result<Vec<CharacterRow>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, story_id, slug, name, occupation, bio, created_at, updated_at
             FROM characters WHERE story_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([story_id], CharacterRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Sanity-check helper used by integration tests.
    #[doc(hidden)]
    pub fn _count(&self) -> Result<i64, StoreError> {
        let conn = self.world.conn()?;
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM characters", [], |row| row.get(0))?;
        Ok(n)
    }


}
