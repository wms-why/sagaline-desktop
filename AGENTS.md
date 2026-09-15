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
│   ├── sagaline-core/                story workspace model + validation
│   │   ├── Cargo.toml                 name = "sagaline-core"
│   │   └── src/                       Markdown Source of Truth, in-memory graph
│   └── sagaline/                     the desktop binary (hello-world placeholder)
│       ├── Cargo.toml                 name = "sagaline"
│       └── src/main.rs                placeholder shell
├── README.md                          build + quickstart
└── AGENTS.md                          you are here
```

The `crates/` layout is the seam where deferred work goes — add
new crates as siblings, not nested inside `sagaline/`.

## Stack

- **Language:** Rust 1.80+
- **UI:** `gpui-kit = "0.6"` — a single facade crate that re-exports
  GPUI + gpui-base + gpui-component + default assets. (Currently
  only used by the placeholder shell; UI work resumes when the
  next phase ships.)
- **Single dependency for the UI layer.** Don't add `gpui`,
  `gpui-base`, or `gpui-component` directly to your `Cargo.toml` —
  `gpui-kit` re-exports all three at the matching version. Drift
  between the facade and the internals is the most common source
  of "doc example doesn't compile" failures.

## Conventions

### Workspace layout

Current member crates: `sagaline-core` and `sagaline`. Add new
crates as siblings under `crates/` and add them to
`members = […]` in the workspace `Cargo.toml`.

- `crates/sagaline-core/` — pure story workspace model:
  `StoryRoot` (path validation), `StoryGraph` (single-walk in-memory
  graph + validation), entity types (`Story` / `Bible` / `Character`
  / `Environment` / `Prop` / `Chapter` / `Scene` / `Shot`).
- `crates/sagaline/` keeps the binary thin — `main.rs` boots the
  app, hands off to `core`, renders with `gpui-component`. Currently
  a hello-world placeholder.
- Shared dep versions go in `[workspace.dependencies]`. Member
crates take them with `{ workspace = true }`.


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

### Storage model — Markdown Source of Truth

**The filesystem is the database.** A story is a directory of
Markdown + YAML front-matter files; nothing else.

```text
<user-chosen>/<story-slug>/
├── story.md                         # story entry; id/type/slug/title in front matter
├── bible/<slug>.md                  # one file per bible entry (world / timeline / rules / lore)
├── characters/<slug>/character.md   # one folder per character
├── environments/<slug>/environment.md
├── props/<slug>.md                  # single-file entities
├── chapters/<NNN-slug>/
│   ├── chapter.md
│   └── scenes/<NNN-slug>.md
└── assets/                          # generated images / videos / audio (gitignored)
```

- **Story entry is `story.md`** at the root; the loader rejects any
  directory missing it.
- **Slugs are path-derived** — `characters/lin-mo/character.md` →
  slug `lin-mo`. The on-disk slug is the source of truth;
  front-matter `slug` must match or validation flags a
  `SlugMismatch`.
- **Front matter is YAML**, delimited by `---` lines at the head
  of the file. Body is preserved verbatim (not parsed this turn).
- **Reference resolution** — a scene's front matter can name
  characters / environment / props by either `id` or `slug`:
  ```yaml
  characters: [lin-mo, su-yan]
  environment: laboratory
  props: [energy-core]
  ```
  Slug wins on tie. Unresolved strings still emit a `Reference`
  (with `to` set to the literal string) so `validate()` can flag
  `BrokenReference`.
- **Walker skips** `assets/`, `references/`, anything under
  `characters/<slug>/references/` or `environments/<slug>/references/`,
  and any hidden (`.`-prefixed) directory.

### Persistence & secrets

- **No embedded database in this turn.** `StoryGraph::load` walks
  the directory once and produces an in-memory graph; there is no
  SQLite / sled / redb / index layer. The filesystem *is* the
  index. SQLite may be reintroduced in a later phase for runtime
  generation-job state (jobs / assets / embeddings) — not for
  story content. When that lands it lives in
  `~/.sageline/data/index.db`, not in the story directory.
- **Story root location** — chosen by the user (or by an
  application-level "open story" flow in a later phase). The core
  crate does not assume any default location.
- **BYOK keys** — machine-bound, encrypted. Future location:
  `~/.sageline/data/keys.db` (placeholder; the storage crate is
  not built yet). Read at startup, never write to a global
  location, never phone home.
- **No telemetry.** No analytics, no crash reporting, no
  auto-update pings. A user with `cargo build` and no network must
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
cargo check -p sagaline-core   # fast — typecheck the core crate only
cargo test  -p sagaline-core   # 36 in-module tests
cargo build -p sagaline-core   # full link of the core crate
```

A `sagaline-core` change is done when `cargo test -p sagaline-core`
passes (36 tests). Workspace-wide `cargo check` is clean.

For visual changes in a later phase, `cargo run` and confirm the
window opens.

## Known carry-over

- The desktop binary (`crates/sagaline/src/main.rs`) is a
  hello-world placeholder. It does not yet render any UI shell
  beyond `println!`.
- `crates/sagaline-ui/` and `crates/sagaline-providers/` do not
  yet exist. Both will be scaffolded fresh in later phases; until
  then, no UI rendering and no model provider adapters.
- `Cargo.lock` is gitignored (intentional for a binary; keep it
  that way).

## Deferred (next turns)

In priority order — do not skip ahead:

1. **`crates/sagaline-ui/` (scaffold)** — create the crate against
   the new `sagaline-core` API. File tree, Markdown editor pane,
   scene reference preview. Re-add `gpui-kit` / `gpui-component`
   to the workspace at that point.
2. **App shell (`crates/sagaline/src/main.rs`)** — replace the
   `println!` placeholder with a real top-level `App` that owns
   the workspace view and opens the `sagaline-ui` components.
3. **`crates/sagaline-providers/` (scaffold)** — `trait
   ModelAdapter` with per-provider implementations (OpenAI, Google
   Gemini, Kling, Runway, ComfyUI HTTP, Ollama HTTP). When this
   lands, runtime generation-job state (jobs / assets / embeddings)
   may go into `~/.sageline/data/index.db` via `rusqlite` — but
   **story content stays in the Markdown files**.
4. **BYOK key store** — encrypted storage under
   `~/.sageline/data/keys.db`, age-encrypted, machine-bound.
   Provider config reads from here.
5. **Hosted project sync** — when `../homeweb/` ships the project-
   sync endpoint, add an opt-in client. This is the only piece of
   this project that talks to `homeweb/`, and it must stay opt-in
   (the open source pipeline must work without it).