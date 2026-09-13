# AGENTS.md

Guide for AI coding agents (Claude Code, Codex, etc.) working in
this project.

This file covers this project only — its stack, conventions,
verification, and deferred list. The monorepo-level doc
(`../AGENTS.md`) covers the product shape, the layered model, the
cross-project policy, and the public constraints that apply to
every project in the repo. Read that alongside this file when in
doubt.

## Project layout

```text
client/                              Rust workspace (resolver = "2")
├── Cargo.toml                        workspace root + shared deps
├── crates/
│   └── sagaline/                     the desktop binary
│       ├── Cargo.toml                 name = "sagaline"
│       └── src/main.rs                gpui-kit hello-world scaffold
├── README.md                          build + quickstart
└── AGENTS.md                          you are here
```

The `crates/` layout is the seam where deferred work goes — add
new crates as siblings, not nested inside `sagaline/`.

## Stack

- **Language:** Rust 1.80+
- **UI:** `gpui-kit = "0.6"` — a single facade crate that re-exports
  GPUI + gpui-base + gpui-component + default assets.
- **Single dependency for the UI layer.** Don't add `gpui`,
  `gpui-base`, or `gpui-component` directly to your `Cargo.toml` —
  `gpui-kit` re-exports all three at the matching version. Drift
  between the facade and the internals is the most common source
  of "doc example doesn't compile" failures.

## Conventions

### Workspace layout

- Single member crate `sagaline` for now. Add new crates as
  siblings under `crates/` and add them to `members = […]` in
  the workspace `Cargo.toml`. Anticipated split:
  - `crates/core/` — story / character / shot workspace model,
    persistence, BYOK key storage.
  - `crates/providers/` — `trait ModelAdapter` with per-provider
    implementations.
  - `crates/sagaline/` keeps the binary thin — `main.rs` boots
    the app, hands off to `core`, renders with `gpui-component`.
- Shared dep versions go in `[workspace.dependencies]` (already
  set up for `gpui-kit`). Member crates take them with
  `{ workspace = true }`.

### Rust rules

- **No `as any` / `: any`.** Never inline-cast an object type
  for member access — narrow with `in` / `typeof` or `match`
  instead. If a library boundary really requires an unchecked
  cast, assign to a named `const` with a one-line reason; never
  inline.
- **No tiny pure-rename functions.** Inline trivial expressions;
  README-style scaffolds tempt wrappers that just rename another
  function. Drop them. Three call sites need lockstep behavior,
  exported name is a stable domain concept, callback identity
  matters, type guard preserves narrowing, or public API
  boundary needs indirection — otherwise inline.
- **Pre-allocate at the type system.** `&str` not `String` where
  the lifetime permits, `&[u8]` not `Vec<u8>` for read-only
  buffers. Don't avoidably clone.
- **The gpui / gpui-kit API surface is opaque-ish.** If a
  method name doesn't exist, **don't invent one**. Read the
  `gpui-base` source under
  `~/.cargo/registry/src/.../gpui-base-0.6.1/src/` — the README
  example uses the published API but may be ahead of the version
  you have pinned. The styled helpers (`.text_xl()`, `.bg()`,
  `.text_color()`) live on `Div` via a trait re-exported from
  `gpui-kit`, not as global helper functions.

### Persistence & secrets

- BYOK keys live in `~/.config/sagaline/`. Read at startup, never
  write to a global location, never phone home.
- Project state: SQLite (planned) via `rusqlite`. Database file
  in the user's project directory, not in app data.
- Don't bundle telemetry, crash reporting, or auto-update checks
  into this binary. A user with `cargo build` and no network must
  be able to use the whole pipeline.

### Brand strings

- Use the wordmark from `gpui-component`'s built-in components, or
  rebuild the film-clapper SVG that appears in
  `../homeweb/src/components/brand/logo.tsx` if a custom render
  is needed (match dimensions and stroke weight).
- This binary does NOT depend on any marketing copy. All user-
  visible strings are app-shell strings (project name, role
  labels, button copy) — never the homepage/FAQ copy from
  `../homeweb/`.

## Verifying changes

```bash
cargo check       # fast — typecheck only
cargo build       # full link — produces target/debug/sagaline
cargo run         # launches the desktop window (visual verify)
```

A change is done when `cargo build` produces
`target/debug/sagaline` without errors. The pre-existing
`block 0.1.6` future-compat warning on transitive deps is not
yours to chase.

For visual changes, `cargo run` and confirm the window opens;
the current scaffold renders a single window with the brand + a
"New story" button.

## Known carry-over

- This is a minimal scaffold. `main.rs` is one `SagalineApp` view
  with the brand + a placeholder button. No story workspace, no
  adapter trait, no persistence — that's all in the deferred
  list below.
- `Cargo.lock` is gitignored (intentional for a binary; keep it
  that way).

## Deferred (next turns)

In priority order — do not skip ahead:

1. **`crates/core/`** — story / character / shot workspace model.
   SQLite persistence via `rusqlite`. BYOK key storage at
   `~/.config/sagaline/`.
2. **`crates/providers/`** — `trait ModelAdapter` with
   per-provider implementations (OpenAI, Google Gemini, Kling,
   Runway, ComfyUI HTTP, Ollama HTTP).
3. **App shell** — top-level `App` entity that owns the workspace
   view; replace the placeholder `SagalineApp` in `main.rs`.
4. **Hosted project sync** — when `../homeweb/` ships the project-
   sync endpoint, add an opt-in client. This is the only piece
   of this project that talks to `homeweb/`, and it must stay
   opt-in (the open source pipeline must work without it).
