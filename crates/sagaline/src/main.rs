// Sagaline desktop binary — opens a gpui-kit window.
//
// Boot sequence:
//   1. `gpui_kit::init` brings up the styled components and the
//      base widget layer.
//   2. `sagaline::install_env` opens (or creates) the
//      `~/.sageline/data/` store, parses `config.toml`, loads the
//      `prefs.toml` project location, and installs the resulting
//      `AppEnv` as a gpui global.
//   3. The window opens, showing the workspace view from
//      `sagaline-ui`.
//   4. When the user opens a story (⌘ O), the view emits
//      `StoryOpened`; we subscribe and call
//      `sagaline::run_agent`, which routes the agent's event stream
//      into the `AgentEventLog` global. The UI re-renders the
//      activity panel automatically.
//
// Key bindings:
//   ⌘ N       — New story (opens the onboarding modal)
//   ⌘ O       — Open Story
//   ⌘ R       — Reload current story
//   ⌘ ,       — Project Settings

use gpui_kit::component::Root;
use gpui_kit::*;

use sagaline_ui::{StoryOpened, WorkspaceView};
use sagaline::{default_data_dir, install_env, run_agent};

mod key_bindings {
    use gpui_kit::KeyBinding;
    use sagaline_ui::{CreateStory, OpenProjectSettings, OpenStory, ReloadStory};

    pub fn new_story() -> KeyBinding {
        KeyBinding::new("cmd-n", CreateStory, None)
    }

    pub fn open_story() -> KeyBinding {
        KeyBinding::new("cmd-o", OpenStory, None)
    }

    pub fn reload_story() -> KeyBinding {
        KeyBinding::new("cmd-r", ReloadStory, None)
    }

    pub fn project_settings() -> KeyBinding {
        KeyBinding::new("cmd-,", OpenProjectSettings, None)
    }
}

/// Install the global tracing subscriber before anything else runs.
///
/// Two layers, both gated by `RUST_LOG` (default `warn`):
///
/// 1. **stderr** — for interactive runs (`cargo run`); ANSI escape
///    codes stay on.
/// 2. **file** — append to `<data_dir>/sagaline.log`; ANSI off, so
///    `grep` / `tail -f` see plain text. Best-effort: if the file
///    can't be opened (permission denied, read-only volume), we
///    fall back to stderr-only so the user still sees errors.
///
/// `try_init()` makes the function idempotent — a future test
/// harness or hot-reload won't panic on the second call.
fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn"));

    let log_path = default_data_dir().join("sagaline.log");
    if let Some(parent) = log_path.parent() {
        // Best-effort — fall through to stderr-only if the dir
        // can't be created.
        let _ = std::fs::create_dir_all(parent);
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "sagaline: cannot open log file {} ({e}); using stderr only",
                log_path.display()
            );
            install_stderr_only(env_filter);
            return;
        }
    };

    let stderr_layer = fmt::layer().with_writer(std::io::stderr);
    let file_layer = fmt::layer().with_writer(file).with_ansi(false);

    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
}

fn install_stderr_only(env_filter: tracing_subscriber::EnvFilter) {
    use tracing_subscriber::{fmt, prelude::*};
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();
}

fn main() {
    init_tracing();

    application().run(|cx| {
        gpui_kit::init(cx);

        let env = install_env(cx).expect("install AppEnv");

        cx.bind_keys([
            key_bindings::new_story(),
            key_bindings::open_story(),
            key_bindings::reload_story(),
            key_bindings::project_settings(),
        ]);

        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| WorkspaceView::new_with_tree(cx));
                sagaline_ui::register_actions(view.clone(), cx);

                // When the user opens a story, kick off the agent
                // loop. We use a `subscribe` callback so the agent
                // runs even on programmatic `open_story` calls
                // (e.g. a future "recent stories" menu).
                let env_for_sub = env.clone();
                cx.subscribe(&view, move |_view, event: &StoryOpened, cx| {
                    let story = event.story.clone();
                    let env = env_for_sub.clone();
                    run_agent(env, story, cx);
                })
                .detach();

                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
