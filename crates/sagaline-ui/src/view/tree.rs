//! Tree-item construction from a [`StoryGraph`].
//!
//! See `mod.rs` for the broader module overview.

use gpui_base::TreeItem;
use sagaline_core::{EntityType as StoryEntityType, ParsedEntity, StoryGraph};

/// Build the `TreeItem` hierarchy from a `StoryGraph`.
///
/// The five top-level groups are domain types (Bible / Characters /
/// Environments / Props / Chapters). Each chapter is followed
/// directly by its scenes — there is no nested "Scenes in <slug>"
/// folder, no leading-whitespace indenting (the tree widget handles
/// indentation), and no slug in the label.
pub(super) fn build_tree_items(graph: Option<&StoryGraph>) -> Vec<TreeItem> {
    let Some(g) = graph else {
        return Vec::new();
    };

    let mut root_items: Vec<TreeItem> = Vec::new();
    root_items.push(TreeItem::new(
        g.story.id.0.clone(),
        g.story
            .title()
            .unwrap_or_else(|| g.story.id.0.clone()),
    ));

    let sections: &[(StoryEntityType, &str)] = &[
        (StoryEntityType::Bible, "Bible"),
        (StoryEntityType::Character, "Characters"),
        (StoryEntityType::Environment, "Environments"),
        (StoryEntityType::Prop, "Props"),
        (StoryEntityType::Chapter, "Chapters"),
    ];

    for (ty, label) in sections.iter() {
        let entries = g.entities_by_type(*ty);
        if entries.is_empty() {
            continue;
        }
        let group = match ty {
            StoryEntityType::Chapter => {
                let mut group = TreeItem::new(format!("__group_{label}"), label.to_string())
                    .expanded(true);
                for ch in &entries {
                    group = group.child(TreeItem::new(ch.id.0.clone(), entity_label(ch)));
                    let scenes_under = g.entities_by_type(StoryEntityType::Scene);
                    for sc in scenes_under
                        .into_iter()
                        .filter(|s| s.path.starts_with(format!("chapters/{}/", ch.slug).as_str()))
                    {
                        group = group.child(TreeItem::new(sc.id.0.clone(), entity_label(sc)));
                    }
                }
                group
            }
            _ => {
                let mut group = TreeItem::new(format!("__group_{label}"), label.to_string())
                    .expanded(true);
                for e in &entries {
                    group = group.child(TreeItem::new(e.id.0.clone(), entity_label(e)));
                }
                group
            }
        };
        root_items.push(group);
    }

    root_items
}

pub(super) fn entity_label(e: &ParsedEntity) -> String {
    e.title().unwrap_or_else(|| e.id.0.clone())
}