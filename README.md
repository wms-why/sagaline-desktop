# Sagaline — desktop client

Open source AI video generator. Rust + GPUI Kit, Apache 2.0.

This is the minimal scaffold for the desktop client. The companion web
project lives at `../homeweb/` (TanStack Start, Cloudflare Workers).

## Stack

- **Language:** Rust 1.80+
- **UI:** [`gpui-kit`](https://crates.io/crates/gpui-kit) — a single facade
  crate that re-exports GPUI + gpui-base + gpui-component + default assets.
  Pin to `0.6`.

## Layout

```text
client/
├── Cargo.toml              workspace root (resolver = "2")
├── README.md               you are here
└── crates/
    └── sagaline/           the desktop binary
        ├── Cargo.toml
        └── src/
            └── main.rs     gpui-kit hello world + brand surface
```

## Build & run

```bash
# from this directory (client/)
cargo run                 # launch the desktop window

# from anywhere
cargo run --manifest-path client/Cargo.toml
```

On first run, gpui-kit builds its bundled icon set; subsequent runs are fast.

## Next steps (deferred to follow-up turns)

1. Story workspace — premise, character bible, shot planner.
2. Model adapter trait + per-provider implementations
   (OpenAI, Google Gemini, Kling, Runway, ComfyUI/Ollama).
3. BYOK key storage in `~/.config/sagaline/`.
4. Local project persistence (SQLite or sled).
5. Project sync against the hosted cloud build (in `homeweb/`).

## Notes on scaffolding choices

- `gpui-kit` is the single dependency the README prescribes. It pins a
  matching GPUI release and re-exports the layers we need, so we don't have
  to mirror that matrix in our workspace.
- The window surface is the documented `gpui_kit::application()` +
  `gpui_kit::init(cx)` + `Root::new(view, window, cx)` pattern. Avoid
  hand-rolled GPUI scaffolding for now — the next turns will use
  `gpui-component`'s form / input / tree / dock primitives directly.
- This scaffold is intentionally tiny. Once the workspace model + adapter
  layer are in, `main.rs` will route through a top-level `App` entity rather
  than a single `SagalineApp` view.
