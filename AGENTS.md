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
    ├── sagaline-ui/                  gpui-kit views (workspace + activity panel)
    │   ├── Cargo.toml                 name = "sagaline-ui"
    │   └── src/                       WorkspaceView, AgentEventLog renderer
    ├── sagaline-keys/                unified redb store + age-encrypted keys
    │   ├── Cargo.toml                 name = "sagaline-keys"
    │   └── src/                       SagalineStore, KeyStore<'_>, JobStore<'_>
    ├── sagaline-providers/           model adapter traits + provider backends
    │   ├── Cargo.toml                 name = "sagaline-providers"
    │   └── src/                       ModelAdapter, ImageGen, openai_compat chat
    └── sagaline/                     the desktop binary
        ├── Cargo.toml                 name = "sagaline"
        └── src/{main.rs,env.rs,sink.rs}  AppEnv + run_agent + gpui main
```

The `crates/` layout is the seam where deferred work goes — add
new crates as siblings, not nested inside `sagaline/`.

## Stack

- **Language:** Rust 1.80+
- **UI:** `gpui-kit = "0.6"` — a single facade crate that re-exports
  GPUI + gpui-base + gpui-component + default assets. Used by
  `sagaline-ui` (workspace + activity panel) and `sagaline` (app
  shell).
- **Single dependency for the UI layer.** Don't add `gpui`,
  `gpui-base`, or `gpui-component` directly to your `Cargo.toml` —
  `gpui-kit` re-exports all three at the matching version. Drift
  between the facade and the internals is the most common source
  of "doc example doesn't compile" failures.

### Workspace layout

Current member crates: `sagaline-core`, `sagaline-agent`,
`sagaline-ui`, `sagaline-keys`, `sagaline-providers`, `sagaline`.
Add new crates as siblings under `crates/` and add them to
`members = […]` in the workspace `Cargo.toml`.

The six crates line up with the agent's anatomy (see
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
  loop per scene. Tools: `ReadFileTool`, `WriteFileTool`,
  `ListDirTool`, `FindTool`, `ValidateStoryTool`,
  `GenerateImageTool`. PLAN / REFLECT are canned (deterministic
  over the scene's frontmatter); the LLM swap is the next
  priority.
- `crates/sagaline-ui/` — gpui-kit views. `WorkspaceView` is a
  two-pane layout: left pane = story tree, right pane = tabbed
  (**Preview** for the selected entity's YAML+body, **Activity**
  for a live scrollback of the agent's event stream).
  `AgentEventLog` is the gpui global the app shell pushes events
  into; the activity panel reads it.
- `crates/sagaline-keys/` — unified [`SagalineStore`]: a single
  `redb::Database` at `~/.sageline/data/keys.db` holding both the
  encrypted key table (`provider_key`, age-encrypted with a
  per-machine X25519 identity) and the job table (`jobs`).
  `KeyStore` / `JobStore` are typed views that borrow from the
  store.
- `crates/sagaline-providers/` — model-adapter traits
  ([`ModelAdapter`], [`ImageGen`], [`Tts`], [`ImageToVideo`]) and
  provider backends. Native `minimax` image backend; rig-core
  OpenAI-compatible chat factory in
  `openai_compat::build_chat` (covers OpenAI, DeepSeek, Ollama,
  MiniMax chat, Tongyi). `ProviderConfigSet` parses
  `~/.sageline/data/config.toml`. `ProviderRegistry` is the
  capability-keyed dispatch.
- `crates/sagaline/` keeps the binary thin — `main.rs` boots
  gpui, installs `AppEnv` (an `Arc<SagalineStore>` +
  `ProviderConfigSet` + `ProviderRegistry`) as a global, and
  subscribes to `WorkspaceView::StoryOpened` to spawn the agent
  loop. The agent's events flow through a `ChannelSink` →
  `AgentEventLog` global → activity panel.
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
  crate's `AgentEventLog` is the global the app-shell
  forwarder pushes into; tests use `VecCollector`. Don't
  reintroduce callback-style hooks "for one-off convenience" —
  they serialize the UI thread.

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

- **No embedded story database.** `StoryGraph::load` walks the
  directory once and produces an in-memory graph; there is no
  SQLite / sled / redb / index layer for story content. The
  filesystem *is* the index.
- **Runtime state** (jobs, assets) lives in
  `~/.sageline/data/keys.db` (sagaline-keys) — same `redb` file
  as the encrypted key table. Story content never touches this
  store; the agent reads keys only from here.
- **Story root location** — chosen by the user (via ⌘ O in the
  desktop app). The core crate does not assume any default
  location.
- **BYOK keys** — machine-bound, age-encrypted. Stored at
  `~/.sageline/data/keys.db` (`provider_key` table). Read at
  startup via `sagaline_keys::SagalineStore::keys().get(...)`,
  never written to a global location, never phoned home.
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
cargo test  --workspace         # ~83 tests across 6 crates
cargo build -p sagaline         # the desktop binary
```

A change is done when `cargo test --workspace` is clean. The
agent crate's integration test (`tests/loop_smoke.rs`) builds a
temp story, runs `Agent::run`, and asserts the OBSERVE → PLAN →
ACT → REFLECT event stream end-to-end. Add to it (or to a
sibling `tests/foo.rs`) when you add a new tool, a new event
variant, or any other loop-touching change. The providers crate
has `tests/{minimax_image,minimax_image_smoke,openai_compat_chat}.rs`
that exercise the wire shape with `wiremock`.

For visual changes, `cargo run` and confirm the window opens.

## Known carry-over

- `crates/sagaline-agent/` PLAN / REFLECT are still canned
  (`build_canned_plan` in `loop_.rs` + a fixed "tool issue"
  REFLECT string). LLM swap is the next priority — the chat
  factory (`sagaline_providers::openai_compat::build_chat`) is
  ready and rig-core v0.42.0 is pinned. The LLM-aware loop is a
  prompt + client swap, not a redesign.
- `crates/sagaline-providers/` has a native `minimax` image
  backend but no OpenAI `gpt-image-1` adapter yet; an OpenAI
  image entry in `config.toml` is silently rejected at
  `registry.pick_image("openai")`. Adding the adapter is a
  single-trait impl.
- `sagaline_agent::tools::generate_image` writes the asset bytes
  to disk but does not update the shot's `assets.keyframe` /
  `status: succeeded` frontmatter fields. The follow-up is
  parsing the existing shot front matter, applying the patch,
  and writing it back via the existing `WriteFileTool`.
- The activity tab is the second tab; the user can't click
  between Preview and Activity yet (the tab bar renders the
  state but click handling isn't wired). A `SwitchTab` action is
  the trivial follow-up.
- `Cargo.lock` is gitignored (intentional for a binary; keep it
  that way).

## Deferred (next turns)

In priority order — do not skip ahead. The order is driven by
`SAGALINE.md`: Sagaline is a GUI agent, so the agent loop runs
first (with mock tools / canned PLAN), the providers feed it
real actions, the LLM swap replaces the canned responses, and
only then do the chat-side UX features (editing, batch
generation, project sync) come back into scope.

1. **Wire LLM into `sagaline-agent`** — replace the canned PLAN
   / REFLECT responses with real LLM calls. First backend:
   OpenAI-compatible chat completions via
   `sagaline_providers::openai_compat::build_chat`. The agent
   crate's `Tool` registry already speaks JSON Schema, so this
   is a prompt + chat client swap, not a redesign. The
   `LlmClient` trait + `RigLlm` impl will sit in
   `sagaline-agent`; the loop becomes PLAN → call LLM with the
   resolved context + tool schemas → dispatch tool calls →
   REFLECT with the LLM's critique.
2. **OpenAI `gpt-image-1` adapter** — the `ProviderRegistry` is
   already prepared for an image backend under the `openai`
   name; an `OpenAiImage` impl of [`ImageGen`] closes the gap so
   `pick_image("openai")` works.
3. **Generate-image tool → shot frontmatter round-trip** —
   `generate_image` writes the asset to disk but doesn't update
   the shot's `assets.keyframe` / `status: succeeded` fields.
   `sagaline_agent` needs a follow-up step that parses the
   shot's front matter, applies the patch, and writes it back.
   The `WriteFileTool` is already in the registry.
4. **Activity-tab click + SwitchTab action** — bind the tab bar
   in `sagaline-ui/src/view.rs` to a `SwitchTab` action. Trivial.
5. **BYOK key-entry UI** — a panel under the activity tab to
   list `ProviderKeyId`s, add new keys (paste a plaintext once
   → encrypted via the existing `SagalineStore::keys().put`),
   delete. Tied to item 1 — once a chat key is present, the
   "Run agent" button can actually use it.
6. **Tts + ImageToVideo backends** — `Tts` and `ImageToVideo`
   traits are defined but no provider has implemented them yet.
   The registry can route calls as soon as one does.
7. **Hosted project sync** — when `../homeweb/` ships the
   project-sync endpoint, add an opt-in client. This is the
   only piece of this project that talks to `homeweb/`, and it
   must stay opt-in (the open source pipeline must work without
   it).

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
