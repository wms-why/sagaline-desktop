//! End-to-end smoke test for the agent loop.
//!
//! Phase 2.5: spins up an in-memory SQLite world with one
//! character, one environment, one chapter, one scene, runs
//! the agent, and asserts the event stream shape.

use std::sync::Arc;

use sagaline_agent::tools::{
    AssignCharacterToSceneTool, AssignEnvironmentToSceneTool, CreateChapterTool,
    CreateCharacterTool, CreateEnvironmentTool, CreateSceneTool, ValidateWorldTool,
};
use sagaline_agent::{Agent, AgentConfig, AgentEvent, EventSink, StepOutcome, ToolRegistry};
use sagaline_store::repo::{NewChapter, NewCharacter, NewEnvironment, NewScene, NewStory};
use sagaline_store::World;

fn build_world() -> (Arc<World>, String) {
    let world = Arc::new(World::in_memory().expect("in-memory world"));
    let story = world
        .stories()
        .create(NewStory {
            slug: "demo",
            title: "Demo",
            summary: "",
        })
        .unwrap();
    world
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "lin-mo",
            name: "Lin Mo",
            occupation: None,
            bio: "",
        })
        .unwrap();
    world
        .environments()
        .create(NewEnvironment {
            story_id: &story.id,
            slug: "laboratory",
            name: "Laboratory",
            description: "",
        })
        .unwrap();
    let chapter = world
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "001-start",
            ordinal: 1,
            title: "Start",
            synopsis: "",
        })
        .unwrap();
    let scene = world
        .scenes()
        .create_scene(NewScene {
            chapter_id: &chapter.id,
            slug: "001-intro",
            ordinal: 1,
            title: "Intro",
            synopsis: "Lin Mo walks into the lab.",
        })
        .unwrap();
    let lin_mo = world
        .characters()
        .list_for_story(&story.id)
        .unwrap()
        .into_iter()
        .find(|c| c.slug == "lin-mo")
        .unwrap();
    let lab = world
        .environments()
        .list_for_story(&story.id)
        .unwrap()
        .into_iter()
        .find(|e| e.slug == "laboratory")
        .unwrap();
    // Manually assign the scene's character + environment.
    {
        let conn = world.conn().unwrap();
        conn.execute(
            "INSERT INTO scene_characters (scene_id, character_id)
             VALUES (?1, ?2)",
            rusqlite::params![scene.id, lin_mo.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO scene_environments (scene_id, environment_id)
             VALUES (?1, ?2)",
            rusqlite::params![scene.id, lab.id],
        )
        .unwrap();
    }
    (world, story.id)
}

fn build_agent(world: Arc<World>) -> Agent {
    let mut agent = Agent::with_config(AgentConfig::default());
    agent
        .tools_mut()
        .register(ValidateWorldTool::new(world.clone()));
    agent
        .tools_mut()
        .register(CreateCharacterTool::new(world.clone()));
    agent
        .tools_mut()
        .register(CreateEnvironmentTool::new(world.clone()));
    agent
        .tools_mut()
        .register(CreateChapterTool::new(world.clone()));
    agent
        .tools_mut()
        .register(CreateSceneTool::new(world.clone()));
    agent
        .tools_mut()
        .register(AssignCharacterToSceneTool::new(world.clone()));
    agent
        .tools_mut()
        .register(AssignEnvironmentToSceneTool::new(world.clone()));
    agent
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
    let (world, story_id) = build_world();
    let agent = build_agent(world.clone());
    let mut sink = Collect::default();
    let outcome = agent.run(world, &story_id, &mut sink).await.expect("run");

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
    assert_eq!(observe.characters.len(), 1);
    assert!(observe.environment.is_some());

    // The Act event invoked validate_world.
    let act = sink
        .events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Act {
                tool,
                result_summary,
                ..
            } => Some((tool, result_summary)),
            _ => None,
        })
        .expect("act event");
    assert_eq!(act.0, "validate_world");
    assert_eq!(
        act.1, "ok",
        "validate_world should report ok for a clean story"
    );

    // Reflect must have passed.
    let reflect_ok = sink.events.iter().any(|e| {
        matches!(
            e,
            AgentEvent::Reflect {
                validation_ok: true,
                ..
            }
        )
    });
    assert!(reflect_ok, "expected at least one passing reflect");
}

#[test]
fn tool_registry_into_arc_shares_tools() {
    // Sanity: cloning a registry and wrapping it in Arc keeps
    // the tool lookups working. ApproveProposalTool relies on
    // this.
    let world = Arc::new(World::in_memory().unwrap());
    let mut reg = ToolRegistry::new();
    reg.register(ValidateWorldTool::new(world.clone()));
    let arc = reg.into_arc();
    let t = arc.get("validate_world").expect("registered");
    assert_eq!(t.descriptor().name, "validate_world");
}
