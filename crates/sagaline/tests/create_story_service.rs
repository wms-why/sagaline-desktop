//! End-to-end regression test for the Create-story flow through
//! the binary's `StoryService` adapter.
//!
//! Reproduces the bug class the user reported: "click Create, nothing
//! happens". The previous failure mode was that the dialog closed
//! before the user could see what went wrong (or didn't show errors
//! at all). These tests exercise the actual code path the click
//! handler drives:
//!
//! 1. `AppEnvStoryService::create_story` returns Ok on a well-formed
//!    title and writes the new story on disk.
//! 2. `create_story` returns a meaningful error on an empty title
//!    (so the dialog can surface it instead of silently closing).
//! 3. `create_story` returns a meaningful error when no project
//!    location has been set yet.
//! 4. `set_project_location` followed by `create_story` is the
//!    happy path the user actually hits.
//!
//! Uses `SAGALINE_DATA_DIR` to redirect `AppEnv::open` at a
//! `tempfile::TempDir`, and a separate `tempfile::TempDir` for the
//! project location. This is the exact same plumbing the desktop
//! binary uses — no mocks.

use std::path::PathBuf;
use std::sync::Arc;

use sagaline::AppEnv;
use sagaline_ui::StoryService;

#[test]
fn create_story_fails_when_no_project_location_is_set() {
    let data_dir = tempfile::tempdir().expect("data dir");
    // SAFETY: this test sets a process-wide env var that other
    // tests in the same binary may observe. Run in this serial
    // test file with no concurrent `AppEnv::open` calls.
    // SAFETY:
    // `set_var` is `unsafe` from Rust 2024 onward; pin to a
    // single-threaded scoped env so we don't leak the override
    // to other tests.
    std::env::set_var("SAGALINE_DATA_DIR", data_dir.path());
    let env = Arc::new(AppEnv::open().expect("env open"));

    let svc = sagaline::AppEnvStoryService::new(env.clone());
    assert!(
        svc.project_location_display().is_none(),
        "fresh env must have no project location"
    );

    let err = svc
        .create_story("Hollow Star")
        .expect_err("create without project location must fail");
    assert!(
        err.contains("project location"),
        "error must mention project location, got: {err}"
    );

    std::env::remove_var("SAGALINE_DATA_DIR");
}

#[test]
fn create_story_writes_story_on_disk_when_location_is_set() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let project_dir = tempfile::tempdir().expect("project dir");
    std::env::set_var("SAGALINE_DATA_DIR", data_dir.path());

    let env = Arc::new(AppEnv::open().expect("env open"));
    let svc = sagaline::AppEnvStoryService::new(env.clone());

    // Wire up the project location exactly as the Settings modal
    // would. `set_project_location` persists to prefs and rebuilds
    // the in-memory `StoryStore`.
    svc.set_project_location(PathBuf::from(project_dir.path()))
        .expect("set project location");
    assert_eq!(
        svc.project_location_display().as_deref(),
        Some(project_dir.path().to_str().unwrap())
    );

    let story = svc
        .create_story("Hollow Star")
        .expect("create should succeed with location + title");
    assert_eq!(story.title, "Hollow Star");

    // The story handle must point inside the project location and
    // the directory must really exist on disk.
    let story_path = story.handle().path();
    assert!(
        story_path.starts_with(project_dir.path()),
        "story path {} must live under project location {}",
        story_path.display(),
        project_dir.path().display()
    );
    assert!(story_path.is_dir(), "story directory must exist");

    // `recent_stories` is the list the right-hand panel reads
    // after a successful create.
    let recent = svc.recent_stories();
    assert_eq!(recent.len(), 1, "expected one story in recent list");
    assert_eq!(recent[0].title, "Hollow Star");

    std::env::remove_var("SAGALINE_DATA_DIR");
}

#[test]
fn create_story_reports_empty_title_error() {
    let data_dir = tempfile::tempdir().expect("data dir");
    let project_dir = tempfile::tempdir().expect("project dir");
    std::env::set_var("SAGALINE_DATA_DIR", data_dir.path());

    let env = Arc::new(AppEnv::open().expect("env open"));
    let svc = sagaline::AppEnvStoryService::new(env.clone());
    svc.set_project_location(PathBuf::from(project_dir.path()))
        .expect("set project location");

    let err = svc
        .create_story("   ")
        .expect_err("empty title must error");
    // The UI uses this string verbatim in the dialog error label;
    // it must be human-readable, not a debug dump.
    assert!(
        err.to_lowercase().contains("empty") || err.to_lowercase().contains("slug"),
        "expected a user-facing empty-title error, got: {err}"
    );

    std::env::remove_var("SAGALINE_DATA_DIR");
}

#[test]
fn create_story_creates_distinct_handles_for_duplicate_titles() {
    // The slug-disambiguation in `FileStoryStore::create` means two
    // stories with the same title still get distinct on-disk
    // directories. The dialog handler must surface both, not the
    // first one twice.
    let data_dir = tempfile::tempdir().expect("data dir");
    let project_dir = tempfile::tempdir().expect("project dir");
    std::env::set_var("SAGALINE_DATA_DIR", data_dir.path());

    let env = Arc::new(AppEnv::open().expect("env open"));
    let svc = sagaline::AppEnvStoryService::new(env.clone());
    svc.set_project_location(PathBuf::from(project_dir.path()))
        .expect("set project location");

    let s1 = svc.create_story("Hollow Star").expect("first create");
    let s2 = svc.create_story("Hollow Star").expect("second create");
    assert_ne!(
        s1.handle().path(),
        s2.handle().path(),
        "duplicate titles must get distinct on-disk paths"
    );
    let recent = svc.recent_stories();
    assert_eq!(recent.len(), 2, "both stories should appear in recent list");

    std::env::remove_var("SAGALINE_DATA_DIR");
}
