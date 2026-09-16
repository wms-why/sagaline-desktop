//! End-to-end smoke test for the agent loop.
//!
//! Spins up a temp story directory with one character, one environment,
//! one scene, runs the agent, and asserts the event stream shape.

use std::fs;
use std::path::PathBuf;

use sagaline_agent::{Agent, AgentEvent, EventSink, StepOutcome, Tool};
use sagaline_agent::tools::ReadFileTool;
use sagaline_core::StoryRoot;
use tempfile::tempdir;

fn write(p: &std::path::Path, body: &str) {
    fs::write(p, body).unwrap();
}

fn build_story(root: &std::path::Path) {
    write(
        &root.join("story.md"),
        "---\nid: story_demo\ntype: story\nslug: demo\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\ndemo\n",
    );
    fs::create_dir_all(root.join("characters/lin-mo")).unwrap();
    write(
        &root.join("characters/lin-mo/character.md"),
        "---\nid: character_lin-mo\ntype: character\nslug: lin-mo\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n林默\n",
    );
    fs::create_dir_all(root.join("environments/laboratory")).unwrap();
    write(
        &root.join("environments/laboratory/environment.md"),
        "---\nid: environment_laboratory\ntype: environment\nslug: laboratory\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n实验室\n",
    );
    fs::create_dir_all(root.join("chapters/001-start/scenes")).unwrap();
    write(
        &root.join("chapters/001-start/chapter.md"),
        "---\nid: chapter_001-start\ntype: chapter\nslug: 001-start\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n第一章\n",
    );
    write(
        &root.join("chapters/001-start/scenes/001-intro.md"),
        "---\nid: scene_001_intro\ntype: scene\nslug: 001-intro\ncharacters: [lin-mo]\nenvironment: laboratory\ncreated_at: 2026-09-15T00:00:00Z\nupdated_at: 2026-09-15T00:00:00Z\n---\n林默走进实验室\n",
    );
}

#[derive(Default)]
struct Collect {
    events: Vec<AgentEvent>,
}

impl EventSink for Collect {
    fn emit(&mut self, e: AgentEvent) {
        self.events.push(e);
    }
}

#[tokio::test]
async fn loop_emits_one_full_pass_per_scene() {
    let dir = tempdir().unwrap();
    build_story(dir.path());

    // Sanity: core can load the story.
    let root = StoryRoot::new(dir.path()).expect("story root");
    let graph = sagaline_core::StoryGraph::load(&root).expect("graph");
    assert!(graph.validate().is_ok(), "fixture must validate");
    let mut agent = Agent::new();
    agent.tools_mut().register(ReadFileTool::new(dir.path()));
    let mut sink = Collect::default();
    let outcome = agent.run(dir.path(), &mut sink).await.expect("run");

    assert_eq!(outcome, StepOutcome::Complete);

    // Event shape: StepStart, Observe, Plan, Act, Reflect, ... Done.
    let kinds: Vec<&'static str> = sink
        .events
        .iter()
        .map(|e| match e {
            AgentEvent::StepStart { .. } => "StepStart",
            AgentEvent::Observe { .. } => "Observe",
            AgentEvent::Plan { .. } => "Plan",
            AgentEvent::Act { .. } => "Act",
            AgentEvent::Reflect { .. } => "Reflect",
            AgentEvent::Done { .. } => "Done",
        })
        .collect();

    assert!(kinds.starts_with(&["StepStart", "Observe", "Plan", "Act", "Reflect"]));
    assert_eq!(*kinds.last().unwrap(), "Done");

    // The single Observe must carry our resolved context.
    let observe = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Observe { resolved, .. } => Some(resolved),
            _ => None,
        })
        .expect("observe event");
    assert_eq!(observe.characters, vec!["character_lin-mo"]);
    assert_eq!(observe.environment.as_deref(), Some("environment_laboratory"));

    // The Act event invoked the read_file tool.
    let act = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Act { tool, result_summary, .. } => Some((tool, result_summary)),
            _ => None,
        })
        .expect("act event");
    assert_eq!(act.0, "read_file");
    assert!(act.1.ends_with(" bytes"), "summary should report bytes: {act:?}");

    // Reflect must have passed (the read succeeded).
    let reflect_ok = sink.events.iter().any(
        |e| matches!(e, AgentEvent::Reflect { validation_ok: true, .. }),
    );
    assert!(reflect_ok, "expected at least one passing reflect");
}

#[test]
fn tool_descriptor_advertises_json_schema() {
    let t = ReadFileTool::new(PathBuf::new());
    let d = t.descriptor();
    assert_eq!(d.name, "read_file");
    let json = serde_json::to_string(&d.parameters).unwrap();
    assert!(json.contains("\"path\""), "schema missing `path`: {json}");
}
