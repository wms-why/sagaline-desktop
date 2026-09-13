//! Sagaline desktop client — open source AI video generator.
//!
//! Thin shell: opens the workspace database, seeds demo data, spawns the
//! background writer task, and hands off to `sagaline-ui` for rendering.
//! Subsequent turns add model adapters (OpenAI / Gemini / Kling / Runway /
//! ComfyUI / Ollama) and project sync with homeweb.

use std::path::PathBuf;

use gpui_kit::base::ScrollbarMode;
use gpui_kit::component::*;
use gpui_kit::*;
use sagaline_ui::state::WorkspaceState;
use sagaline_ui::view::WorkspaceView;

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        // The scrollable wrapper used by `overflow_scrollbar` reads its mode
        // from the base theme's scrollbar settings; the default `Scrolling`
        // mode hides the bar 2 s after the last scroll gesture, so a user
        // opening the app with overflowing content sees no bar at all.
        // Pin the mode to `Always` so the right edge of the workspace always
        // shows the scrollbar when the content is taller than the viewport.
        Theme::set_scrollbar_mode(ScrollbarMode::Always, cx);

        let db_path = project_db_path();
        let (cmd_tx, cmd_rx) =
            futures::channel::mpsc::unbounded::<sagaline_core::cmd::Cmd>();
        let state = WorkspaceState::open(db_path, cmd_tx)
            .expect("workspace open");
        let writer_conn = state.conn.clone();
        let workspace_view = cx.new(|_| WorkspaceView::new(state));

        // Background writer — consumes `Cmd` values dispatched by the UI and
        cx.spawn(async move |_cx| {
            sagaline::cmd::run_writer(writer_conn, cmd_rx).await;
        })
        .detach();
        cx.spawn(async move |cx| {
            cx.open_window(
                WindowOptions {
                    // Sidebar is 280 px; the content pane needs enough room to
                    // fit the overview stat row plus a chapter heading without
                    // scrolling horizontally on first paint. Smaller windows
                    // clip the layout.
                    window_min_size: Some(gpui::Size {
                        width: px(800.),
                        height: px(480.),
                    }),
                    // Initial size: fits the Overview stat row (3× ~200 px cards
                    // plus padding ≈ 700 px content width) and the first chapter
                    // block without any scrolling on first paint. Origin clears
                    // the macOS menu bar / Dock.
                    window_bounds: Some(gpui::WindowBounds::Windowed(
                        gpui::Bounds {
                            origin: gpui::Point {
                                x: px(120.),
                                y: px(120.),
                            },
                            size: gpui::Size {
                                width: px(1100.),
                                height: px(720.),
                            },
                        },
                    )),
                    ..WindowOptions::default()
                },
                |window, cx| cx.new(|cx| Root::new(workspace_view, window, cx)),
            )
            .expect("Failed to open window");
        })
        .detach();
    });
}

fn project_db_path() -> PathBuf {
    // First-launch / dev convenience: store the project DB in the user's
    // config directory. AGENTS.md targets `~/.config/sagaline/` for BYOK keys;
    // the project DB lives next to that. We do not crash on failure to
    // create the dir — `open()` surfaces a clear error.
    let mut path = dirs_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("sagaline");
    let _ = std::fs::create_dir_all(&path);
    path.push("workspace.sqlite3");
    path
}

#[cfg(target_os = "macos")]
fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| {
        let mut p = PathBuf::from(h);
        p.push("Library/Application Support");
        p
    })
}

#[cfg(target_os = "linux")]
fn dirs_config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME").map(|h| {
        let mut p = PathBuf::from(h);
        p.push(".config");
        p
    })
}

#[cfg(target_os = "windows")]
fn dirs_config_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn dirs_config_dir() -> Option<PathBuf> {
    None
}