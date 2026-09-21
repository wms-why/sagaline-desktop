//! Storage-agnostic story façade.
//!
//! The UI talks in domain terms — story, character, scene, shot — and
//! never names a directory path, a slug, or a Markdown filename. This
//! module is the only place the UI is allowed to construct, list, or
//! open a story; the underlying [`crate::StoryRoot`] /
//! [`crate::StoryGraph`] are the on-disk implementation and are not
//! part of the façade's public surface.
//!
//! On disk the layout is unchanged (see [`crate::path`]). The façade
//! just re-projects the directory tree into `Story` / `StorySummary`
//! handles the UI can hold without knowing the path.
//!
//! The agent and tests still use [`crate::StoryRoot`] /
//! [`crate::StoryGraph`] directly; those types are deliberately
//! preserved as the in-memory representation of the agent's memory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_yaml::Value;

use crate::entity::EntityId;
use crate::error::CoreError;
use crate::markdown::split;
use crate::story_root::{rfc3339_now, StoryRoot};

/// A user-facing story handle.
///
/// Holds the title the user sees, the entity id the agent uses, and
/// the timestamp metadata that drives the recent-stories list. The
/// on-disk location is hidden behind the opaque [`StoryHandle`]; the
/// UI never sees a path.
#[derive(Debug, Clone)]
pub struct Story {
    handle: StoryHandle,
    /// Globally-unique id of the story (taken from the `id:` field of
    /// the story entry).
    pub id: EntityId,
    /// Title from the story's front matter. Never empty.
    pub title: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 last-modified timestamp.
    pub updated_at: String,
}

impl Story {
    /// Borrow the opaque storage handle. The binary crate uses it
    /// to hand the path back to the agent; the UI never names a
    /// `StoryHandle::path` explicitly.
    pub fn handle(&self) -> &StoryHandle {
        &self.handle
    }

    /// Build a `Story` for a path the user opened outside the
    /// `StoryStore` flow (e.g. via the file picker's "Open a
    /// story" button). The fields are populated from the on-disk
    /// `story.md` if present, otherwise placeholders are used and
    /// `WorkspaceState::open_story` will surface the real error.
    pub fn new_public(
        handle: StoryHandle,
        id: EntityId,
        title: String,
        created_at: String,
        updated_at: String,
    ) -> Self {
        Self {
            handle,
            id,
            title,
            created_at,
            updated_at,
        }
    }
}

/// Opaque storage handle. [`StoryHandle::new`] is public so the
/// binary crate can wrap a path the user picked; the UI never
/// names a path.
/// only caller is [`FileStoryStore`]; the UI receives `Arc<Story>`s
/// and never names a path.
#[derive(Debug, Clone)]
pub struct StoryHandle {
    root: Arc<PathBuf>,
}

impl StoryHandle {
    /// Build a handle for the given path. Used by the binary
    /// crate to wrap a path the user picked through the file
    /// picker; the UI never names a path.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
        }
    }

    /// Borrow the on-disk path. The binary crate uses it when
    /// handing the story off to the agent; the UI never names it.
    pub fn path(&self) -> &Path {
        self.root.as_path()
    }
}

impl PartialEq for StoryHandle {
    fn eq(&self, other: &Self) -> bool {
        self.root.as_path() == other.root.as_path()
    }
}

impl Eq for StoryHandle {}

/// Lightweight summary for the recent-stories list.
///
/// Holds enough to render the row in the picker; the full [`Story`]
/// is loaded lazily by [`StoryStore::open`].
#[derive(Debug, Clone)]
pub struct StorySummary {
    pub handle: StoryHandle,
    pub title: String,
    /// RFC 3339 last-modified timestamp. RFC 3339 strings sort
    /// monotonically, so the recent list uses a string compare.
    pub updated_at: String,
}

/// The user's chosen place to keep new stories.
///
/// Stored as a per-machine preference in `~/.sageline/data/prefs.toml`
/// (see the binary crate's prefs store). The UI calls
/// [`StoryStore::create`] and the store resolves the location
/// internally.
#[derive(Debug, Clone)]
pub struct ProjectLocation(pub(crate) Arc<PathBuf>);

impl ProjectLocation {
    /// Build a location from an arbitrary path. The path is not
    /// validated here — the store does the directory-walk /
    /// create-if-missing work on first use.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(Arc::new(path.into()))
    }

    /// Borrow the path. The binary crate reads it when persisting
    /// the preference; the UI never displays it directly.
    pub fn path(&self) -> &Path {
        self.0.as_path()
    }

    /// Render the path for display in the UI. The plan is to show
    /// this verbatim in the project-locations hint; it is a
    /// `Display::to_string` of the path, not a directory-tree
    /// metaphor.
    pub fn display(&self) -> String {
        self.0.display().to_string()
    }
}

/// Storage-agnostic story operations.
///
/// The UI holds an `Arc<dyn StoryStore>` (provided by the binary
/// crate's `AppEnv`) and calls these methods. The trait exists so
/// tests can substitute a `MemoryStoryStore` if the file-backed
/// store ever needs to be replaced.
pub trait StoryStore: Send + Sync + 'static {
    /// List every story under the configured [`ProjectLocation`],
    /// most-recently-updated first. Capped at 25 entries.
    fn list(&self) -> Result<Vec<StorySummary>, CoreError>;

    /// Open the story behind `handle`, loading the full [`Story`]
    /// (with timestamps and id). The store re-walks the directory
    /// each time so a freshly-edited `story.md` is picked up.
    fn open(&self, handle: &StoryHandle) -> Result<Arc<Story>, CoreError>;

    /// Re-walk an already-open story and return a refreshed
    /// [`Story`]. Equivalent to `open(handle)` but takes the
    /// existing story as a hint to avoid an extra path round-trip.
    fn reload(&self, story: &Story) -> Result<Arc<Story>, CoreError>;

    /// Create a brand-new story with the given `title`. The slug
    /// and the on-disk path are derived from the title (lowercase,
    /// digits, `-`; collisions append `-2`, `-3`, …). The
    /// [`ProjectLocation`] is implicit — the store was constructed
    /// against it.
    fn create(&self, title: &str) -> Result<Arc<Story>, CoreError>;
}

/// File-backed [`StoryStore`] implementation.
///
/// The on-disk layout is unchanged (a directory with a `story.md`
/// at the root, see [`crate::path`]). The store is the only
/// component that knows about the directory layout — every other
/// call site holds `Arc<Story>` / `StorySummary`.
pub struct FileStoryStore {
    location: ProjectLocation,
}

impl FileStoryStore {
    /// Wrap a [`ProjectLocation`]. The location's path is not
    /// inspected here; the first `create` / `open` validates it.
    pub fn new(location: ProjectLocation) -> Self {
        Self { location }
    }

    /// Borrow the configured location. Used by the binary's prefs
    /// store to read the path back out without exposing
    /// `ProjectLocation`'s field to non-`core` code.
    pub fn location(&self) -> &ProjectLocation {
        &self.location
    }
}

impl StoryStore for FileStoryStore {
    fn list(&self) -> Result<Vec<StorySummary>, CoreError> {
        let base = self.location.path();
        let meta = match fs::metadata(base) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(e) => {
                return Err(CoreError::Walk {
                    root: base.to_path_buf(),
                    source: e,
                });
            }
        };
        if !meta.is_dir() {
            return Err(CoreError::StoryPathNotDir(base.to_path_buf()));
        }

        let mut out: Vec<StorySummary> = Vec::new();
        let entries = match fs::read_dir(base) {
            Ok(it) => it,
            Err(e) => {
                return Err(CoreError::Walk {
                    root: base.to_path_buf(),
                    source: e,
                });
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Hidden / dotfile directories are skipped — matches
            // the on-disk walker in `graph.rs`.
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let root = match StoryRoot::new(&path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let (title, updated_at) = read_story_meta(&root).unwrap_or_else(|| {
                (root.title().to_string(), rfc3339_now())
            });
            out.push(StorySummary {
                handle: StoryHandle::new(path),
                title,
                updated_at,
            });
        }
        // RFC 3339 sorts monotonically descending = most recent
        // first.
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        out.truncate(25);
        Ok(out)
    }

    fn open(&self, handle: &StoryHandle) -> Result<Arc<Story>, CoreError> {
        let root = StoryRoot::new(handle.path())?;
        let (title, updated_at) =
            read_story_meta(&root).unwrap_or_else(|| (root.title().to_string(), rfc3339_now()));
        let created_at = updated_at.clone();
        Ok(Arc::new(Story {
            handle: handle.clone(),
            id: root.story_id().clone(),
            title,
            created_at,
            updated_at,
        }))
    }

    fn reload(&self, story: &Story) -> Result<Arc<Story>, CoreError> {
        self.open(story.handle())
    }

    fn create(&self, title: &str) -> Result<Arc<Story>, CoreError> {
        if title.trim().is_empty() {
            return Err(CoreError::InvalidSlug {
                slug: title.to_string(),
                reason: "title must not be empty",
            });
        }
        let base = self.location.path();
        if !base.exists() {
            return Err(CoreError::StoryPathMissing(base.to_path_buf()));
        }
        if !base.is_dir() {
            return Err(CoreError::StoryPathNotDir(base.to_path_buf()));
        }

        let mut slug = derive_slug(title).ok_or_else(|| CoreError::InvalidSlug {
            slug: title.to_string(),
            reason: "title produces no usable slug, use letters/digits",
        })?;

        // Collision-avoid: try `slug`, `slug-2`, `slug-3`, …
        let mut attempt = 1usize;
        loop {
            let candidate = base.join(&slug);
            match fs::metadata(&candidate) {
                Ok(_) => {
                    attempt += 1;
                    slug = format!("{}-{}", derive_slug(title).unwrap(), attempt);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => {
                    return Err(CoreError::ReadFile {
                        path: candidate,
                        source: e,
                    });
                }
            }
        }

        let root = StoryRoot::init(base, &slug, title)?;
        let now = rfc3339_now();
        Ok(Arc::new(Story {
            handle: StoryHandle::new(root.path().to_path_buf()),
            id: root.story_id().clone(),
            title: root.title().to_string(),
            created_at: now.clone(),
            updated_at: now,
        }))
    }
}

/// Read `(title, updated_at)` out of a freshly-loaded `story.md`.
/// Returns `None` if the file is unreadable or has no front matter;
/// the caller falls back to the title from the path and the current
/// wall-clock time.
fn read_story_meta(root: &StoryRoot) -> Option<(String, String)> {
    let story_file = root.path().join("story.md");
    let text = fs::read_to_string(&story_file).ok()?;
    let split = split(&text).ok()?;
    let title = split
        .frontmatter
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| root.title().to_string());
    let updated_at = split
        .frontmatter
        .get("updated_at")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(rfc3339_now);
    Some((title, updated_at))
}

/// Derive a slug from a human title. Strips whitespace, lowercases,
/// maps non-`[a-z0-9-]` characters to `-`, collapses consecutive
/// `-`, and trims leading / trailing `-`. Returns `None` if nothing
/// is left.
fn derive_slug(title: &str) -> Option<String> {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = true; // suppress leading dashes
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_dash = false;
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        // All other characters are dropped.
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_slug_basic() {
        assert_eq!(derive_slug("Hollow Star"), Some("hollow-star".into()));
        assert_eq!(derive_slug("  My First Story  "), Some("my-first-story".into()));
        assert_eq!(derive_slug("Story #2!"), Some("story-2".into()));
        assert_eq!(derive_slug("----"), None);
        assert_eq!(derive_slug("中文标题"), None);
    }

    #[test]
    fn file_store_create_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileStoryStore::new(ProjectLocation::new(tmp.path()));
        let s1 = store.create("My First Story").unwrap();
        assert_eq!(s1.title, "My First Story");
        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "My First Story");

        // Second create with the same title produces a distinct
        // handle (auto-disambiguated slug).
        let s2 = store.create("My First Story").unwrap();
        assert_ne!(s1.handle(), s2.handle());
        let list2 = store.list().unwrap();
        assert_eq!(list2.len(), 2);
    }

    #[test]
    fn file_store_rejects_empty_title() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FileStoryStore::new(ProjectLocation::new(tmp.path()));
        let err = store.create("   ").unwrap_err();
        assert!(matches!(err, CoreError::InvalidSlug { .. }));
    }

    #[test]
    fn file_store_missing_location_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let store = FileStoryStore::new(ProjectLocation::new(missing));
        let err = store.create("X").unwrap_err();
        assert!(matches!(err, CoreError::StoryPathMissing(_)));
    }
}
