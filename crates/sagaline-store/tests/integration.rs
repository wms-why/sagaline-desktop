//! Integration tests for the world store. Cover:
//! - migration runner applies all 8 migrations on first open
//! - persistent across open/close cycles
//! - FK constraints enforced
//! - slug uniqueness enforced
//! - multi-table transaction via `SceneRepo::create_scene_in_tx`
//! - connection pool concurrency

use sagaline_store::repo::{NewCharacter, NewScene, NewStory};
use sagaline_store::StoreError;
use sagaline_store::World;
use secrecy::ExposeSecret as _;

fn fresh() -> World {
    World::in_memory().expect("in-memory world")
}

#[test]
fn all_eight_migrations_applied() {
    let w = fresh();
    let conn = w.conn().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refinery_schema_history",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 8, "expected 8 migrations, found {n}");
}

#[test]
fn story_round_trip_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();

    // First open: create a story.
    {
        let w = World::open_at(&dir.path().join("world.db")).unwrap();
        let row = w
            .stories()
            .create(NewStory {
                slug: "lin-mo",
                title: "Lin Mo",
                summary: "rough around the edges",
            })
            .unwrap();
        assert_eq!(row.slug, "lin-mo");
        assert_eq!(w.stories()._count().unwrap(), 1);
    }

    // Second open: the story is still there.
    {
        let w = World::open_at(&dir.path().join("world.db")).unwrap();
        let n = w.stories()._count().unwrap();
        assert_eq!(n, 1, "story should survive close/reopen");
        let row = w.stories().get_by_slug("lin-mo").unwrap().unwrap();
        assert_eq!(row.title, "Lin Mo");
    }
}

#[test]
fn reopen_is_idempotent_on_migrations() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("world.db");

    // Open, then close. Reopen and confirm the migration count
    // didn't double (refinery must record the applied set).
    for _ in 0..3 {
        let w = World::open_at(&db).unwrap();
        let conn = w.conn().unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM refinery_schema_history",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 8);
    }
}

#[test]
fn slug_uniqueness_is_enforced() {
    let w = fresh();
    w.stories()
        .create(NewStory { slug: "alpha", title: "Alpha", summary: "" })
        .unwrap();
    let err = w
        .stories()
        .create(NewStory { slug: "alpha", title: "Dup", summary: "" })
        .unwrap_err();
    assert!(
        matches!(err, StoreError::Sqlite(rusqlite::Error::SqliteFailure(ref e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation),
        "expected UNIQUE constraint violation, got {err:?}"
    );
}

#[test]
fn fk_violation_when_chapter_has_no_story() {
    let w = fresh();
    let err = w
        .scenes()
        .create_chapter(sagaline_store::repo::scene::NewChapter {
            story_id: "does-not-exist",
            slug: "ch-1",
            ordinal: 1,
            title: "Ch 1",
            synopsis: "",
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Sqlite(_)));
}

#[test]
fn multi_table_transaction_all_or_nothing() {
    let w = fresh();
    let story = w
        .stories()
        .create(NewStory { slug: "tx-test", title: "Tx", summary: "" })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(sagaline_store::repo::scene::NewChapter {
            story_id: &story.id,
            slug: "ch-1",
            ordinal: 1,
            title: "Ch 1",
            synopsis: "",
        })
        .unwrap();

    // Wrap a multi-step write in one transaction: scene + its
    // character assignment. If the second insert violates a
    // constraint, the first must roll back too.
    let mut conn = w.conn().unwrap();
    let result: Result<(), StoreError> = (|| {
        let mut tx = conn.transaction()?;
        let _scene = w.scenes().create_scene_in_tx(
            &tx,
            NewScene {
                chapter_id: &ch.id,
                slug: "scene-1",
                ordinal: 1,
                title: "S1",
                synopsis: "",
            },
        )?;
        // Intentionally bogus FK to trigger a constraint failure.
        tx.execute(
            "INSERT INTO scene_characters (scene_id, character_id) VALUES (?1, ?2)",
            rusqlite::params!["does-not-exist", "does-not-exist-either"],
        )?;
        tx.commit()?;
        Ok(())
    })();

    assert!(result.is_err(), "expected the bogus FK to fail");
    // Scene row must not exist — the transaction rolled back.
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM scenes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "scene must not persist after tx rollback");
}

#[test]
fn character_lives_under_story() {
    let w = fresh();
    let story = w
        .stories()
        .create(NewStory { slug: "s", title: "S", summary: "" })
        .unwrap();
    let c = w
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "lin-mo",
            name: "Lin Mo",
            occupation: Some("reporter"),
            bio: "stubborn",
        })
        .unwrap();
    assert_eq!(c.slug, "lin-mo");

    let list = w.characters().list_for_story(&story.id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Lin Mo");
}

#[test]
fn pool_handles_concurrent_writes() {
    // 4 threads each insert a story; the pool must serialize them
    // without dropping or deadlocking.
    //
    // Uses a tempfile-backed world, not `in_memory()`, because
    // SQLite `:memory:` databases can't enable WAL (no shared-
    // memory mapping), and without WAL the pool's connections lock
    // each other under contention. Production code uses a real
    // file on disk, so this is the realistic code path.
    let dir = tempfile::tempdir().unwrap();
    let w = World::open_at(&dir.path().join("world.db")).unwrap();
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let w = w.clone();
            std::thread::spawn(move || {
                w.stories()
                    .create(NewStory {
                        slug: &format!("story-{i}"),
                        title: "T",
                        summary: "",
                    })
                    .unwrap();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(w.stories()._count().unwrap(), 4);
}

#[test]
fn uuid_v7_ids_are_unique_and_sortable() {
    let w = fresh();
    let ids: Vec<String> = (0..3).map(|_| w.stories().new_id()).collect();
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 3, "ids must be unique");
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "UUID v7 ids should be roughly time-ordered");
}

// ---- provider_key + jobs repos (formerly in sagaline-keys) -------

use sagaline_store::repo::job::{Job, JobStatus};
use sagaline_store::ProviderKeyId;
use secrecy::SecretString;

#[test]
fn key_repo_round_trip_uses_age_encryption() {
    let w = World::in_memory().unwrap();
    let id = ProviderKeyId::new("minimax", "work-laptop").unwrap();
    w.keys()
        .put(
            &id,
            &SecretString::new("sk-test-1234567890".to_string().into_boxed_str()),
        )
        .unwrap();
    let handle = w.keys().get(&id).unwrap();
    assert_eq!(
        handle.reveal().expose_secret(),
        "sk-test-1234567890",
        "decryption must round-trip the plaintext"
    );
    assert_eq!(handle.id(), &id);
    assert_eq!(w.keys()._count().unwrap(), 1);
}

#[test]
fn key_repo_get_missing_is_not_found() {
    let w = World::in_memory().unwrap();
    let id = ProviderKeyId::new("minimax", "absent").unwrap();
    let err = w.keys().get(&id).unwrap_err();
    assert!(matches!(err, sagaline_store::StoreError::NotFound { .. }));
}

#[test]
fn key_repo_put_overwrites() {
    let w = World::in_memory().unwrap();
    let id = ProviderKeyId::new("minimax", "k").unwrap();
    w.keys()
        .put(
            &id,
            &SecretString::new("old".to_string().into_boxed_str()),
        )
        .unwrap();
    w.keys()
        .put(
            &id,
            &SecretString::new("new".to_string().into_boxed_str()),
        )
        .unwrap();
    assert_eq!(
        w.keys().get(&id).unwrap().reveal().expose_secret(),
        "new"
    );
}

#[test]
fn key_repo_list_ids_sorts_by_provider_then_key() {
    let w = World::in_memory().unwrap();
    for (p, k) in [("openai", "z"), ("minimax", "a"), ("minimax", "b")] {
        w.keys()
            .put(
                &ProviderKeyId::new(p, k).unwrap(),
                &SecretString::new("x".to_string().into_boxed_str()),
            )
            .unwrap();
    }
    let names: Vec<String> = w
        .keys()
        .list_ids()
        .unwrap()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, vec!["minimax/a", "minimax/b", "openai/z"]);
}

#[test]
fn key_repo_delete_removes() {
    let w = World::in_memory().unwrap();
    let id = ProviderKeyId::new("minimax", "k").unwrap();
    w.keys()
        .put(
            &id,
            &SecretString::new("x".to_string().into_boxed_str()),
        )
        .unwrap();
    assert!(w.keys().delete(&id).unwrap());
    assert!(matches!(
        w.keys().get(&id).unwrap_err(),
        sagaline_store::StoreError::NotFound { .. }
    ));
}

#[test]
fn job_repo_round_trip() {
    let w = World::in_memory().unwrap();
    let job = Job {
        job_id: "j1".into(),
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
    };
    w.jobs().put(&job).unwrap();
    let got = w.jobs().get("j1").unwrap();
    assert_eq!(got.provider, "minimax");
    assert_eq!(got.status, JobStatus::Queued);

    let updated = w
        .jobs()
        .update("j1", |j| {
            j.status = JobStatus::Running;
            j.started_at = Some("2026-09-18T00:00:00Z".into());
        })
        .unwrap();
    assert_eq!(updated.status, JobStatus::Running);

    w.jobs().delete("j1").unwrap();
    assert!(matches!(
        w.jobs().get("j1").unwrap_err(),
        sagaline_store::StoreError::NotFound { .. }
    ));
}

#[test]
fn job_repo_list_by_status_and_pending() {
    let w = World::in_memory().unwrap();
    let mk = |id: &str, status: JobStatus| Job {
        job_id: id.into(),
        shot_id: "shot".into(),
        capability: "image".into(),
        provider: "minimax".into(),
        model_id: "x".into(),
        provider_task_id: None,
        status,
        started_at: None,
        finished_at: None,
        asset_path: None,
        attempt: 1,
        error: None,
    };
    w.jobs().put(&mk("q1", JobStatus::Queued)).unwrap();
    w.jobs().put(&mk("r1", JobStatus::Running)).unwrap();
    w.jobs().put(&mk("d1", JobStatus::Succeeded)).unwrap();

    assert_eq!(w.jobs().list_by_status(JobStatus::Queued).unwrap().len(), 1);
    assert_eq!(w.jobs().list_by_status(JobStatus::Running).unwrap().len(), 1);
    assert_eq!(w.jobs().list_pending().unwrap().len(), 2);
}

#[test]
fn legacy_keys_db_is_deleted_on_first_open() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("keys.db");
    // Pretend the old redb file exists with arbitrary bytes.
    std::fs::write(&legacy, b"not really redb").unwrap();
    assert!(legacy.exists());

    let _w = World::open_at(&dir.path().join("world.db")).unwrap();
    assert!(!legacy.exists(), "legacy keys.db must be removed");
}

#[test]
fn provider_key_id_rejects_bad_chars() {
    assert!(ProviderKeyId::new("minimax", "default").is_ok());
    assert!(ProviderKeyId::new("mini/max", "default").is_err());
    assert!(ProviderKeyId::new("", "default").is_err());
    let too_long = "a".repeat(65);
    assert!(ProviderKeyId::new("p", &too_long).is_err());
}
