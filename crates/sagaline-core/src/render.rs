//! UI-friendly projection of a [`ParsedEntity`].
//!
//! The desktop preview pane never shows the raw YAML front matter
//! or the Markdown body to the user. Instead it renders a small
//! `(label, value)` definition list tailored to the entity type.
//! This module owns the per-type field selection so the UI is
//! presentation-only.

use serde_yaml::Value;

use crate::entity::{EntityType, ParsedEntity};

/// Field rows the UI should render for this entity, in display
/// order. Each row is `(label, value)`; the UI is responsible for
/// styling and layout.
pub fn render_preview_lines(e: &ParsedEntity) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();

    rows.push(("Type", e.type_.as_str().to_string()));
    rows.push(("Identifier", e.id.0.clone()));

    if let Some(title) = e.title() {
        rows.push(("Title", title));
    }

    if let Some(summary) = e.summary() {
        rows.push(("Summary", summary));
    }

    match e.type_ {
        EntityType::Scene => {
            for_each_string_list(&e.frontmatter, "characters", |s| {
                rows.push(("Character", s.to_string()));
            });
            for_each_string_list(&e.frontmatter, "props", |s| {
                rows.push(("Prop", s.to_string()));
            });
            if let Some(env) = str_field(&e.frontmatter, "environment") {
                rows.push(("Environment", env));
            }
        }
        EntityType::Shot => {
            if let Some(v) = str_field(&e.frontmatter, "status") {
                rows.push(("Status", v));
            }
            if let Some(v) = str_field(&e.frontmatter, "duration_seconds") {
                rows.push(("Duration", format!("{v}s")));
            }
            if let Some(v) = str_field(&e.frontmatter, "camera") {
                rows.push(("Camera", v));
            }
            if let Some(v) = str_field(&e.frontmatter, "mood") {
                rows.push(("Mood", v));
            }
        }
        EntityType::Character
        | EntityType::Environment
        | EntityType::Prop
        | EntityType::Bible
        | EntityType::Chapter
        | EntityType::Story => {
            // Already covered by the universal Title / Summary rows.
        }
    }

    rows
}

fn str_field(fm: &Value, key: &str) -> Option<String> {
    fm.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn for_each_string_list(fm: &Value, key: &str, mut visit: impl FnMut(&str)) {
    if let Some(seq) = fm.get(key).and_then(Value::as_sequence) {
        for v in seq {
            if let Some(s) = v.as_str() {
                visit(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{EntityId, EntityType};
    use std::path::PathBuf;

    fn entity(ty: EntityType, front: &str) -> ParsedEntity {
        let fm: Value = serde_yaml::from_str(front).unwrap_or(Value::Null);
        ParsedEntity {
            id: EntityId::new("scene_001"),
            type_: ty,
            slug: "scene-001".into(),
            path: PathBuf::from("scenes/scene-001.md"),
            frontmatter: fm,
            body: "The first line of the body.\nA second line.".into(),
        }
    }

    #[test]
    fn scene_rows_include_refs() {
        let e = entity(
            EntityType::Scene,
            r#"title: Opening
characters: [lin-mo, su-yan]
environment: laboratory
props: [energy-core]"#,
        );
        let rows = render_preview_lines(&e);
        let labels: Vec<&str> = rows.iter().map(|(k, _)| *k).collect();
        assert!(labels.contains(&"Title"));
        assert!(labels.contains(&"Environment"));
        assert!(labels.contains(&"Character"));
        assert!(labels.contains(&"Prop"));
    }

    #[test]
    fn shot_rows_include_status() {
        let e = entity(
            EntityType::Shot,
            r#"title: Reveal
status: succeeded
duration_seconds: "3"
camera: close_up
mood: tense"#,
        );
        let rows = render_preview_lines(&e);
        assert!(rows.iter().any(|(k, v)| *k == "Status" && v == "succeeded"));
        assert!(rows.iter().any(|(k, v)| *k == "Duration" && v == "3s"));
        assert!(rows.iter().any(|(k, v)| *k == "Camera" && v == "close_up"));
    }

    #[test]
    fn universal_rows_for_character() {
        let e = entity(
            EntityType::Character,
            r#"title: Lin Mo"#,
        );
        let rows = render_preview_lines(&e);
        assert_eq!(rows[0].0, "Type");
        assert_eq!(rows[0].1, "character");
        assert!(rows.iter().any(|(k, v)| *k == "Title" && v == "Lin Mo"));
        assert!(rows.iter().any(|(k, v)| *k == "Summary" && v.contains("first line")));
    }
}
