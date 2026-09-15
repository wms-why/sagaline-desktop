//! Prompt assembly — turn the agent's loaded context into the text the
//! LLM will see.
//!
//! In this scaffold the strings are only used by tests and by the
//! canned PLAN / REFLECT responses; the LLM integration is a later
//! phase. Keeping the assembly in one place means the eventual
//! switch is a swap of the prompt source, not a redesign.

use sagaline_core::ParsedEntity;

use crate::event::ResolvedContext;

/// Resolve the entity references in a scene's frontmatter into a
/// [`ResolvedContext`]. Used by [`Agent::run_loop`] to populate the
/// OBSERVE event.
///
/// Pure function: takes the entities already loaded into the
/// `StoryGraph`. The agent does not re-read files here.
pub fn resolve_context(
    scene: &ParsedEntity,
    entities: &[ParsedEntity],
) -> ResolvedContext {
    use sagaline_core::frontmatter;

    let fm = &scene.frontmatter;
    let refs = frontmatter::scene_references(fm, &scene.id, entities);

    let mut characters = Vec::new();
    let mut environment = None;
    let mut props = Vec::new();

    for r in refs {
        match r.kind {
            sagaline_core::ReferenceKind::Character => characters.push(r.to.to_string()),
            sagaline_core::ReferenceKind::Environment => {
                environment.get_or_insert_with(|| r.to.to_string());
            }
            sagaline_core::ReferenceKind::Prop => props.push(r.to.to_string()),
        }
    }

    ResolvedContext {
        characters,
        environment,
        props,
    }
}
