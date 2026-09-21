//! Cross-table world-state validation, runnable both standalone
//! (read-only, used by the `validate_world` tool) and inside a
//! caller-supplied transaction (used by `approve_proposal` so the
//! validation step is atomic with the commit).
//!
//! SQLite FK constraints cover most references at write time; this
//! module surfaces references that are technically valid but
//! semantically wrong (e.g. a scene's character lives in a
//! different story than the chapter the scene sits in).

use rusqlite::Connection;
use serde::Serialize;

/// The shape of a single validation finding. Mirrors
/// `ValidationIssue` in `sagaline-agent`'s `world_validate` tool
/// (kept structurally identical so the JSON wire format stays the
/// same; the agent crate re-exports / converts as needed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValidationIssue {
    /// `scene_characters.character_id` references a character in
    /// a different story than the chapter the scene sits in.
    CrossStoryCharacter {
        scene_id: String,
        character_id: String,
        scene_story: String,
        character_story: String,
    },
    /// `scene_characters.character_age_id` references an age row
    /// that doesn't belong to the listed character.
    OrphanAgeReference {
        scene_id: String,
        character_id: String,
        character_age_id: String,
    },
    /// `scene_characters.appearance_id` references an appearance
    /// that doesn't belong to the listed character.
    OrphanAppearanceReference {
        scene_id: String,
        character_id: String,
        appearance_id: String,
    },
    /// `scene_environments.environment_id` points to an
    /// environment in a different story than the chapter.
    CrossStoryEnvironment {
        scene_id: String,
        environment_id: String,
        scene_story: String,
        environment_story: String,
    },
}

type CrossCharRow = (String, String, String, String);
type CrossEnvRow = (String, String, String, String);
type OrphanRefRow = (String, String, String);

/// Run every cross-table check. `story_filter`, if set, restricts
/// the cross-story checks (orphan age/appearance checks are
/// global — they don't depend on the story).
pub fn validate_world_in_tx(
    conn: &Connection,
    story_filter: Option<&str>,
) -> rusqlite::Result<Vec<ValidationIssue>> {
    let mut out = Vec::new();

    for (scene_id, character_id, scene_story, character_story) in
        collect_cross_char(conn, story_filter)?
    {
        out.push(ValidationIssue::CrossStoryCharacter {
            scene_id,
            character_id,
            scene_story,
            character_story,
        });
    }
    for (scene_id, character_id, character_age_id) in collect_orphan_ages(conn)? {
        out.push(ValidationIssue::OrphanAgeReference {
            scene_id,
            character_id,
            character_age_id,
        });
    }
    for (scene_id, character_id, appearance_id) in collect_orphan_appearances(conn)? {
        out.push(ValidationIssue::OrphanAppearanceReference {
            scene_id,
            character_id,
            appearance_id,
        });
    }
    for (scene_id, environment_id, scene_story, environment_story) in
        collect_cross_env(conn, story_filter)?
    {
        out.push(ValidationIssue::CrossStoryEnvironment {
            scene_id,
            environment_id,
            scene_story,
            environment_story,
        });
    }
    Ok(out)
}

fn collect_cross_char(
    conn: &Connection,
    story_filter: Option<&str>,
) -> rusqlite::Result<Vec<CrossCharRow>> {
    if let Some(story_id) = story_filter {
        let mut stmt = conn.prepare(
            "SELECT sc.scene_id, sc.character_id,
                    s_chapter.story_id AS scene_story,
                    c.story_id AS character_story
             FROM scene_characters sc
             JOIN scenes s ON s.id = sc.scene_id
             JOIN chapters s_chapter ON s_chapter.id = s.chapter_id
             JOIN characters c ON c.id = sc.character_id
             WHERE c.story_id != s_chapter.story_id
               AND s_chapter.story_id = ?1",
        )?;
        let rows = stmt.query_map([story_id], |r| {
            Ok((
                r.get::<_, String>("scene_id")?,
                r.get::<_, String>("character_id")?,
                r.get::<_, String>("scene_story")?,
                r.get::<_, String>("character_story")?,
            ))
        })?;
        rows.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT sc.scene_id, sc.character_id,
                    s_chapter.story_id AS scene_story,
                    c.story_id AS character_story
             FROM scene_characters sc
             JOIN scenes s ON s.id = sc.scene_id
             JOIN chapters s_chapter ON s_chapter.id = s.chapter_id
             JOIN characters c ON c.id = sc.character_id
             WHERE c.story_id != s_chapter.story_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>("scene_id")?,
                r.get::<_, String>("character_id")?,
                r.get::<_, String>("scene_story")?,
                r.get::<_, String>("character_story")?,
            ))
        })?;
        rows.collect()
    }
}

fn collect_cross_env(
    conn: &Connection,
    story_filter: Option<&str>,
) -> rusqlite::Result<Vec<CrossEnvRow>> {
    if let Some(story_id) = story_filter {
        let mut stmt = conn.prepare(
            "SELECT se.scene_id, se.environment_id,
                    s_chapter.story_id AS scene_story,
                    e.story_id AS environment_story
             FROM scene_environments se
             JOIN scenes s ON s.id = se.scene_id
             JOIN chapters s_chapter ON s_chapter.id = s.chapter_id
             JOIN environments e ON e.id = se.environment_id
             WHERE e.story_id != s_chapter.story_id
               AND s_chapter.story_id = ?1",
        )?;
        let rows = stmt.query_map([story_id], |r| {
            Ok((
                r.get::<_, String>("scene_id")?,
                r.get::<_, String>("environment_id")?,
                r.get::<_, String>("scene_story")?,
                r.get::<_, String>("environment_story")?,
            ))
        })?;
        rows.collect()
    } else {
        let mut stmt = conn.prepare(
            "SELECT se.scene_id, se.environment_id,
                    s_chapter.story_id AS scene_story,
                    e.story_id AS environment_story
             FROM scene_environments se
             JOIN scenes s ON s.id = se.scene_id
             JOIN chapters s_chapter ON s_chapter.id = s.chapter_id
             JOIN environments e ON e.id = se.environment_id
             WHERE e.story_id != s_chapter.story_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>("scene_id")?,
                r.get::<_, String>("environment_id")?,
                r.get::<_, String>("scene_story")?,
                r.get::<_, String>("environment_story")?,
            ))
        })?;
        rows.collect()
    }
}

fn collect_orphan_ages(conn: &Connection) -> rusqlite::Result<Vec<OrphanRefRow>> {
    let mut stmt = conn.prepare(
        "SELECT sc.scene_id, sc.character_id, sc.character_age_id
         FROM scene_characters sc
         WHERE sc.character_age_id IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM character_ages ca
               WHERE ca.id = sc.character_age_id
                 AND ca.character_id = sc.character_id
           )",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>("scene_id")?,
            r.get::<_, String>("character_id")?,
            r.get::<_, String>("character_age_id")?,
        ))
    })?;
    rows.collect()
}

fn collect_orphan_appearances(conn: &Connection) -> rusqlite::Result<Vec<OrphanRefRow>> {
    let mut stmt = conn.prepare(
        "SELECT sc.scene_id, sc.character_id, sc.appearance_id
         FROM scene_characters sc
         WHERE sc.appearance_id IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM character_appearances ca
               WHERE ca.id = sc.appearance_id
                 AND ca.character_id = sc.character_id
           )",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>("scene_id")?,
            r.get::<_, String>("character_id")?,
            r.get::<_, String>("appearance_id")?,
        ))
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::NewChapter;
    use crate::repo::NewCharacter;
    use crate::repo::NewEnvironment;
    use crate::repo::NewScene;
    use crate::repo::NewStory;
    use crate::World;

    fn fresh() -> World {
        World::in_memory().expect("world")
    }

    #[test]
    fn validate_world_in_tx_empty_world_has_no_issues() {
        let world = fresh();
        let conn = world.conn().unwrap();
        let issues = validate_world_in_tx(&conn, None).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_world_in_tx_returns_empty_when_story_filter_excludes_issues() {
        // Sanity: if everything is well-formed, validation
        // returns no issues regardless of filter.
        let world = fresh();
        let story = world
            .stories()
            .create(NewStory {
                slug: "s",
                title: "Story",
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

        let conn = world.conn().unwrap();
        let issues = validate_world_in_tx(&conn, Some(&story.id)).unwrap();
        assert!(issues.is_empty());
    }

    #[test]
    fn validate_world_in_tx_detects_cross_story_environment_via_filter() {
        // Build story A with a chapter, story B with an environment,
        // link them by hand. validate_world(story=A) should flag
        // the cross-story reference.
        let world = fresh();
        let story_a = world
            .stories()
            .create(NewStory { slug: "a", title: "A", summary: "" })
            .unwrap();
        let story_b = world
            .stories()
            .create(NewStory { slug: "b", title: "B", summary: "" })
            .unwrap();
        let chapter_a = world
            .scenes()
            .create_chapter(NewChapter {
                story_id: &story_a.id,
                slug: "ch1",
                ordinal: 1,
                title: "Ch1",
                synopsis: "",
            })
            .unwrap();
        // Create a scene using the existing tool path.
        let scene_id = {
            let mut conn = world.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO scenes (id, chapter_id, slug, ordinal, title, synopsis, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                rusqlite::params![
                    uuid::Uuid::now_v7().to_string(),
                    chapter_a.id,
                    "sc1",
                    1i64,
                    "Sc1",
                    "",
                    crate::time_util::now_iso(),
                ],
            )
            .unwrap();
            let id: String = tx
                .query_row(
                    "SELECT id FROM scenes WHERE chapter_id = ?1 AND slug = ?2",
                    rusqlite::params![chapter_a.id, "sc1"],
                    |r| r.get(0),
                )
                .unwrap();
            tx.commit().unwrap();
            id
        };
        // Environment in story B.
        let env = world
            .environments()
            .create(NewEnvironment {
                story_id: &story_b.id,
                slug: "warehouse",
                name: "Warehouse",
                description: "",
            })
            .unwrap();
        // Link them.
        {
            let conn = world.conn().unwrap();
            conn.execute(
                "INSERT INTO scene_environments (scene_id, environment_id) VALUES (?1, ?2)",
                rusqlite::params![scene_id, env.id],
            )
            .unwrap();
        }

        let conn = world.conn().unwrap();
        let issues = validate_world_in_tx(&conn, Some(&story_a.id)).unwrap();
        assert_eq!(issues.len(), 1);
        match &issues[0] {
            ValidationIssue::CrossStoryEnvironment {
                scene_id: sid,
                environment_id: eid,
                scene_story: ss,
                environment_story: es,
            } => {
                assert_eq!(sid, &scene_id);
                assert_eq!(eid, &env.id);
                assert_eq!(ss, &story_a.id);
                assert_eq!(es, &story_b.id);
            }
            other => panic!("expected CrossStoryEnvironment, got {other:?}"),
        }
    }
}
