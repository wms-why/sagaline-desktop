//! Connection pool over r2d2 + rusqlite. Synchronous on purpose: every
//! call goes through `sagaline-bridge`'s `spawn_blocking` so callers
//! from the GPUI side never block the GPUI scheduler.

use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;

use crate::error::StoreError;

/// Thread-safe SQLite pool. Clone-able; `PooledConn` checks out one
/// connection for a unit of work and returns it on drop.
pub type Pool = r2d2::Pool<SqliteConnectionManager>;
pub type PooledConn = r2d2::PooledConnection<SqliteConnectionManager>;

/// Pragmas we run on every new connection. WAL gives concurrent
/// readers + one writer; foreign keys are off by default in SQLite.
/// `synchronous=NORMAL` is the WAL sweet spot; `busy_timeout` keeps
/// short contention from erroring out instead of waiting.
pub const CONNECTION_PRAGMAS: &[&str] = &[
    "PRAGMA journal_mode = WAL",
    "PRAGMA synchronous = NORMAL",
    "PRAGMA foreign_keys = ON",
    "PRAGMA busy_timeout = 5000",
];

fn apply_pragmas(c: &mut Connection) -> rusqlite::Result<()> {
    for pragma in CONNECTION_PRAGMAS {
        c.execute_batch(pragma)?;
    }
    Ok(())
}

/// Open a pool against the given DB file. Pool size is intentionally
/// modest (8): one Tokio bridge thread + a handful of GPUI background
/// tasks is the realistic peak concurrency for a desktop app.
pub fn open_pool(db_path: &std::path::Path) -> Result<Pool, StoreError> {
    let mgr = SqliteConnectionManager::file(db_path).with_init(apply_pragmas);
    let pool = r2d2::Pool::builder()
        .max_size(8)
        .build(mgr)
        .map_err(StoreError::from)?;
    Ok(pool)
}

/// One in-memory pool for tests. Each pool owns its own database.
pub fn in_memory_pool() -> Result<Pool, StoreError> {
    let mgr = SqliteConnectionManager::memory().with_init(apply_pragmas);
    let pool = r2d2::Pool::builder()
        .max_size(4)
        .build(mgr)
        .map_err(StoreError::from)?;
    Ok(pool)
}
