//! End-to-end test of `Agent::run_stream` going through the
//! `sagaline-bridge::TokioBridge`.
//!
//! Phase 2.5: the loop drives the SQLite world DB; the ACT
//! step calls `validate_world`. The bridge plumbing is the
//! same — what changes is the loop's source of truth.
//!
//! This is the regression guard for the original motivation:
//! GPUI's `cx.spawn` runs futures on GPUI's custom scheduler, NOT
//! on Tokio, so calling `tokio::fs::read` from inside a GPUI
//! future panics with "no reactor running". The agent's tool
//! registry may use `tokio::fs` for I/O, so the agent's events
//! *must* be produced on a Tokio runtime. This test asserts that
//! the bridge really wires things that way, end-to-end.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::TestAppContext;
use sagaline_agent::tools::ValidateWorldTool;
use sagaline_agent::{Agent, AgentConfig, AgentEvent};
use sagaline_bridge::TokioBridge;
use sagaline_store::repo::{NewChapter, NewScene, NewStory};
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
    world
        .scenes()
        .create_scene(NewScene {
            chapter_id: &chapter.id,
            slug: "001-intro",
            ordinal: 1,
            title: "Intro",
            synopsis: "Lin Mo walks into the lab.",
        })
        .unwrap();
    (world, story.id)
}

fn build_bridge(cx: &TestAppContext) -> (tokio::runtime::Runtime, TokioBridge) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("sagaline-agent-test")
        .build()
        .expect("tokio runtime");
    let bridge = TokioBridge::new(rt.handle().clone(), cx.background_executor.clone());
    (rt, bridge)
}

fn allow_off_thread_wakes(cx: &TestAppContext) {
    cx.dispatcher.allow_parking();
}

#[gpui_kit::test]
async fn run_stream_under_bridge_executes_tokio_fs_io(cx: &mut TestAppContext) {
    allow_off_thread_wakes(cx);

    let (world, story_id) = build_world();
    let (_rt, bridge) = build_bridge(cx);

    let mut agent = Agent::with_config(AgentConfig::default());
    agent
        .tools_mut()
        .register(ValidateWorldTool::new(world.clone()));

    let (mut rx, _stream_task) = bridge.forward_stream(agent.run_stream(world, &story_id));

    let mut events: Vec<AgentEvent> = Vec::new();
    loop {
        cx.run_until_parked();
        match rx.try_recv() {
            Ok(event) => events.push(event),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }

    let kinds: Vec<&'static str> = events
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
    assert!(
        kinds.starts_with(&["StepStart", "Observe", "Plan", "Act", "Reflect"]),
        "unexpected event shape: {kinds:?}"
    );
    assert_eq!(*kinds.last().expect("at least one event"), "Done");

    let act = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::Act {
                tool,
                result_summary,
                ..
            } => Some((tool, result_summary)),
            _ => None,
        })
        .expect("Act event from the validate_world tool");
    assert_eq!(act.0, "validate_world");
    assert_eq!(act.1, "ok", "validate_world reports ok for a clean story");

    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::Reflect {
            validation_ok: true,
            ..
        }
    )));
}
