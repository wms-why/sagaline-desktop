-- provider_key + jobs — BYOK + generation-job rows that used to
-- live in the redb `keys.db` file (which is deleted on first open).
-- Both tables are populated by the UI / agent code going forward;
-- no migration script reads the old redb file.
--
-- ciphertext is age-encrypted with the per-machine X25519 identity at
-- `data_dir/identity.age` (see sagaline-store/src/identity.rs).

CREATE TABLE provider_key (
    provider        TEXT NOT NULL,
    key_id          TEXT NOT NULL,
    ciphertext      BLOB NOT NULL,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (provider, key_id)
);

CREATE TABLE jobs (
    job_id              TEXT PRIMARY KEY,
    shot_id             TEXT NOT NULL,
    capability          TEXT NOT NULL,
    provider            TEXT NOT NULL,
    model_id            TEXT NOT NULL,
    provider_task_id    TEXT,
    status              TEXT NOT NULL CHECK (status IN
                            ('queued','running','succeeded','failed','cancelled')),
    started_at          TEXT,
    finished_at         TEXT,
    asset_path          TEXT,
    attempt             INTEGER NOT NULL DEFAULT 1,
    error               TEXT
);
CREATE INDEX jobs_status_idx ON jobs(status);
CREATE INDEX jobs_shot_idx ON jobs(shot_id);
