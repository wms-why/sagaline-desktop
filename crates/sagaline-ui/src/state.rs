//! UI-side workspace state.
//!
//! This is the *selection* layer, not the persistence layer. The persistence
//! layer is `sagaline_core::db`. The view holds a snapshot of the workspace
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc::UnboundedSender;
use sagaline_core::rusqlite;
use sagaline_core::db;
use sagaline_core::model::*;

/// Top-level sections of a Story workspace, in sidebar order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Overview,
    StoryBible,
    Characters,
    Environments,
    Props,
    References,
    Chapters,
}

impl Section {
    pub const ALL: &'static [Section] = &[
        Section::Overview,
        Section::StoryBible,
        Section::Characters,
        Section::Environments,
        Section::Props,
        Section::References,
        Section::Chapters,
    ];

    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Section::Overview => t!("section.overview"),
            Section::StoryBible => t!("section.story_bible"),
            Section::Characters => t!("section.characters"),
            Section::Environments => t!("section.environments"),
            Section::Props => t!("section.props"),
            Section::References => t!("section.references"),
            Section::Chapters => t!("section.chapters"),
        }
    }
}

/// Owns the SQLite connection + the loaded workspace snapshot + selection.
pub struct WorkspaceState {
    pub db_path: PathBuf,
    pub conn: Arc<Mutex<rusqlite::Connection>>,
    pub workspace: StoryWorkspace,
    pub selected_story_id: Option<StoryId>,
    pub section: Section,
    /// Set when the user picks a specific chapter / scene / shot in the
    /// Chapters section. Optional — they may just browse the section.
    pub selected_chapter_id: Option<ChapterId>,
    pub selected_scene_id: Option<SceneId>,
    pub selected_shot_id: Option<ShotId>,
    /// Sender half of the writer task's mpsc channel. The UI dispatches
    /// `sagaline_core::cmd::Cmd` values onto this; the background writer in
    /// `sagaline::cmd::run_writer` consumes them off the matching receiver.
    pub cmd_tx: UnboundedSender<sagaline_core::cmd::Cmd>,
}

impl WorkspaceState {
    pub fn open(
        db_path: PathBuf,
        cmd_tx: UnboundedSender<sagaline_core::cmd::Cmd>,
    ) -> anyhow::Result<Self> {
        let conn = db::open(&db_path)?;
        sagaline_core::seed::ensure_demo_story(&conn)?;
        let stories = db::list_stories(&conn)?;
        let selected_story_id = stories.first().map(|s| s.id.clone());
        let workspace = match &selected_story_id {
            Some(id) => db::load_workspace(&conn, id)?,
            None => StoryWorkspace::default(),
        };
        Ok(Self {
            db_path,
            conn: Arc::new(Mutex::new(conn)),
            workspace,
            selected_story_id,
            section: Section::Overview,
            selected_chapter_id: None,
            selected_scene_id: None,
            selected_shot_id: None,
            cmd_tx,
        })
    }

    /// Send a command to the background writer. Non-blocking — the writer
    /// processes commands sequentially on its own task. Returns
    /// `Err(SendError)` only if the writer task has been dropped.
    pub fn dispatch(
        &self,
        cmd: sagaline_core::cmd::Cmd,
    ) -> Result<(), futures::channel::mpsc::TrySendError<sagaline_core::cmd::Cmd>> {
        self.cmd_tx.unbounded_send(cmd)
    }
    pub fn select_story(&mut self, id: StoryId) -> anyhow::Result<()> {
        self.selected_story_id = Some(id.clone());
        self.section = Section::Overview;
        self.selected_chapter_id = None;
        self.selected_scene_id = None;
        self.selected_shot_id = None;
        self.refresh()?;
        Ok(())
    }

    pub fn select_section(&mut self, section: Section) {
        self.section = section;
        // When entering Chapters, auto-select the first chapter + scene + shot.
        if section == Section::Chapters {
            if let Some(ch) = self.workspace.chapters.first() {
                self.selected_chapter_id = Some(ch.chapter.id.clone());
                if let Some(sc) = ch.scenes.first() {
                    self.selected_scene_id = Some(sc.scene.id.clone());
                    self.selected_shot_id =
                        sc.shots.first().map(|s| s.shot.id.clone());
                }
            }
        }
    }

    pub fn select_chapter(&mut self, id: &ChapterId) {
        self.selected_chapter_id = Some(id.clone());
        self.selected_scene_id = None;
        self.selected_shot_id = None;
        if let Some(ch) = self
            .workspace
            .chapters
            .iter()
            .find(|c| c.chapter.id == *id)
        {
            if let Some(sc) = ch.scenes.first() {
                self.selected_scene_id = Some(sc.scene.id.clone());
                self.selected_shot_id =
                    sc.shots.first().map(|s| s.shot.id.clone());
            }
        }
    }

    pub fn select_scene(&mut self, id: SceneId) {
        self.selected_scene_id = Some(id.clone());
        self.selected_shot_id = None;
        for ch in &self.workspace.chapters {
            for sc in &ch.scenes {
                if sc.scene.id == id {
                    self.selected_shot_id =
                        sc.shots.first().map(|s| s.shot.id.clone());
                    return;
                }
            }
        }
    }

    pub fn select_shot(&mut self, id: ShotId) {
        self.selected_shot_id = Some(id);
    }

    /// Reload the workspace from SQLite.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_story_id {
            let conn = self.conn.lock().expect("conn lock poisoned");
            self.workspace = db::load_workspace(&conn, id)?;
        }
        Ok(())
    }
}