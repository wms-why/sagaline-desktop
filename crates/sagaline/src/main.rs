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
use sagaline::{install_env, run_agent};

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

fn main() {
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
