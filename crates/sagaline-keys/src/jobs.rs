//! Generation job store. Borrows the [`SagalineStore`]'s redb
//! handle; both tables live in the same `~/.sageline/data/keys.db`
//! file under one `redb::Database`.
//!
//! See [`SagalineStore`] for the unified entry point.

use std::path::Path;

use redb::ReadableTable as _;
use serde::{Deserialize, Serialize};

use crate::error::KeyError;
use crate::store::{JOB_TABLE, SagalineStore};

/// One generation job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Caller-supplied id (e.g. UUID or `shot_004_keyframe`).
    pub job_id: String,
    /// Shot entity id this job belongs to (e.g. `shot_004`).
    pub shot_id: String,
    /// `Capability::as_str()`: `"image"`, `"tts"`, `"image_to_video"`.
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
    pub asset_path: Option<std::path::PathBuf>,
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

/// Typed view over the job column of a [`SagalineStore`].
pub struct JobStore<'a> {
    pub(crate) store: &'a SagalineStore,
}

impl<'a> JobStore<'a> {
    pub fn db_path(&self) -> &Path {
        self.store.db_path()
    }

    /// Insert or replace a job by id.
    pub fn put(&self, job: &Job) -> Result<(), KeyError> {
        let json = serde_json::to_string(job)
            .map_err(|e| KeyError::Other(format!("job serialize: {e}")))?;
        let txn = self.store.db().begin_write().map_err(|e| KeyError::Database(e.into()))?;
        {
            let mut table = txn.open_table(JOB_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            table
                .insert(job.job_id.as_str(), json.as_str())
                .map_err(|e| KeyError::Database(e.into()))?;
        }
        txn.commit().map_err(|e| KeyError::Database(e.into()))?;
        Ok(())
    }

    /// Load a job by id. Returns [`KeyError::NotFound`] if missing.
    pub fn get(&self, job_id: &str) -> Result<Job, KeyError> {
        let json = {
            let txn = self.store.db().begin_read().map_err(|e| KeyError::Database(e.into()))?;
            let table = txn.open_table(JOB_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            let guard = table
                .get(job_id)
                .map_err(|e| KeyError::Database(e.into()))?
                .ok_or_else(|| KeyError::NotFound {
                    provider: "jobs".into(),
                    key_id: job_id.into(),
                })?;
            guard.value().to_string()
        };
        serde_json::from_str(&json)
            .map_err(|e| KeyError::Other(format!("job parse: {e}")))
    }

    /// Update an existing job. The full record is rewritten (no
    /// patch-by-field semantics — the agent always knows the full
    /// state).
    pub fn update<F>(&self, job_id: &str, mutate: F) -> Result<Job, KeyError>
    where
        F: FnOnce(&mut Job),
    {
        let mut job = self.get(job_id)?;
        mutate(&mut job);
        self.put(&job)?;
        Ok(job)
    }

    /// List jobs in a given status, oldest first.
    pub fn list_by_status(&self, status: JobStatus) -> Result<Vec<Job>, KeyError> {
        let txn = self.store.db().begin_read().map_err(|e| KeyError::Database(e.into()))?;
        let table = txn.open_table(JOB_TABLE).map_err(|e| KeyError::Database(e.into()))?;
        let mut out = Vec::new();
        for entry in table.iter().map_err(|e| KeyError::Database(e.into()))? {
            let (_, v) = entry.map_err(|e| KeyError::Database(e.into()))?;
            let json = v.value();
            if let Ok(job) = serde_json::from_str::<Job>(json) {
                if job.status == status {
                    out.push(job);
                }
            }
        }
        out.sort_by(|a, b| a.job_id.cmp(&b.job_id));
        Ok(out)
    }

    /// Convenience: list jobs that need resume on startup (queued or
    /// running).
    pub fn list_pending(&self) -> Result<Vec<Job>, KeyError> {
        let mut all = self.list_by_status(JobStatus::Queued)?;
        all.extend(self.list_by_status(JobStatus::Running)?);
        Ok(all)
    }

    /// Delete a job by id. Returns `true` if a row was deleted.
    pub fn delete(&self, job_id: &str) -> Result<bool, KeyError> {
        let txn = self.store.db().begin_write().map_err(|e| KeyError::Database(e.into()))?;
        let removed = {
            let mut table = txn.open_table(JOB_TABLE).map_err(|e| KeyError::Database(e.into()))?;
            let result = table
                .remove(job_id)
                .map_err(|e| KeyError::Database(e.into()))?;
            result.is_some()
        };
        txn.commit().map_err(|e| KeyError::Database(e.into()))?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// `KeyStore<'a>` borrows from `SagalineStore`; the test holds
    /// the store alive for the test's lifetime and binds both views
    /// in parallel.
    fn fresh() -> (tempfile::TempDir, JobStore<'static>) {
        // Leak the store so we can hand out a 'static view. The
        // tempdir cleans up the directory when dropped; the leaked
        // store is reclaimed at process exit.
        let dir = tempdir().unwrap();
        let store: &'static SagalineStore =
            Box::leak(Box::new(SagalineStore::open(dir.path()).unwrap()));
        (dir, store.jobs())
    }

    fn sample(id: &str) -> Job {
        Job {
            job_id: id.into(),
            shot_id: "shot_004".into(),
            capability: "image".into(),
            provider: "minimax".into(),
            model_id: "image-01".into(),
            provider_task_id: None,
            status: JobStatus::Queued,
            started_at: None,
            finished_at: None,
            asset_path: None,
            attempt: 1,
            error: None,
        }
    }

    #[test]
    fn put_get_round_trip() {
        let (_d, s) = fresh();
        s.put(&sample("job_a")).unwrap();
        let got = s.get("job_a").unwrap();
        assert_eq!(got.provider, "minimax");
        assert_eq!(got.status, JobStatus::Queued);
    }

    #[test]
    fn update_patches_status() {
        let (_d, s) = fresh();
        s.put(&sample("job_b")).unwrap();
        let updated = s
            .update("job_b", |j| {
                j.status = JobStatus::Running;
                j.started_at = Some("2026-09-16T00:00:00Z".into());
            })
            .unwrap();
        assert_eq!(updated.status, JobStatus::Running);
        assert_eq!(updated.started_at.as_deref(), Some("2026-09-16T00:00:00Z"));
    }

    #[test]
    fn list_by_status_and_pending() {
        let (_d, s) = fresh();
        s.put(&sample("q1")).unwrap();
        s.put(&sample("q2")).unwrap();
        s.update("q2", |j| j.status = JobStatus::Running).unwrap();
        s.put(&{
            let mut j = sample("done");
            j.status = JobStatus::Succeeded;
            j
        })
        .unwrap();

        assert_eq!(s.list_by_status(JobStatus::Queued).unwrap().len(), 1);
        assert_eq!(s.list_by_status(JobStatus::Running).unwrap().len(), 1);
        assert_eq!(s.list_pending().unwrap().len(), 2);
        assert_eq!(s.list_by_status(JobStatus::Succeeded).unwrap().len(), 1);
    }

    #[test]
    fn get_missing_is_not_found() {
        let (_d, s) = fresh();
        let err = s.get("nope").unwrap_err();
        assert!(matches!(err, KeyError::NotFound { .. }));
    }

    #[test]
    fn delete_removes() {
        let (_d, s) = fresh();
        s.put(&sample("x")).unwrap();
        assert!(s.delete("x").unwrap());
        assert!(matches!(s.get("x").unwrap_err(), KeyError::NotFound { .. }));
    }

    #[test]
    fn job_status_is_terminal() {
        assert!(!JobStatus::Queued.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Succeeded.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
    }

    #[test]
    fn jobs_db_path_is_keys_db() {
        let dir = tempdir().unwrap();
        let store = SagalineStore::open(dir.path()).unwrap();
        let jobs = store.jobs();
        assert!(jobs.db_path().ends_with("keys.db"));
    }
}
