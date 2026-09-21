//! Integration tests for the Phase 2 domain tools.

use std::sync::Arc;

use sagaline_agent::tools::{
    AddCharacterAgeTool, AddCharacterAppearanceTool, AssignCharacterToSceneTool,
    AssignEnvironmentToSceneTool, CreateChapterTool, CreateCharacterTool, CreateEnvironmentTool,
    CreatePropTool, CreateSceneTool, CreateShotTool, GetStoryTool, ListStoriesTool,
    SearchStoryTool, UpdateCharacterTool, ValidateWorldTool,
};
use sagaline_agent::{Capability, Tool, ToolContext, ToolRegistry};
use sagaline_store::repo::{NewChapter, NewCharacter, NewEnvironment, NewProp, NewScene, NewStory};
use sagaline_store::World;
use serde_json::json;

fn fresh() -> (Arc<World>, ToolContext) {
    let world = Arc::new(World::in_memory().expect("world"));
    let ctx = ToolContext::new(world.clone());
    (world, ctx)
}

fn fresh_registry() -> (Arc<World>, ToolContext, ToolRegistry) {
    let (w, c) = fresh();
    (w, c, ToolRegistry::new())
}

async fn run(tool: &dyn Tool, ctx: ToolContext, args: serde_json::Value) -> serde_json::Value {
    tool.execute(ctx, args).await.expect("tool call")
}

#[tokio::test]
async fn get_story_by_id_and_slug() {
    let (w, ctx) = fresh();
    let tool = GetStoryTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "s",
            title: "Story",
            summary: "",
        })
        .unwrap();

    let by_id = run(&tool, ctx.clone(), json!({ "id": story.id.clone() })).await;
    assert_eq!(by_id["story"]["slug"], "s");

    let by_slug = run(&tool, ctx.clone(), json!({ "slug": "s" })).await;
    assert_eq!(by_slug["story"]["id"], story.id);

    let neither = tool.execute(ctx, json!({})).await.unwrap_err();
    assert!(
        matches!(neither, sagaline_agent::ToolError::BadArgs { .. }),
        "got {neither:?}"
    );
}

#[tokio::test]
async fn list_stories_empty_then_populated() {
    let (w, ctx) = fresh();
    let tool = ListStoriesTool::new(w.clone());
    assert_eq!(
        run(&tool, ctx.clone(), json!({})).await["stories"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    w.stories()
        .create(NewStory {
            slug: "a",
            title: "A",
            summary: "",
        })
        .unwrap();
    w.stories()
        .create(NewStory {
            slug: "b",
            title: "B",
            summary: "",
        })
        .unwrap();
    let out = run(&tool, ctx, json!({})).await;
    assert_eq!(out["stories"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn search_story_substring() {
    let (w, ctx) = fresh();
    let tool = SearchStoryTool::new(w.clone());
    w.stories()
        .create(NewStory {
            slug: "a",
            title: "The Adventure",
            summary: "",
        })
        .unwrap();
    w.stories()
        .create(NewStory {
            slug: "b",
            title: "Cooking Show",
            summary: "adventure in taste",
        })
        .unwrap();
    let out = run(&tool, ctx, json!({ "query": "adventure" })).await;
    let matches = out["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 2, "title + summary match");
}

#[tokio::test]
async fn create_character_and_update() {
    let (w, ctx, mut reg) = fresh_registry();
    reg.register(CreateCharacterTool::new(w.clone()));
    reg.register(UpdateCharacterTool::new(w.clone()));

    let story = w
        .stories()
        .create(NewStory {
            slug: "s",
            title: "S",
            summary: "",
        })
        .unwrap();

    let created = run(
        reg.get("create_character").unwrap(),
        ctx.clone(),
        json!({
            "story_id": story.id,
            "slug": "lin-mo",
            "name": "Lin Mo",
            "occupation": "reporter",
            "bio": "stubborn",
        }),
    )
    .await;
    let id = created["character"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["character"]["name"], "Lin Mo");

    let updated = run(
        reg.get("update_character").unwrap(),
        ctx,
        json!({
            "character_id": id,
            "name": "Lin Mo (elderly)",
            "bio": "stubborn, older",
        }),
    )
    .await;
    assert_eq!(updated["character"]["name"], "Lin Mo (elderly)");
    assert_eq!(
        updated["character"]["occupation"], "reporter",
        "occupation untouched"
    );
}

#[tokio::test]
async fn add_character_age_and_appearance() {
    let (w, ctx, mut reg) = fresh_registry();
    reg.register(CreateCharacterTool::new(w.clone()));
    reg.register(AddCharacterAgeTool::new(w.clone()));
    reg.register(AddCharacterAppearanceTool::new(w.clone()));

    let story = w
        .stories()
        .create(NewStory {
            slug: "s",
            title: "S",
            summary: "",
        })
        .unwrap();
    let c = w
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "lin-mo",
            name: "Lin Mo",
            occupation: None,
            bio: "",
        })
        .unwrap();

    let age = run(
        reg.get("add_character_age").unwrap(),
        ctx.clone(),
        json!({ "character_id": c.id, "age": 28, "note": "first appearance" }),
    )
    .await;
    assert!(age["age_id"].is_string());

    let app = run(
        reg.get("add_character_appearance").unwrap(),
        ctx,
        json!({ "character_id": c.id, "label": "战斗服", "description": "battle gear" }),
    )
    .await;
    assert!(app["appearance_id"].is_string());
}

#[tokio::test]
async fn create_chapter_links_to_story() {
    let (w, ctx) = fresh();
    let tool = CreateChapterTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "s",
            title: "S",
            summary: "",
        })
        .unwrap();
    let ch = run(
        &tool,
        ctx,
        json!({
            "story_id": story.id,
            "slug": "ch-1",
            "ordinal": 1,
            "title": "Chapter One",
            "synopsis": "",
        }),
    )
    .await;
    assert_eq!(ch["chapter"]["story_id"], story.id);
}

#[tokio::test]
async fn create_scene_is_multi_table_atomic() {
    let (w, ctx, mut reg) = fresh_registry();
    reg.register(CreateChapterTool::new(w.clone()));
    reg.register(CreateCharacterTool::new(w.clone()));
    reg.register(CreateEnvironmentTool::new(w.clone()));
    reg.register(CreateSceneTool::new(w.clone()));
    reg.register(AssignCharacterToSceneTool::new(w.clone()));
    reg.register(AssignEnvironmentToSceneTool::new(w.clone()));

    let story = w
        .stories()
        .create(NewStory {
            slug: "tx",
            title: "Tx",
            summary: "",
        })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "ch-1",
            ordinal: 1,
            title: "Ch 1",
            synopsis: "",
        })
        .unwrap();
    let lin = w
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "lin-mo",
            name: "Lin Mo",
            occupation: None,
            bio: "",
        })
        .unwrap();
    let lab = w
        .environments()
        .create(NewEnvironment {
            story_id: &story.id,
            slug: "lab",
            name: "Lab",
            description: "",
        })
        .unwrap();

    // Headline: create_scene writes 4 rows in one transaction.
    let out = run(
        reg.get("create_scene").unwrap(),
        ctx.clone(),
        json!({
            "chapter_id": ch.id,
            "slug": "scene-1",
            "ordinal": 1,
            "title": "S1",
            "synopsis": "",
            "environment_id": lab.id,
            "characters": [
                { "character_id": lin.id }
            ],
        }),
    )
    .await;
    let scene_id = out["scene"]["id"].as_str().unwrap().to_string();
    assert_eq!(out["character_assignments"], 1);
    assert_eq!(out["environment_assigned"], true);

    // Verify rows landed.
    let conn = w.conn().unwrap();
    let n_chars: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scene_characters WHERE scene_id = ?1",
            [&scene_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n_chars, 1);
    let n_env: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scene_environments WHERE scene_id = ?1",
            [&scene_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n_env, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn create_scene_rolls_back_helper() {
    use sagaline_agent::ToolError;

    let (w, ctx) = fresh();
    let tool = CreateSceneTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "rb2",
            title: "RB",
            summary: "",
        })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "ch",
            ordinal: 1,
            title: "Ch",
            synopsis: "",
        })
        .unwrap();
    let err = tool
        .execute(
            ctx,
            json!({
                "chapter_id": ch.id,
                "slug": "scene-x",
                "ordinal": 1,
                "title": "X",
                "synopsis": "",
                "characters": [
                    { "character_id": "does-not-exist" }
                ],
            }),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ToolError::Execution { .. }), "got {err:?}");

    let conn = w.conn().unwrap();
    let n_scenes: i64 = conn
        .query_row("SELECT COUNT(*) FROM scenes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n_scenes, 0, "scene must roll back");
}

#[tokio::test]
async fn assign_character_to_scene_upserts() {
    let (w, ctx, mut reg) = fresh_registry();
    reg.register(AssignCharacterToSceneTool::new(w.clone()));

    let story = w
        .stories()
        .create(NewStory {
            slug: "up",
            title: "Up",
            summary: "",
        })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "c",
            ordinal: 1,
            title: "C",
            synopsis: "",
        })
        .unwrap();
    let scene = w
        .scenes()
        .create_scene(NewScene {
            chapter_id: &ch.id,
            slug: "s",
            ordinal: 1,
            title: "S",
            synopsis: "",
        })
        .unwrap();
    let c = w
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "x",
            name: "X",
            occupation: None,
            bio: "",
        })
        .unwrap();

    run(
        reg.get("assign_character_to_scene").unwrap(),
        ctx.clone(),
        json!({ "scene_id": scene.id, "character_id": c.id }),
    )
    .await;
    // Same call again -> upsert replaces the prior row; same outcome.
    run(
        reg.get("assign_character_to_scene").unwrap(),
        ctx,
        json!({ "scene_id": scene.id, "character_id": c.id }),
    )
    .await;
    let conn = w.conn().unwrap();
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM scene_characters WHERE scene_id = ?1 AND character_id = ?2",
            rusqlite::params![scene.id, c.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "upsert must keep exactly one row");
}

#[tokio::test]
async fn create_shot_round_trip() {
    let (w, ctx) = fresh();
    let tool = CreateShotTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "shot",
            title: "S",
            summary: "",
        })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "c",
            ordinal: 1,
            title: "C",
            synopsis: "",
        })
        .unwrap();
    let scene = w
        .scenes()
        .create_scene(NewScene {
            chapter_id: &ch.id,
            slug: "s",
            ordinal: 1,
            title: "S",
            synopsis: "",
        })
        .unwrap();
    let out = run(
        &tool,
        ctx,
        json!({
            "scene_id": scene.id,
            "slug": "shot-1",
            "ordinal": 1,
            "title": "Opening",
            "duration_sec": 4.5,
            "prompt": "slow push-in on Lin Mo",
        }),
    )
    .await;
    assert!(out["shot_id"].is_string());
}

#[tokio::test]
async fn create_environment_round_trip() {
    let (w, ctx) = fresh();
    let tool = CreateEnvironmentTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "env",
            title: "E",
            summary: "",
        })
        .unwrap();
    let out = run(
        &tool,
        ctx,
        json!({
            "story_id": story.id,
            "slug": "lab",
            "name": "Lab",
            "description": "幽蓝的地下实验室",
        }),
    )
    .await;
    let row = &out["environment"];
    assert_eq!(row["slug"], "lab");
    assert_eq!(row["name"], "Lab");
    assert_eq!(row["description"], "幽蓝的地下实验室");
}

#[tokio::test]
async fn create_prop_round_trip() {
    let (w, ctx) = fresh();
    let tool = CreatePropTool::new(w.clone());
    let story = w
        .stories()
        .create(NewStory {
            slug: "pr",
            title: "P",
            summary: "",
        })
        .unwrap();
    let out = run(
        &tool,
        ctx,
        json!({
            "story_id": story.id,
            "slug": "energy-core",
            "name": "Energy Core",
            "description": "",
        }),
    )
    .await;
    assert!(out["prop_id"].is_string());
}

#[tokio::test]
async fn validate_world_clean_when_consistent() {
    let (w, ctx, mut reg) = fresh_registry();
    reg.register(CreateSceneTool::new(w.clone()));

    let story = w
        .stories()
        .create(NewStory {
            slug: "v",
            title: "V",
            summary: "",
        })
        .unwrap();
    let ch = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &story.id,
            slug: "c",
            ordinal: 1,
            title: "C",
            synopsis: "",
        })
        .unwrap();
    let lin = w
        .characters()
        .create(NewCharacter {
            story_id: &story.id,
            slug: "l",
            name: "L",
            occupation: None,
            bio: "",
        })
        .unwrap();
    let env = w
        .environments()
        .create(NewEnvironment {
            story_id: &story.id,
            slug: "e",
            name: "E",
            description: "",
        })
        .unwrap();
    run(
        reg.get("create_scene").unwrap(),
        ctx.clone(),
        json!({
            "chapter_id": ch.id,
            "slug": "s",
            "ordinal": 1,
            "title": "S",
            "environment_id": env.id,
            "characters": [{ "character_id": lin.id }],
        }),
    )
    .await;

    let out = run(&ValidateWorldTool::new(w.clone()), ctx, json!({})).await;
    assert_eq!(out["ok"], true);
    assert_eq!(out["errors"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn validate_world_flags_cross_story_assignment() {
    // Direct SQL to bypass tool-level FK checks — create a
    // legitimate scene + character in story A, then a scene in
    // story B referencing that character. validate_world should
    // flag it.
    use sagaline_store::repo::{CharacterRow, SceneRow};
    use std::sync::Arc;

    let (w, ctx) = fresh();
    let s_a = w
        .stories()
        .create(NewStory {
            slug: "a",
            title: "A",
            summary: "",
        })
        .unwrap();
    let s_b = w
        .stories()
        .create(NewStory {
            slug: "b",
            title: "B",
            summary: "",
        })
        .unwrap();
    let ch_a = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &s_a.id,
            slug: "c",
            ordinal: 1,
            title: "C",
            synopsis: "",
        })
        .unwrap();
    let ch_b = w
        .scenes()
        .create_chapter(NewChapter {
            story_id: &s_b.id,
            slug: "c",
            ordinal: 1,
            title: "C",
            synopsis: "",
        })
        .unwrap();
    let lin_a: CharacterRow = w
        .characters()
        .create(NewCharacter {
            story_id: &s_a.id,
            slug: "l",
            name: "L",
            occupation: None,
            bio: "",
        })
        .unwrap();
    let scene_b: SceneRow = w
        .scenes()
        .create_scene(NewScene {
            chapter_id: &ch_b.id,
            slug: "s",
            ordinal: 1,
            title: "S",
            synopsis: "",
        })
        .unwrap();
    let _ch_a_id = ch_a.id.clone();
    let _ = ch_a;
    let _ = Arc::new(0);

    // Bypass create_scene (which would refuse cross-story).
    // Inject the bad row directly.
    {
        let conn = w.conn().unwrap();
        // Disable FK so this is even possible.
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        conn.execute(
            "INSERT INTO scene_characters (scene_id, character_id) VALUES (?1, ?2)",
            rusqlite::params![scene_b.id, lin_a.id],
        )
        .unwrap();
    }

    let out = run(&ValidateWorldTool::new(w), ctx, json!({})).await;
    assert_eq!(out["ok"], false);
    let errs = out["errors"].as_array().unwrap();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0]["kind"], "cross_story_character");
    assert_eq!(errs[0]["scene_story"], s_b.id);
    assert_eq!(errs[0]["character_story"], s_a.id);
}

#[test]
fn tool_registry_groups_by_capability() {
    let (w, _ctx) = fresh();
    let mut reg = ToolRegistry::new();
    reg.register(GetStoryTool::new(w.clone()));
    reg.register(CreateCharacterTool::new(w.clone()));
    reg.register(ValidateWorldTool::new(w.clone()));

    let groups = reg.by_capability();
    let read = groups.get(&Capability::Read).expect("read group");
    assert!(read.contains(&"get_story".to_string()));
    assert!(read.contains(&"validate_world".to_string()));
    let mutate = groups.get(&Capability::Mutate).expect("mutate group");
    assert!(mutate.contains(&"create_character".to_string()));
    assert!(groups.get(&Capability::Execute).is_none() || groups[&Capability::Execute].is_empty());
}
