use rusqlite::{OptionalExtension as _, Row};
use uuid::Uuid;

use crate::error::StoreError;
use crate::pool::PooledConn;
use crate::time_util::now_iso;
use crate::world::World;

/// What the UI / agent supplies when creating a [`StoryRow`].
#[derive(Debug, Clone)]
pub struct NewStory<'a> {
    pub slug: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
}

/// One row of the `stories` table. Stable id is a UUID v7 string.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoryRow {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub created_at: String,
    pub updated_at: String,
}

impl StoryRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            slug: row.get("slug")?,
            title: row.get("title")?,
            summary: row.get("summary")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }
}

pub struct StoryRepo<'w> {
    world: &'w World,
}

impl<'w> StoryRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    /// Mint a fresh UUID v7.
    pub fn new_id(&self) -> String {
        Uuid::now_v7().to_string()
    }

    /// Current UTC timestamp, second precision. Mirrors the format
    /// the project's wire format for `Job::started_at` /
    /// `Job::finished_at`.
    pub fn now(&self) -> String {
        now_iso()
    }

    /// Open a connection from the pool.
    pub fn conn(&self) -> Result<PooledConn, StoreError> {
        self.world.conn()
    }

    pub fn create(&self, new: NewStory<'_>) -> Result<StoryRow, StoreError> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let row = self.create_in_tx(&tx, new)?;
        tx.commit()?;
        Ok(row)
    }

    /// Create inside a caller-supplied transaction so multi-table
    /// mutations (Phase 2's `create_scene`) can share the boundary.
    pub fn create_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        new: NewStory<'_>,
    ) -> Result<StoryRow, StoreError> {
        if new.slug.is_empty() {
            return Err(StoreError::Other("story slug cannot be empty".into()));
        }
        let id = Uuid::now_v7().to_string();
        let now = now_iso();
        tx.execute(
            "INSERT INTO stories (id, slug, title, summary, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![id, new.slug, new.title, new.summary, now],
        )?;
        Ok(StoryRow {
            id,
            slug: new.slug.into(),
            title: new.title.into(),
            summary: new.summary.into(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<StoryRow>, StoreError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, slug, title, summary, created_at, updated_at
                 FROM stories WHERE id = ?1",
                [id],
                StoryRow::from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn get_by_slug(&self, slug: &str) -> Result<Option<StoryRow>, StoreError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, slug, title, summary, created_at, updated_at
                 FROM stories WHERE slug = ?1",
                [slug],
                StoryRow::from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list(&self) -> Result<Vec<StoryRow>, StoreError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, slug, title, summary, created_at, updated_at
             FROM stories ORDER BY created_at ASC",
        )?;
        let rows = stmt
            .query_map([], StoryRow::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn delete(&self, id: &str) -> Result<bool, StoreError> {
        let conn = self.conn()?;
        let removed = conn.execute("DELETE FROM stories WHERE id = ?1", [id])?;
        Ok(removed > 0)
    }

    /// Sanity-check helper used by integration tests.
    #[doc(hidden)]
    pub fn _count(&self) -> Result<i64, StoreError> {
        let conn = self.conn()?;
        let n: i64 =
            conn.query_row("SELECT COUNT(*) FROM stories", [], |row| row.get(0))?;
        Ok(n)
    }
}
