//! In-memory story graph: walks the directory once, returns parsed entities
//! and the references between them. Validation runs against this graph in
//! memory — there is no database to consult.
//!
//! See [`StoryGraph::load`] for the entry point.

use std::fs;
use std::path::{Path, PathBuf};

use serde::de::Error as _;

use crate::entity::{EntityId, EntityType, ParsedEntity, Reference};
use crate::error::{CoreError, ValidationError};
use crate::frontmatter;
use crate::markdown::{split, SplitFile};
use crate::path;
use crate::story_root::StoryRoot;

/// The loaded story. Holds the canonical story entry, all parsed entities
/// (including any that share an id with another — duplicates are caught by
/// [`StoryGraph::validate`]), and the directed reference graph between
/// them.
#[derive(Debug, Clone)]
pub struct StoryGraph {
    pub story: ParsedEntity,
    pub entities: Vec<ParsedEntity>,
    pub references: Vec<Reference>,
}

impl StoryGraph {
    /// Walk the story directory and produce the full in-memory graph.
    ///
    /// Fails if the directory doesn't satisfy [`StoryRoot::new`], or if any
    /// markdown file under it can't be read / parsed.
    pub fn load(root: &StoryRoot) -> Result<Self, CoreError> {
        let base = root.path();
        let mut entities: Vec<ParsedEntity> = Vec::new();
        let mut refs: Vec<Reference> = Vec::new();

        walk(base, &mut |rel_path, abs_path| {
            // story.md is materialized below from StoryRoot; skip the walk
            // copy so we don't double-load it.
            if rel_path == Path::new("story.md") {
                return Ok(());
            }

            let Some((type_, slug_from_path)) = path::classify(rel_path) else {
                return Ok(()); // not a tracked layout
            };

            let text = fs::read_to_string(abs_path).map_err(|source| CoreError::ReadFile {
                path: abs_path.to_path_buf(),
                source,
            })?;
            let SplitFile { frontmatter, body } = split(&text).map_err(|e| match e {
                CoreError::FrontMatter { source, .. } => CoreError::FrontMatter {
                    path: abs_path.to_path_buf(),
                    source,
                },
                other => other,
            })?;

            let id = frontmatter::id_of(&frontmatter).ok_or_else(|| {
                CoreError::FrontMatter {
                    path: abs_path.to_path_buf(),
                    source: serde_yaml::Error::custom("missing `id` in front matter"),
                }
            })?;

            let entity = ParsedEntity {
                id: EntityId::new(id),
                type_,
                slug: slug_from_path,
                path: rel_path.to_path_buf(),
                frontmatter,
                body,
            };
            entities.push(entity);
            Ok(())
        })?;

        let entity_refs: &[ParsedEntity] = &entities;
        let scene_indices: Vec<usize> = entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.type_ == EntityType::Scene)
            .map(|(i, _)| i)
            .collect();
        for i in scene_indices {
            let scene = &entities[i];
            refs.extend(frontmatter::scene_references(
                &scene.frontmatter,
                &scene.id,
                entity_refs,
            ));
        }

        // Materialize the canonical story entity from the StoryRoot. The
        // walk also visited story.md; pull its body / front matter from
        // there, or use defaults if the walker didn't visit it.
        let (story_fm, story_body) = match entities
            .iter()
            .position(|e| e.id == *root.story_id())
        {
            Some(idx) => {
                let e = entities.remove(idx);
                (e.frontmatter, e.body)
            }
            None => (serde_yaml::Value::Null, String::new()),
        };
        let story = root.story_entity(story_fm, story_body);

        Ok(Self {
            story,
            entities,
            references: refs,
        })
    }

    /// Lookup by entity id. Returns the first match when ids collide.
    pub fn entity(&self, id: &EntityId) -> Option<&ParsedEntity> {
        self.entities.iter().find(|e| &e.id == id)
    }

    /// All entities of a given type, sorted by on-disk path.
    pub fn entities_by_type(&self, ty: EntityType) -> Vec<&ParsedEntity> {
        let mut v: Vec<&ParsedEntity> = self
            .entities
            .iter()
            .filter(|e| e.type_ == ty)
            .collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        v
    }

    /// Outgoing references from a given entity.
    pub fn references_from(&self, id: &EntityId) -> Vec<&Reference> {
        self.references.iter().filter(|r| &r.from == id).collect()
    }

    /// Reverse lookup: all references whose `to` is the given id.
    pub fn references_to(&self, id: &EntityId) -> Vec<&Reference> {
        self.references.iter().filter(|r| &r.to == id).collect()
    }

    /// Iterate every reference (used by validation / tests).
    pub fn iter_references(&self) -> impl Iterator<Item = &Reference> {
        self.references.iter()
    }

    /// Run schema + path + reference validation across the whole graph.
    ///
    /// `Ok(())` if every entity is well-formed. Otherwise a list of
    /// [`ValidationError`]s with paths / ids for the caller to surface.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errs = Vec::new();

        // 1. id uniqueness across the whole story
        let mut by_id: std::collections::HashMap<&str, Vec<&Path>> =
            std::collections::HashMap::new();
        for e in &self.entities {
            by_id
                .entry(e.id.as_str())
                .or_default()
                .push(e.path.as_path());
        }
        let mut dup_ids: Vec<(String, Vec<PathBuf>)> = Vec::new();
        for (id, paths) in by_id {
            if paths.len() > 1 {
                dup_ids.push((id.to_string(), paths.into_iter().map(Path::to_path_buf).collect()));
            }
        }
        // Sort for deterministic error ordering.
        dup_ids.sort_by(|a, b| a.0.cmp(&b.0));
        for (id, paths) in dup_ids {
            errs.push(ValidationError::DuplicateId { id, paths });
        }

        // 2. per-entity: frontmatter checks
        for e in &self.entities {
            let fm = &e.frontmatter;

            if frontmatter::id_of(fm).is_none() {
                errs.push(ValidationError::MissingField {
                    path: e.path.clone(),
                    field: "id",
                });
            }

            let fm_slug = frontmatter::slug_of(fm).unwrap_or_default();
            if fm_slug.is_empty() {
                errs.push(ValidationError::MissingField {
                    path: e.path.clone(),
                    field: "slug",
                });
            } else if fm_slug != e.slug {
                errs.push(ValidationError::SlugMismatch {
                    path: e.path.clone(),
                    expected: fm_slug,
                    actual: e.slug.clone(),
                });
            }
        }

        // 3. references: every `to` must exist as an entity
        let entity_ids: std::collections::HashSet<&str> =
            self.entities.iter().map(|e| e.id.as_str()).collect();
        let mut broken: Vec<ValidationError> = Vec::new();
        for r in &self.references {
            if !entity_ids.contains(r.to.as_str()) {
                broken.push(ValidationError::BrokenReference {
                    path: self
                        .entities
                        .iter()
                        .find(|e| e.id == r.from)
                        .map(|e| e.path.clone())
                        .unwrap_or_else(|| PathBuf::from("<unknown>")),
                    kind: r.kind.to_string(),
                    missing: r.to.as_str().to_string(),
                    available: self.entities.iter().map(|e| e.slug.clone()).collect(),
                });
            }
        }
        broken.sort_by(|a, b| match (a, b) {
            (
                ValidationError::BrokenReference { path: p1, missing: m1, .. },
                ValidationError::BrokenReference { path: p2, missing: m2, .. },
            ) => p1.cmp(p2).then(m1.cmp(m2)),
            _ => std::cmp::Ordering::Equal,
        });
        errs.extend(broken);

        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }
}

/// Recursive directory walker that yields `(relative_path, absolute_path)`
/// for every regular file under `root`. We hand-roll this instead of using
/// the `walkdir` crate to keep dependencies minimal.
fn walk(
    base: &Path,
    visit: &mut dyn FnMut(&Path, &Path) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    visit_dir(base, base, visit)
}

fn visit_dir(
    base: &Path,
    dir: &Path,
    visit: &mut dyn FnMut(&Path, &Path) -> Result<(), CoreError>,
) -> Result<(), CoreError> {
    let entries = fs::read_dir(dir).map_err(|source| CoreError::Walk {
        root: base.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CoreError::Walk {
            root: base.to_path_buf(),
            source,
        })?;
        let abs = entry.path();
        let file_type = entry.file_type().map_err(|source| CoreError::Walk {
            root: base.to_path_buf(),
            source,
        })?;
        let rel = abs
            .strip_prefix(base)
            .map_err(|_| CoreError::Walk {
                root: base.to_path_buf(),
                source: std::io::Error::new(std::io::ErrorKind::Other, "path outside root"),
            })?
            .to_path_buf();

        if file_type.is_dir() {
            if rel
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            {
                continue;
            }
            let last = rel
                .components()
                .last()
                .map(|c| c.as_os_str().to_string_lossy().into_owned());
            if matches!(last.as_deref(), Some("assets") | Some("references")) {
                continue;
            }
            visit_dir(base, &abs, visit)?;
        } else if file_type.is_file() {
            if rel
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
            {
                continue;
            }
            visit(&rel, &abs)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(p: &Path, body: &str) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }

    fn fixture_story(dir: &Path) {
        write(
            &dir.join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\ntitle: My Story\n---\n# body\n",
        );
        write(
            &dir.join("bible/world.md"),
            "---\nid: bible_world\ntype: bible\nslug: world\n---\n",
        );
        write(
            &dir.join("characters/lin-mo/character.md"),
            "---\nid: char_lin_mo\ntype: character\nslug: lin-mo\n---\n",
        );
        write(
            &dir.join("characters/su-yan/character.md"),
            "---\nid: char_su_yan\ntype: character\nslug: su-yan\n---\n",
        );
        write(
            &dir.join("environments/laboratory/environment.md"),
            "---\nid: env_lab\ntype: environment\nslug: laboratory\n---\n",
        );
        write(
            &dir.join("props/energy-core.md"),
            "---\nid: prop_core\ntype: prop\nslug: energy-core\n---\n",
        );
        write(
            &dir.join("chapters/001-the-beginning/chapter.md"),
            "---\nid: chap_001\ntype: chapter\nslug: 001-the-beginning\norder: 1\n---\n",
        );
        write(
            &dir.join("chapters/001-the-beginning/scenes/001-laboratory.md"),
            "---\nid: scene_001\ntype: scene\nslug: 001-laboratory\norder: 1\n\
            characters: [lin-mo, su-yan]\nenvironment: laboratory\nprops: [energy-core]\n---\n",
        );
    }

    #[test]
    fn load_builds_full_graph() {
        let tmp = tempdir().unwrap();
        fixture_story(tmp.path());
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        assert_eq!(g.story.id.as_str(), "s1");
        assert_eq!(g.entities_by_type(EntityType::Character).len(), 2);
        assert_eq!(g.entities_by_type(EntityType::Environment).len(), 1);
        assert_eq!(g.entities_by_type(EntityType::Prop).len(), 1);
        assert_eq!(g.entities_by_type(EntityType::Bible).len(), 1);
        assert_eq!(g.entities_by_type(EntityType::Chapter).len(), 1);
        assert_eq!(g.entities_by_type(EntityType::Scene).len(), 1);
        assert_eq!(g.entities_by_type(EntityType::Shot).len(), 0);
        assert_eq!(g.references.len(), 4);
    }

    #[test]
    fn validate_ok_on_valid_fixture() {
        let tmp = tempdir().unwrap();
        fixture_story(tmp.path());
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        let r = g.validate();
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn validate_detects_broken_reference() {
        let tmp = tempdir().unwrap();
        fixture_story(tmp.path());
        write(
            &tmp.path().join("chapters/001-the-beginning/scenes/001-laboratory.md"),
            "---\nid: scene_001\ntype: scene\nslug: 001-laboratory\ncharacters: [ghost]\n---\n",
        );
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        let errs = g.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::BrokenReference { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn validate_detects_slug_mismatch() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\n---\n",
        );
        write(
            &tmp.path().join("characters/lin-mo/character.md"),
            "---\nid: char_lin_mo\ntype: character\nslug: NOT-MATCH\n---\n",
        );
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        let errs = g.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::SlugMismatch { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn validate_detects_duplicate_id() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\n---\n",
        );
        write(
            &tmp.path().join("characters/lin-mo/character.md"),
            "---\nid: dup\ntype: character\nslug: lin-mo\n---\n",
        );
        write(
            &tmp.path().join("characters/su-yan/character.md"),
            "---\nid: dup\ntype: character\nslug: su-yan\n---\n",
        );
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        let errs = g.validate().unwrap_err();
        assert!(
            errs.iter().any(|e| matches!(e, ValidationError::DuplicateId { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn references_to_reverse_works() {
        let tmp = tempdir().unwrap();
        fixture_story(tmp.path());
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        let lin = EntityId::new("char_lin_mo");
        let refs = g.references_to(&lin);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].from, EntityId::new("scene_001"));
    }

    #[test]
    fn load_skips_assets_and_references_dirs() {
        let tmp = tempdir().unwrap();
        write(
            &tmp.path().join("story.md"),
            "---\nid: s1\ntype: story\nslug: story\n---\n",
        );
        write(
            &tmp.path().join("characters/lin-mo/character.md"),
            "---\nid: char_lin_mo\ntype: character\nslug: lin-mo\n---\n",
        );
        // Should not be picked up: lives under characters/<slug>/references/
        write(
            &tmp.path().join("characters/lin-mo/references/age-28.md"),
            "---\nid: should_be_ignored\ntype: unknown\nslug: ignored\n---\n",
        );
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g = StoryGraph::load(&root).unwrap();
        assert_eq!(g.entities.len(), 1);
    }

    #[test]
    fn load_is_idempotent() {
        let tmp = tempdir().unwrap();
        fixture_story(tmp.path());
        let root = StoryRoot::new(tmp.path()).unwrap();
        let g1 = StoryGraph::load(&root).unwrap();
        let g2 = StoryGraph::load(&root).unwrap();
        assert_eq!(g1.entities.len(), g2.entities.len());
        assert_eq!(g1.references.len(), g2.references.len());
        for (a, b) in g1.entities.iter().zip(g2.entities.iter()) {
            assert_eq!(a.path, b.path);
            assert_eq!(a.slug, b.slug);
            assert_eq!(a.id, b.id);
        }
    }
}