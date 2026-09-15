//! Path ↔ entity mapping.
//!
//! The directory layout under a story root is fixed:
//!
//! ```text
//! story.md
//! bible/<slug>.md
//! characters/<slug>/character.md
//! environments/<slug>/environment.md
//! props/<slug>.md
//! chapters/<NNN-slug>/chapter.md
//! chapters/<NNN-slug>/scenes/<NNN-slug>.md
//! chapters/<NNN-slug>/scenes/<NNN-slug>/shots/<NNN-slug>.md
//! ```
//!
//! This module decides what entity type a given relative path represents and
//! extracts its slug.

use std::path::{Path, PathBuf};

use crate::entity::EntityType;

/// `path` must be relative to the story root. Returns `None` if the path
/// doesn't match any known entity layout.
pub fn classify(rel: &Path) -> Option<(EntityType, String)> {
    let segments: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

    match segments.as_slice() {
        [file] if file == "story.md" => Some((EntityType::Story, "story".into())),

        [dir, file] if dir == "bible" && file.ends_with(".md") => {
            Some((EntityType::Bible, strip_md(file)))
        }

        [dir, file] if dir == "props" && file.ends_with(".md") => {
            Some((EntityType::Prop, strip_md(file)))
        }

        [dir, sub, file]
            if dir == "characters"
                && sub != "references"
                && file == "character.md" =>
        {
            Some((EntityType::Character, sub.clone()))
        }

        [dir, sub, file]
            if dir == "environments"
                && sub != "references"
                && file == "environment.md" =>
        {
            Some((EntityType::Environment, sub.clone()))
        }

        [dir, _chapter, file] if dir == "chapters" && file == "chapter.md" => {
            Some((EntityType::Chapter, _chapter.clone()))
        }

        [dir, _chapter, scenes, file]
            if dir == "chapters" && scenes == "scenes" && file.ends_with(".md") =>
        {
            Some((EntityType::Scene, strip_md(file)))
        }

        [dir, _chapter, scenes, _scene, shots, file]
            if dir == "chapters"
                && scenes == "scenes"
                && shots == "shots"
                && file.ends_with(".md") =>
        {
            Some((EntityType::Shot, strip_md(file)))
        }

        _ => None,
    }
}

fn strip_md(file: &str) -> String {
    file.trim_end_matches(".md").to_string()
}

/// Reconstruct the canonical relative path for an entity of the given type
/// and slug. Inverse of [`classify`] for the well-defined shapes.
///
/// Chapter / scene / shot cannot be reconstructed from their own slug alone
/// — they require chapter context. We return a "shape-only" canonical path
/// that still classifies back to the right type + slug; the caller fills in
/// chapter context where it matters.
pub fn canonical(type_: EntityType, slug: &str) -> PathBuf {
    use EntityType::*;
    let mut p = PathBuf::new();
    match type_ {
        Story => p.push("story.md"),
        Bible => {
            p.push("bible");
            p.push(format!("{slug}.md"));
        }
        Character => {
            p.push("characters");
            p.push(slug);
            p.push("character.md");
        }
        Environment => {
            p.push("environments");
            p.push(slug);
            p.push("environment.md");
        }
        Prop => {
            p.push("props");
            p.push(format!("{slug}.md"));
        }
        Chapter => {
            p.push("chapters");
            p.push(slug);
            p.push("chapter.md");
        }
        Scene => {
            p.push("chapters");
            p.push("_chapter");
            p.push("scenes");
            p.push(format!("{slug}.md"));
        }
        Shot => {
            p.push("chapters");
            p.push("_chapter");
            p.push("scenes");
            p.push("_scene");
            p.push("shots");
            p.push(format!("{slug}.md"));
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityType::*;

    #[test]
    fn story_file() {
        assert_eq!(
            classify(Path::new("story.md")),
            Some((Story, "story".into()))
        );
    }

    #[test]
    fn bible() {
        assert_eq!(
            classify(Path::new("bible/world.md")),
            Some((Bible, "world".into()))
        );
    }

    #[test]
    fn character() {
        assert_eq!(
            classify(Path::new("characters/lin-mo/character.md")),
            Some((Character, "lin-mo".into()))
        );
    }

    #[test]
    fn environment() {
        assert_eq!(
            classify(Path::new("environments/laboratory/environment.md")),
            Some((Environment, "laboratory".into()))
        );
    }

    #[test]
    fn prop() {
        assert_eq!(
            classify(Path::new("props/energy-core.md")),
            Some((Prop, "energy-core".into()))
        );
    }

    #[test]
    fn chapter() {
        assert_eq!(
            classify(Path::new("chapters/001-the-beginning/chapter.md")),
            Some((Chapter, "001-the-beginning".into()))
        );
    }

    #[test]
    fn scene() {
        assert_eq!(
            classify(Path::new("chapters/001-the-beginning/scenes/001-laboratory.md")),
            Some((Scene, "001-laboratory".into()))
        );
    }

    #[test]
    fn shot() {
        assert_eq!(
            classify(Path::new(
                "chapters/001-the-beginning/scenes/001-laboratory/shots/001-opening.md"
            )),
            Some((Shot, "001-opening".into()))
        );
    }

    #[test]
    fn character_reference_dir_is_ignored() {
        assert!(classify(Path::new("characters/lin-mo/references/age-28.png")).is_none());
    }

    #[test]
    fn unknown_layout_is_none() {
        assert!(classify(Path::new("random/garbage.md")).is_none());
        assert!(classify(Path::new("bible/world/notes.md")).is_none());
        assert!(classify(Path::new("characters/lin-mo/notes.md")).is_none());
    }

    #[test]
    fn canonical_round_trips_classify() {
        for (ty, slug) in [
            (Story, "story"),
            (Bible, "world"),
            (Character, "lin-mo"),
            (Environment, "laboratory"),
            (Prop, "energy-core"),
            (Chapter, "001-the-beginning"),
            (Scene, "001-laboratory"),
            (Shot, "001-opening"),
        ] {
            let p = canonical(ty, slug);
            assert_eq!(classify(&p), Some((ty, slug.into())), "{ty:?}/{slug}");
        }
    }
}