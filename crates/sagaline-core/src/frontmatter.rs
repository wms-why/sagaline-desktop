//! Typed front matter accessors.
//!
//! `ParsedEntity::frontmatter` is stored as a raw `serde_yaml::Value` so we
//! can carry entity-type-specific fields without an explosion of struct
//! types. This module offers per-entity accessors that pull out the well-
//! known fields and turn them into `Vec<EntityId>` etc.
//!
//! All accessors are tolerant: a missing or wrong-shaped field returns
//! `None` (or empty for collections). Type / slug / id consistency is
//! enforced by [`crate::graph::StoryGraph::validate`], not here.

use serde_yaml::Value;

use crate::entity::{EntityId, ParsedEntity, Reference, ReferenceKind};

/// Returns the `id` field, if present and a non-empty string.
pub fn id_of(fm: &Value) -> Option<String> {
    fm.get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Returns the `slug` field, if present and a non-empty string.
pub fn slug_of(fm: &Value) -> Option<String> {
    fm.get("slug")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
/// Returns the `title` field (story + chapter + entity-name fallbacks).
pub fn title_of(fm: &Value) -> Option<String> {
    fm.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Pulls the references out of a scene's front matter and resolves them
/// against the supplied entity list.
///
/// Scene schema:
///
/// ```yaml
/// characters: [lin-mo, su-yan]   # list of entity ids or slugs
/// environment: laboratory         # single entity id or slug
/// props: [energy-core]            # list of entity ids or slugs
/// ```
///
/// Each listed string is matched first against an entity's slug, then its
/// id. On a hit the emitted `Reference.to` is the **entity's actual id**,
/// not the literal input string — so consumers can join against the entity
/// set without a second lookup pass.
pub fn scene_references(
    fm: &Value,
    from: &EntityId,
    entities: &[ParsedEntity],
) -> Vec<Reference> {
    let mut out = Vec::new();

    if let Some(list) = fm.get("characters").and_then(Value::as_sequence) {
        for v in list {
            if let Some(s) = v.as_str() {
                let to = match resolve(s, entities) {
                    Some(e) => e.id.clone(),
                    None => EntityId::new(s),
                };
                out.push(Reference {
                    from: from.clone(),
                    to,
                    kind: ReferenceKind::Character,
                });
            }
        }
    }

    if let Some(s) = fm.get("environment").and_then(Value::as_str) {
        let to = match resolve(s, entities) {
            Some(e) => e.id.clone(),
            None => EntityId::new(s),
        };
        out.push(Reference {
            from: from.clone(),
            to,
            kind: ReferenceKind::Environment,
        });
    }

    if let Some(list) = fm.get("props").and_then(Value::as_sequence) {
        for v in list {
            if let Some(s) = v.as_str() {
                let to = match resolve(s, entities) {
                    Some(e) => e.id.clone(),
                    None => EntityId::new(s),
                };
                out.push(Reference {
                    from: from.clone(),
                    to,
                    kind: ReferenceKind::Prop,
                });
            }
        }
    }

    out
}

/// Resolves a reference string against the entity list. Slug wins on tie
/// (slug uniqueness is enforced by path layout).
fn resolve<'a>(s: &str, entities: &'a [ParsedEntity]) -> Option<&'a ParsedEntity> {
    entities
        .iter()
        .find(|e| e.slug == s)
        .or_else(|| entities.iter().find(|e| e.id.as_str() == s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str;

    fn fm(s: &str) -> Value {
        from_str(s).unwrap()
    }

    #[test]
    fn id_of_basic() {
        assert_eq!(id_of(&fm("id: char_x\n")), Some("char_x".into()));
        assert_eq!(id_of(&fm("id: \"\"\n")), None);
        assert_eq!(id_of(&fm("foo: bar\n")), None);
    }

    #[test]
    fn slug_of_basic() {
        assert_eq!(slug_of(&fm("slug: lin-mo\n")), Some("lin-mo".into()));
        assert_eq!(slug_of(&fm("slug: \"\"\n")), None);
    }

    #[test]
    fn title_of_basic() {
        assert_eq!(
            title_of(&fm("title: Lab scene\n")),
            Some("Lab scene".into())
        );
        assert_eq!(title_of(&fm("title: \"\"\n")), None);
    }

    fn character(id: &str, slug: &str) -> ParsedEntity {
        ParsedEntity {
            id: EntityId::new(id),
            type_: crate::entity::EntityType::Character,
            slug: slug.into(),
            path: Default::default(),
            frontmatter: Value::Null,
            body: String::new(),
        }
    }

    fn environment(id: &str, slug: &str) -> ParsedEntity {
        ParsedEntity {
            id: EntityId::new(id),
            type_: crate::entity::EntityType::Environment,
            slug: slug.into(),
            path: Default::default(),
            frontmatter: Value::Null,
            body: String::new(),
        }
    }

    fn prop(id: &str, slug: &str) -> ParsedEntity {
        ParsedEntity {
            id: EntityId::new(id),
            type_: crate::entity::EntityType::Prop,
            slug: slug.into(),
            path: Default::default(),
            frontmatter: Value::Null,
            body: String::new(),
        }
    }

    #[test]
    fn scene_references_resolves_via_slug_to_entity_id() {
        let v = fm(
            "characters: [lin-mo]\nenvironment: laboratory\nprops: [energy-core]\n",
        );
        let from = EntityId::new("scene_001");
        let ents = vec![
            character("char_lin_mo", "lin-mo"),
            environment("env_lab", "laboratory"),
            prop("prop_core", "energy-core"),
        ];
        let refs = scene_references(&v, &from, &ents);
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].to.as_str(), "char_lin_mo");
        assert_eq!(refs[1].to.as_str(), "env_lab");
        assert_eq!(refs[2].to.as_str(), "prop_core");
    }

    #[test]
    fn scene_references_resolves_via_id_when_slug_missing() {
        let v = fm("characters: [char_lin_mo]\n");
        let from = EntityId::new("scene_x");
        let ents = vec![character("char_lin_mo", "lin-mo")];
        let refs = scene_references(&v, &from, &ents);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].to.as_str(), "char_lin_mo");
    }

    #[test]
    fn scene_references_drops_unresolved() {
        let v = fm("characters: [lin-mo, ghost]\n");
        let from = EntityId::new("scene_x");
        let ents = vec![character("char_lin_mo", "lin-mo")];
        let refs = scene_references(&v, &from, &ents);
        // Unresolved strings are still emitted so validate can flag them.
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].to.as_str(), "char_lin_mo");
        assert_eq!(refs[1].to.as_str(), "ghost");
    }

    #[test]
    fn scene_references_kinds_are_correct() {
        let v = fm(
            "characters: [lin-mo]\nenvironment: laboratory\nprops: [energy-core]\n",
        );
        let from = EntityId::new("scene_x");
        let ents = vec![
            character("char_lin_mo", "lin-mo"),
            environment("env_lab", "laboratory"),
            prop("prop_core", "energy-core"),
        ];
        let refs = scene_references(&v, &from, &ents);
        assert_eq!(refs[0].kind, ReferenceKind::Character);
        assert_eq!(refs[1].kind, ReferenceKind::Environment);
        assert_eq!(refs[2].kind, ReferenceKind::Prop);
    }
}