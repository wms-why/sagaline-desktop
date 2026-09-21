//! Story root — the directory that holds `story.md` and all artifacts.

use std::path::{Path, PathBuf};

use serde_yaml::Value;

use crate::entity::{EntityId, EntityType, ParsedEntity};
use crate::error::CoreError;
use crate::frontmatter;
use crate::markdown::{split, SplitFile};

/// The root of a single story. Holds the directory path and the cached
/// `story.md` parse (id + title).
///
/// Construct with [`StoryRoot::new`]; the directory must exist, be a
/// directory, and contain `story.md` with at least `id` / `type` / `slug`
/// front matter.
#[derive(Debug, Clone)]
pub struct StoryRoot {
    root: PathBuf,
    story_id: EntityId,
    title: String,
}

impl StoryRoot {
    /// Scaffold a fresh story on disk at `parent_dir/slug` and load it back.
    ///
    /// Writes a minimal `story.md` containing the standard front matter
    /// (`id` / `type` / `slug` / `title` / `created_at` / `updated_at`) plus
    /// a placeholder body. No `assets/` directory is created at this layer;
    /// the agent creates that lazily.
    ///
    /// Re-validates the freshly-written file by re-loading it via
    /// [`StoryRoot::new`]; any post-init front-matter failure surfaces as
    /// [`CoreError::FrontMatter`].
    pub fn init(
        parent_dir: &Path,
        slug: &str,
        title: &str,
    ) -> Result<Self, CoreError> {
        validate_slug(slug)?;

        if title.trim().is_empty() {
            return Err(CoreError::InvalidSlug {
                slug: title.to_string(),
                reason: "title must not be empty",
            });
        }

        let meta = std::fs::metadata(parent_dir).map_err(|_| {
            CoreError::StoryPathMissing(parent_dir.to_path_buf())
        })?;
        if !meta.is_dir() {
            return Err(CoreError::StoryPathNotDir(parent_dir.to_path_buf()));
        }

        let target = parent_dir.join(slug);
        match std::fs::metadata(&target) {
            Ok(_) => return Err(CoreError::StoryDirExists(target)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CoreError::ReadFile {
                    path: target,
                    source: e,
                });
            }
        }

        std::fs::create_dir_all(&target).map_err(|source| CoreError::ReadFile {
            path: target.clone(),
            source,
        })?;

        let now = rfc3339_now();
        let story_id = format!("story_{slug}");
        let body = format!(
            "# {title}\n\nWrite the story pitch here. The agent reads this file before every planning step.\n",
        );
        let text = format!(
            "---\nid: {story_id}\ntype: story\nslug: {slug}\ntitle: {title}\ncreated_at: {now}\nupdated_at: {now}\n---\n\n{body}",
        );

        let story_file = target.join("story.md");
        std::fs::write(&story_file, text).map_err(|source| CoreError::ReadFile {
            path: story_file.clone(),
            source,
        })?;

        StoryRoot::new(target)
    }

    /// Validate the path and load the top-level `story.md`.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, CoreError> {
        let root = root.into();
        let meta = std::fs::metadata(&root)
            .map_err(|_| CoreError::StoryPathMissing(root.clone()))?;
        if !meta.is_dir() {
            return Err(CoreError::StoryPathNotDir(root));
        }
        let story_file = root.join("story.md");
        if !story_file.is_file() {
            return Err(CoreError::MissingStoryFile(root));
        }
        let text = std::fs::read_to_string(&story_file).map_err(|source| CoreError::ReadFile {
            path: story_file.clone(),
            source,
        })?;
        let SplitFile { frontmatter, body: _ } = split(&text).map_err(|e| match e {
            CoreError::FrontMatter { source, .. } => CoreError::FrontMatter {
                path: story_file,
                source,
            },
            other => other,
        })?;

        let id = frontmatter::id_of(&frontmatter)
            .ok_or_else(|| CoreError::MissingStoryFile(root.clone()))?;
        let type_ = frontmatter
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::MissingStoryFile(root.clone()))?;
        if type_ != EntityType::Story.as_str() {
            return Err(CoreError::MissingStoryFile(root));
        }

        let title = frontmatter::title_of(&frontmatter).unwrap_or_else(|| id.clone());
        Ok(Self {
            root,
            story_id: EntityId::new(id),
            title,
        })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn story_id(&self) -> &EntityId {
        &self.story_id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// Build the canonical [`ParsedEntity`] for the story itself.
    pub fn story_entity(&self, full_frontmatter: Value, body: String) -> ParsedEntity {
        ParsedEntity {
            id: self.story_id.clone(),
            type_: EntityType::Story,
            slug: "story".into(),
            path: PathBuf::from("story.md"),
            frontmatter: full_frontmatter,
            body,
        }
    }
}

/// Validate a story slug. Accepts ASCII lowercase letters, digits, and
/// `-`. No leading/trailing dashes, no consecutive `--`, no empty input.
fn validate_slug(slug: &str) -> Result<(), CoreError> {
    if slug.is_empty() {
        return Err(CoreError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug must not be empty",
        });
    }
    if slug != slug.trim() {
        return Err(CoreError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug must not have leading or trailing whitespace",
        });
    }
    if slug.starts_with('-') {
        return Err(CoreError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug must not start with `-`",
        });
    }
    if slug.ends_with('-') {
        return Err(CoreError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug must not end with `-`",
        });
    }
    if slug.contains("--") {
        return Err(CoreError::InvalidSlug {
            slug: slug.to_string(),
            reason: "slug must not contain consecutive `-`",
        });
    }
    for ch in slug.chars() {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(CoreError::InvalidSlug {
                slug: slug.to_string(),
                reason: "slug may only contain lowercase letters, digits, and `-`",
            });
        }
    }
    Ok(())
}

/// Format the current wall-clock time as an RFC 3339 string in UTC.
///
/// No external date crate is used — we apply a manual Y/M/D/H/M/S conversion
/// from `SystemTime::now()`. The output is rounded to the nearest second
/// because the YAML timestamp is human-readable and the agent never needs
/// Public re-export for the UI / binary crate. Use
/// [`rfc3339_now`] from inside this crate; outside it, import
/// this alias.
/// Public re-export for callers outside this crate. Inside the
/// crate, use [`rfc3339_now`].
pub fn rfc3339_now_public() -> String {
    rfc3339_now()
}

pub(crate) fn rfc3339_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Days from 1970-01-01 → Y/M/D via the proleptic Gregorian calendar.
    let secs_per_day = 86_400u64;
    let (days, time_of_day) = (secs / secs_per_day, secs % secs_per_day);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    let (year, month, day) = civil_from_days(days as i64);

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z",
    )
}

/// Howard Hinnant's `civil_from_days` algorithm. Returns
/// `(year, month, day)` for the Gregorian date `days` days after
/// 1970-01-01. Works for the entire Unix-epoch timeline we care about.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn new_rejects_missing_dir() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        assert!(matches!(
            StoryRoot::new(&missing),
            Err(CoreError::StoryPathMissing(_))
        ));
    }

    #[test]
    fn new_rejects_file_as_root() {
        let tmp = tempdir().unwrap();
        let f = tmp.path().join("file.txt");
        write(&f, "hi");
        assert!(matches!(
            StoryRoot::new(&f),
            Err(CoreError::StoryPathNotDir(_))
        ));
    }

    #[test]
    fn new_rejects_no_story_md() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("story");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            StoryRoot::new(&dir),
            Err(CoreError::MissingStoryFile(_))
        ));
    }

    #[test]
    fn new_accepts_minimal_story() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("story");
        std::fs::create_dir_all(&dir).unwrap();
        write(
            &dir.join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\ntitle: My Story\n---\n",
        );
        let root = StoryRoot::new(&dir).unwrap();
        assert_eq!(root.story_id().as_str(), "s1");
        assert_eq!(root.title(), "My Story");
    }

    #[test]
    fn init_creates_minimal_story() {
        let tmp = tempdir().unwrap();
        let root = StoryRoot::init(tmp.path(), "demo-story", "Demo Story").unwrap();
        assert_eq!(root.story_id().as_str(), "story_demo-story");
        assert_eq!(root.title(), "Demo Story");
        let path = root.path();
        assert!(path.is_dir());
        assert!(path.join("story.md").is_file());

        let text = std::fs::read_to_string(path.join("story.md")).unwrap();
        // Round-trip through StoryRoot::new so a regression in init's
        // front matter (or the loader's expectations) fails here too.
        let root2 = StoryRoot::new(path).unwrap();
        assert_eq!(root2.story_id().as_str(), "story_demo-story");
        // Spot-check the rendered file: must contain slug, title, a body.
        assert!(text.contains("slug: demo-story"));
        assert!(text.contains("title: Demo Story"));
        assert!(text.contains("# Demo Story"));
    }

    #[test]
    fn init_rejects_invalid_slug() {
        let tmp = tempdir().unwrap();
        let cases = [
            ("", "empty"),
            ("   ", "whitespace"),
            ("Bad/Slug", "slash"),
            ("-leading", "leading-dash"),
            ("trailing-", "trailing-dash"),
            ("double--dash", "double-dash"),
            ("UPPER", "uppercase"),
            ("under_score", "underscore"),
        ];
        for (slug, _label) in cases {
            let res = StoryRoot::init(tmp.path(), slug, "Title");
            assert!(
                matches!(res, Err(CoreError::InvalidSlug { .. })),
                "slug {slug:?} should be rejected but got {res:?}",
            );
        }
    }

    #[test]
    fn init_rejects_existing_directory() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join("already");
        std::fs::create_dir_all(&dir).unwrap();
        let res = StoryRoot::init(tmp.path(), "already", "Title");
        assert!(matches!(res, Err(CoreError::StoryDirExists(_))));
    }

    #[test]
    fn init_rejects_missing_parent() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("nope");
        let res = StoryRoot::init(&missing, "slug", "Title");
        assert!(matches!(res, Err(CoreError::StoryPathMissing(_))));
    }

    #[test]
    fn init_rejects_empty_title() {
        let tmp = tempdir().unwrap();
        let res = StoryRoot::init(tmp.path(), "ok-slug", "   ");
        assert!(matches!(res, Err(CoreError::InvalidSlug { .. })));
    }

    #[test]
    fn civil_from_days_epoch() {
        // Day 0 is 1970-01-01.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // Day 1 is 1970-01-02.
        assert_eq!(civil_from_days(1), (1970, 1, 2));
        // Day 365 is 1971-01-01 (1970 was not a leap year).
        assert_eq!(civil_from_days(365), (1971, 1, 1));
        // Day 730 is 1972-01-01 (1972 is a leap year).
        assert_eq!(civil_from_days(730), (1972, 1, 1));
    }
}