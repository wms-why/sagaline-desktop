use rusqlite::{OptionalExtension as _, Row};
use uuid::Uuid;

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

#[derive(Debug, Clone)]
pub struct NewProp<'a> {
    pub story_id: &'a str,
    pub slug: &'a str,
    pub name: &'a str,
    pub description: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PropRow {
    pub id: String,
    pub story_id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub created_at: String,
    pub updated_at: String,
}

impl PropRow {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            story_id: row.get("story_id")?,
            slug: row.get("slug")?,
            name: row.get("name")?,
            description: row.get("description")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

pub struct PropRepo<'w> {
    world: &'w World,
}

impl<'w> PropRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    pub fn new_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    pub fn create(&self, new: NewProp<'_>) -> Result<PropRow, StoreError> {
        let conn = self.world.conn()?;
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        conn.execute(
            "INSERT INTO props (id, story_id, slug, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![id, new.story_id, new.slug, new.name, new.description, now],
        )?;
        Ok(PropRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            name: new.name.into(),
            description: new.description.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// Same as [`Self::create`], but inside a caller-supplied
    /// transaction. Phase 5's `approve_proposal` uses this so
    /// prop writes can join the outer tx.
    pub fn create_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewProp<'_>,
    ) -> Result<PropRow, StoreError> {
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        tx.execute(
            "INSERT INTO props
                (id, story_id, slug, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                id,
                new.story_id,
                new.slug,
                new.name,
                new.description,
                now,
            ],
        )?;
        Ok(PropRow {
            id,
            story_id: new.story_id.into(),
            slug: new.slug.into(),
            name: new.name.into(),
            description: new.description.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }
}
