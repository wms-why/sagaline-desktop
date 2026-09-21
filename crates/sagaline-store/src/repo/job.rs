//! `JobRepo` — generation job records. Identical shape to the old
//! the project's generation-job record, now backed by the
//! `jobs` SQLite table (V008).

use std::path::PathBuf;

use rusqlite::OptionalExtension as _;
use serde::{Deserialize, Serialize};

use crate::error::StoreError;
use crate::time_util::now_iso;
use crate::world::World;

/// One generation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Caller-supplied id (e.g. UUID or `shot_004_keyframe`).
    pub job_id: String,
    /// Shot entity id this job belongs to (e.g. `shot_004`).
    pub shot_id: String,
    /// Capability key: `"image"`, `"tts"`, `"image_to_video"`.
    pub capability: String,
    /// Logical provider name (e.g. `"minimax"`).
    pub provider: String,
    /// Model id.
    pub model_id: String,
    /// Provider's own task id (filled in after submit).
    #[serde(default)]
    pub provider_task_id: Option<String>,
    /// Current state.
    pub status: JobStatus,
    /// ISO-8601 timestamp (seconds precision). `None` if not yet started.
    #[serde(default)]
    pub started_at: Option<String>,
    /// ISO-8601 timestamp. `None` if not yet finished.
    #[serde(default)]
    pub finished_at: Option<String>,
    /// Where the asset will land on disk (relative to the story root).
    #[serde(default)]
    pub asset_path: Option<PathBuf>,
    /// 1-based attempt count; bumped on retry.
    #[serde(default = "default_attempt")]
    pub attempt: u32,
    /// Free-form error reason if `status == failed`.
    #[serde(default)]
    pub error: Option<String>,
}

fn default_attempt() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

pub struct JobRepo<'w> {
    world: &'w World,
}

impl<'w> JobRepo<'w> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self { world }
    }

    pub fn put(&self, job: &Job) -> Result<(), StoreError> {
        let conn = self.world.conn()?;
        conn.execute(
            "INSERT INTO jobs
                (job_id, shot_id, capability, provider, model_id, provider_task_id,
                 status, started_at, finished_at, asset_path, attempt, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(job_id) DO UPDATE SET
                 shot_id          = excluded.shot_id,
                 capability       = excluded.capability,
                 provider         = excluded.provider,
                 model_id         = excluded.model_id,
                 provider_task_id = excluded.provider_task_id,
                 status           = excluded.status,
                 started_at       = excluded.started_at,
                 finished_at      = excluded.finished_at,
                 asset_path       = excluded.asset_path,
                 attempt          = excluded.attempt,
                 error            = excluded.error",
            rusqlite::params![
                job.job_id,
                job.shot_id,
                job.capability,
                job.provider,
                job.model_id,
                job.provider_task_id,
                job.status.as_str(),
                job.started_at,
                job.finished_at,
                job.asset_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                job.attempt,
                job.error,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, job_id: &str) -> Result<Job, StoreError> {
        let conn = self.world.conn()?;
        let row = conn
            .query_row(
                "SELECT job_id, shot_id, capability, provider, model_id,
                        provider_task_id, status, started_at, finished_at,
                        asset_path, attempt, error
                 FROM jobs WHERE job_id = ?1",
                [job_id],
                Self::from_row,
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound {
                provider: "jobs".into(),
                key_id: job_id.into(),
            })?;
        Ok(row)
    }

    pub fn update<F>(&self, job_id: &str, mutate: F) -> Result<Job, StoreError>
    where
        F: FnOnce(&mut Job),
    {
        let mut job = self.get(job_id)?;
        mutate(&mut job);
        self.put(&job)?;
        Ok(job)
    }

    pub fn list_by_status(&self, status: JobStatus) -> Result<Vec<Job>, StoreError> {
        let conn = self.world.conn()?;
        let mut stmt = conn.prepare(
            "SELECT job_id, shot_id, capability, provider, model_id,
                    provider_task_id, status, started_at, finished_at,
                    asset_path, attempt, error
             FROM jobs WHERE status = ?1 ORDER BY job_id",
        )?;
        let rows = stmt
            .query_map([status.as_str()], Self::from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_pending(&self) -> Result<Vec<Job>, StoreError> {
        let mut all = self.list_by_status(JobStatus::Queued)?;
        all.extend(self.list_by_status(JobStatus::Running)?);
        Ok(all)
    }

    pub fn delete(&self, job_id: &str) -> Result<bool, StoreError> {
        let conn = self.world.conn()?;
        let removed = conn.execute("DELETE FROM jobs WHERE job_id = ?1", [job_id])?;
        Ok(removed > 0)
    }

    /// Touch `updated_at`-like bookkeeping for the job row. Currently
    /// a no-op (the jobs table has no `updated_at`); kept so callers
    /// can express intent. Real updates go through [`Self::update`].
    pub fn touch(&self, _job_id: &str) -> Result<(), StoreError> {
        let _ = now_iso(); // reserved for future bookkeeping columns
        Ok(())
    }

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
        let status_str: String = row.get("status")?;
        let status = match status_str.as_str() {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "cancelled" => JobStatus::Cancelled,
            other => {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::<dyn std::error::Error + Send + Sync>::from(format!("unknown JobStatus `{other}`")),
                ));
            }
        };
        let asset_path: Option<String> = row.get("asset_path")?;
        Ok(Job {
            job_id: row.get("job_id")?,
            shot_id: row.get("shot_id")?,
            capability: row.get("capability")?,
            provider: row.get("provider")?,
            model_id: row.get("model_id")?,
            provider_task_id: row.get("provider_task_id")?,
            status,
            started_at: row.get("started_at")?,
            finished_at: row.get("finished_at")?,
            asset_path: asset_path.map(PathBuf::from),
            attempt: row.get("attempt")?,
            error: row.get("error")?,
        })
    }
}
