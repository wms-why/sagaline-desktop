//! Unit tests for the two pure validation functions that gate the
//! dialog / form submissions. Render plumbing is exercised by the
//! integration tests under `tests/`.

use super::keys::validate_add_key;
use super::new_story_dialog::validate_create_story;

#[test]
fn rejects_missing_project_location() {
    let err = validate_create_story(None, "Hollow Star").unwrap_err();
    assert!(
        err.contains("project location"),
        "expected project-location hint, got: {err}"
    );
}

#[test]
fn rejects_empty_title() {
    let err = validate_create_story(Some("/tmp/x"), "").unwrap_err();
    assert!(
        err.contains("empty"),
        "expected empty-title message, got: {err}"
    );
}

#[test]
fn rejects_whitespace_only_title() {
    let err = validate_create_story(Some("/tmp/x"), "   ").unwrap_err();
    assert!(
        err.contains("empty"),
        "expected empty-title message for whitespace, got: {err}"
    );
}

#[test]
fn accepts_well_formed_input() {
    assert!(validate_create_story(Some("/tmp/x"), "Hollow Star").is_ok());
}

#[test]
fn location_check_runs_before_title_check() {
    // Regression guard: when both inputs are bad, the user
    // sees the location error first — that is the more
    // actionable fix (⌘ ,) and matches the dialog copy.
    let err = validate_create_story(None, "").unwrap_err();
    assert!(
        err.contains("project location"),
        "location error must take priority, got: {err}"
    );
}

// ---- validate_add_key -------------------------------------------

#[test]
fn add_key_rejects_empty_provider() {
    let err = validate_add_key("", "default", "sk-secret").unwrap_err();
    assert!(
        err.contains("provider"),
        "expected provider-required message, got: {err}"
    );
}

#[test]
fn add_key_rejects_whitespace_provider() {
    let err = validate_add_key("   ", "default", "sk-secret").unwrap_err();
    assert!(
        err.contains("provider"),
        "whitespace-only provider must be rejected, got: {err}"
    );
}

#[test]
fn add_key_rejects_empty_key_id() {
    let err = validate_add_key("openai", "", "sk-secret").unwrap_err();
    assert!(
        err.contains("key id"),
        "expected key-id-required message, got: {err}"
    );
}

#[test]
fn add_key_rejects_empty_secret() {
    let err = validate_add_key("openai", "default", "").unwrap_err();
    assert!(
        err.contains("API key"),
        "expected api-key-required message, got: {err}"
    );
}

#[test]
fn add_key_rejects_invalid_id_charset() {
    // ProviderKeyId only allows [A-Za-z0-9_-]. A slash makes
    // the parser reject the composite id.
    let err = validate_add_key("openai/team", "default", "sk-secret").unwrap_err();
    assert!(
        err.contains("invalid id"),
        "expected invalid-id message, got: {err}"
    );
}

#[test]
fn add_key_provider_check_runs_before_secret_check() {
    // Regression guard: when both provider and secret are bad,
    // the user sees the provider error first — matches the
    // dialog copy and is more actionable.
    let err = validate_add_key("", "default", "").unwrap_err();
    assert!(
        err.contains("provider"),
        "provider error must take priority, got: {err}"
    );
}

#[test]
fn add_key_accepts_well_formed_input() {
    assert!(validate_add_key("openai", "default", "sk-secret-123").is_ok());
    assert!(validate_add_key("openai", "prod", "sk-other").is_ok());
    // Underscores + hyphens are allowed.
    assert!(validate_add_key("deep_seek", "org-xyz", "key").is_ok());
}