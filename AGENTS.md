# AGENTS.md

Guide for AI coding agents (Claude Code, Codex, etc.) working in
this project.

This file covers this project only — its stack, conventions,
verification, and deferred list. The monorepo-level doc
(`../AGENTS.md`) covers the product shape, the layered model, the
cross-project policy, and the public constraints that apply to
every project in the repo. Read that alongside this file when in
doubt.

**Before editing anything in this project, also read
[`SAGALINE.md`](SAGALINE.md).** That doc is the product definition:
Sagaline is a GUI-equipped AI agent specialized in structured AI
video generation. The story workspace is its memory and plan; the
desktop binary is its transparency surface. Every code-level
decision in this crate stack should be traceable back to one of the
mappings in that doc.

## Project layout

```text
client/                              Rust workspace (resolver = "2")
├── Cargo.toml                        workspace root + shared deps
├── SAGALINE.md                       product definition (GUI agent) — read first
├── README.md                          build + quickstart
├── AGENTS.md                          you are here
└── crates/
    ├── sagaline-core/                story workspace model + validation
    │   ├── Cargo.toml                 name = "sagaline-core"
    │   └── src/                       Markdown Source of Truth, in-memory graph
    ├── sagaline-agent/               OBSERVE → PLAN → ACT → REFLECT loop + tool registry
    │   ├── Cargo.toml                 name = "sagaline-agent"
    │   └── src/                       Tool trait, Agent::run, AgentEvent stream
    ├── sagaline-ui/                  gpui-kit views (placeholder)
    │   ├── Cargo.toml                 name = "sagaline-ui"
    │   └── src/                       will subscribe to AgentEvent
    └── sagaline/                     the desktop binary (hello-world placeholder)
        ├── Cargo.toml                 name = "sagaline"
        └── src/main.rs                placeholder shell; will own the App
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

### Workspace layout

Current member crates: `sagaline-core`, `sagaline-agent`,
`sagaline-ui`, `sagaline`. Add new crates as siblings under
`crates/` and add them to `members = […]` in the workspace
`Cargo.toml`.

The four crates line up with the agent's anatomy (see
`SAGALINE.md`):

- `crates/sagaline-core/` — pure story workspace model. The
  agent's memory. `StoryRoot` (path validation), `StoryGraph`
  (single-walk in-memory graph + validation), entity types
  (`Story` / `Bible` / `Character` / `Environment` / `Prop` /
  `Chapter` / `Scene` / `Shot`). Also serves the agent's
  reflection step (`validate()`).
- `crates/sagaline-agent/` — the agent itself. `Tool` trait +
  `ToolRegistry` (each tool carries its own JSON Schema so an LLM
  client can advertise them via function-calling), `AgentEvent`
  stream (the chain-of-thought the UI subscribes to),
  `Agent::run()` driving the OBSERVE → PLAN → ACT → REFLECT
  loop per scene. Tools in this turn: `ReadFileTool`. Canned
  PLAN / REFLECT; the LLM swap is a later phase.
- `crates/sagaline-ui/` — gpui-kit views that subscribe to the
  agent's event stream and render the workspace. Currently a
  placeholder; the agent crate is intentionally built first so
  the UI has a real stream to render.
- `crates/sagaline/` keeps the binary thin — `main.rs` boots the
  app, owns the `App`, hands off to `agent` + `ui`. Currently a
  hello-world placeholder.
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
- **Tool descriptor contract** (sagaline-agent). Every
  `Tool` impl must expose its argument shape via
  `ToolDescriptor::for_args::<T>()` where `T: JsonSchema`. This
  is what a future LLM client will advertise via function-
  calling — hand-rolled `serde_json::Value` schemas will be
  rejected. Each tool carries a one-sentence `description` an
  LLM can act on; do not write a description that only a
  human would understand.
- **Event sink, not callbacks** (sagaline-agent). Tooling and
  the loop never take a callback closure; they push an
  `AgentEvent` to the caller-supplied `EventSink`. The UI
  crate will implement `EventSink` to forward events to a
  gpui channel. Tests use `VecCollector`. Don't reintroduce
  callback-style hooks "for one-off convenience" — they
  serialize the UI thread.

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

**The filesystem is the database — and the database is the agent's
memory.** A story is a directory of Markdown + YAML front-matter
files; nothing else. The agent reads and writes this tree with
its `read_file` / `write_file` tools; the desktop binary is just
the transparency surface over that loop.

A story is a directory of Markdown + YAML front-matter files; the
directory tree *is* the agent's plan and long-term memory.

```text
<user-chosen>/<story-slug>/
├── story.md                         # story entry; id/type/slug/title in front matter
├── bible/<slug>.md                  # one file per bible entry (world / timeline / rules / lore)
├── characters/<slug>/character.md
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
cargo check --workspace         # typecheck every crate
cargo test  --workspace         # 36 core + 5 agent (3 unit + 2 integration)
cargo build -p sagaline         # the desktop binary
```

A change is done when `cargo test --workspace` is clean. The
agent crate's integration test (`tests/loop_smoke.rs`) builds a
temp story, runs `Agent::run`, and asserts the OBSERVE → PLAN →
ACT → REFLECT event stream end-to-end. Add to it (or to a
sibling `tests/foo.rs`) when you add a new tool, a new event
variant, or any other loop-touching change.

For visual changes in a later phase, `cargo run` and confirm the
window opens.

## Known carry-over

- The desktop binary (`crates/sagaline/src/main.rs`) is a
  hello-world placeholder. It does not yet own a real `App` or
  open a window.
- `crates/sagaline-ui/` is a placeholder crate. It will
  subscribe to `sagaline-agent`'s `AgentEvent` stream and render
  the right-side chain-of-thought panel + the workspace tree /
  editor / preview.
- `crates/sagaline-providers/` does not yet exist. It will
  house the `ModelAdapter` trait and per-provider
  implementations; the agent's canned PLAN / REFLECT will be
  replaced by real LLM calls at the same time.
- `crates/sagaline-agent/` is scaffolded in this turn: `Tool`
  trait + `ToolRegistry`, `AgentEvent` stream, `Agent::run` with
  the OBSERVE → PLAN → ACT → REFLECT skeleton, one concrete
  tool (`ReadFileTool`). PLAN / REFLECT are canned (deterministic
  over the scene's frontmatter); the LLM swap is a later phase.
  See `SAGALINE.md` for the agent framing.
- `Cargo.lock` is gitignored (intentional for a binary; keep it
  that way).

## Deferred (next turns)

In priority order — do not skip ahead. The order is driven by
`SAGALINE.md`: Sagaline is a GUI agent, so the agent loop comes
first (it can run with mock tools), then the providers feed it
real actions, then the UI subscribes to the agent's event stream,
then the app shell wires it all together.

1. **`crates/sagaline-agent/` — DONE this turn.** `Tool` trait,
   `ToolRegistry` (each tool advertises a JSON Schema for its
   arguments), `AgentEvent` stream, `Agent::run` driving the
   OBSERVE → PLAN → ACT → REFLECT skeleton, one concrete tool
   (`ReadFileTool`). PLAN / REFLECT are canned (deterministic
   over the scene's frontmatter) — the LLM swap is item 3.
   Verified by `tests/loop_smoke.rs` + the in-crate unit tests.
2. **`crates/sagaline-providers/` (scaffold)** — `trait
   ModelAdapter` with per-provider implementations (OpenAI, Google
   Gemini, Kling, Runway, ComfyUI HTTP, Ollama HTTP). When this
   lands, swap the agent's mock `generate_image` / `generate_video`
   tools for real ones. Runtime generation-job state (jobs /
   assets / embeddings) may go into `~/.sageline/data/index.db`
   via `rusqlite` — but **story content stays in the Markdown
   files**.
3. **Wire LLM into `sagaline-agent`** — replace the agent's
   canned PLAN / REFLECT responses with real LLM calls (BYOK;
   first backend: OpenAI-compatible chat completions). The tool
   registry already speaks JSON Schema, so this is a prompt +
   client swap, not a redesign.
4. **`crates/sagaline-ui/` (scaffold)** — create the crate
   against the new `sagaline-agent` event stream. Right panel
   subscribes to `AgentEvent::{Observe, Plan, Act, Reflect}` and
   renders the chain-of-thought live. File tree, Markdown editor
   pane, scene reference preview. Re-add `gpui-kit` /
   `gpui-component` to the workspace at that point.
5. **App shell (`crates/sagaline/src/main.rs`)** — replace the
   `println!` placeholder with a real top-level `App` that owns
   the agent + the workspace view and opens the `sagaline-ui`
   components.
6. **BYOK key store** — encrypted storage under
   `~/.sageline/data/keys.db`, age-encrypted, machine-bound.
   Provider config reads from here.
7. **Hosted project sync** — when `../homeweb/` ships the project-
   sync endpoint, add an opt-in client. This is the only piece of
   this project that talks to `homeweb/`, and it must stay opt-in
   (the open source pipeline must work without it).

## External dependencies — approval gate

This product ships to end users. Every external dependency we add
becomes part of the shipped binary's supply chain (linkage, bundled
artifacts, build prerequisites, license obligations, security CVEs,
platform coverage). **Any new external dep — Rust crate, system
binary, native library, font, model weight — MUST be approved by the
user before it lands in `Cargo.toml`, `[build-dependencies]`,
`build.rs`, or any compose / shell pipeline.**

What counts as "external":

- Any new entry in `[workspace.dependencies]` or a member crate's
  `[dependencies]`.
- Anything `cargo install`-style that the user must fetch to build.
- Anything invoked at runtime via `Command::new(...)`, shell pipeline,
  or `PATH` lookup (`ffmpeg`, `git`, `node`, system fonts, …).
- Bundled native libraries (e.g. `rusqlite` `bundled` feature =
  ships a SQLite amalgamation; `reqwest` `rustls-tls` = ships a
  TLS stack).

What does NOT need approval (the project already relies on these):

- Crates already in `[workspace.dependencies]` before this gate.
- Patches / version bumps of an already-approved crate within the
  same feature set.

Approval process:

1. **Surface the decision early**, before writing the dep into a
   manifest. List the alternatives considered and the tradeoff
   (license, bundle size, platform support, maintenance, native
   build burden).
2. **Stop and wait for the user's call.** Do not commit a
   `Cargo.toml` entry, `build.rs`, or `Command::new` against an
   unapproved dep. If mid-implementation, revert the partial work.
3. **Record the decision** in this file (a one-line entry under
   "Approved external deps" below) once granted, so the next agent
   doesn't re-ask.

Pre-existing in this repo (carry-over, approved by prior turns):
none yet under this gate — this section is new.

Approved external deps (populated as the user grants each one):

- `tokio` v1 — async runtime. Approved 2026-09-16 (this turn). Used by
  provider HTTP clients; brings no native deps of its own.
- `reqwest` v0.12 with default-features = false, features =
  `["json", "rustls-tls", "stream"]` — HTTP client. Approved
  2026-09-16 (this turn). `rustls-tls` avoids the OpenSSL system
  dependency. `stream` is required for downloading video mp4
- `redb` v2 — pure-Rust embedded KV store for the key DB.
  Approved 2026-09-16. Final pick over `sled` (which has entered
  maintenance mode).
- `secrecy` v0.10 — wraps API keys so they don't accidentally
  `Display` / `Debug`. Approved 2026-09-16.
- `age` v0.11 with features = `["std", "armor"]` — X25519-based
  encryption of stored keys. Approved 2026-09-16. Provides the
  recipient/identity abstraction.
- `redb` v2 — pure-Rust embedded KV store for the key DB.
  Approved 2026-09-16. Final pick over `sled` (which has entered
  maintenance mode).

- `mp4` crate v0.x — pure-Rust mp4 muxer for `compose_video`. Approved
  2026-09-16. Concrete version pinned when the tool lands.
- Job store: reuses `redb` from the existing `sagaline-keys` key
  store — the jobs table lives alongside `provider_key` in the
  same `~/.sageline/data/keys.db` redb file. Approved 2026-09-16.
  Final pick over a separate SQLite/JSON store.
- `rig-core` v0.42.0 — opinionated LLM SDK (model providers +
  tool surface). MIT licensed; compatible with our Apache-2.0.
  Approved 2026-09-16. We use only the model + tool surface; the
  classic agent runtime is intentionally NOT adopted (sagaline owns
  its OBSERVE → PLAN → ACT → REFLECT outer loop). Pin:
  `rig-core = "=0.42.0"`. Features enabled: default-features off,
  plus `providers`; within providers, `openai` only (other chat
  providers added per-feature as users ask for them). We also enable
  rig's `image_generation` module so OpenAI-compatible image gen
  (gpt-image-1 etc.) routes through rig. Audio / vector store /
  rerank / transcription modules are NOT enabled. No telemetry
  features (monorepo rule).
- `wiremock` v0.6 — HTTP mock server for integration tests of the
  OpenAI-compatible chat and MiniMax native image backends.
  Approved 2026-09-16. Dev-dependency only; never ships.

