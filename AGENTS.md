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
    ├── sagaline-store/               world state (SQLite) — Source of Truth
    │   ├── Cargo.toml                 name = "sagaline-store"
    │   └── src/                       World, repos, migrations, audit log
    ├── sagaline-core/                domain types + Markdown render layer
    │   ├── Cargo.toml                 name = "sagaline-core"
    │   └── src/                       entity / frontmatter / render (no I/O)
    ├── sagaline-agent/               OBSERVE → PLAN → ACT → REFLECT loop + tool registry
    │   ├── Cargo.toml                 name = "sagaline-agent"
    │   └── src/                       Tool trait, Agent::run, AgentEvent stream
    ├── sagaline-ui/                  gpui-kit views (workspace + activity panel)
    │   ├── Cargo.toml                 name = "sagaline-ui"
    │   └── src/                       WorkspaceView, AgentEventLog renderer
    ├── sagaline-bridge/             thin Tokio ↔ GPUI bridge (spawn / forward / drop = cancel)
    │   ├── Cargo.toml                name = "sagaline-bridge"
    │   └── src/                      TokioBridge, BridgeSlot gpui global
    ├── sagaline-providers/           model adapter traits + provider backends
    │   ├── Cargo.toml                 name = "sagaline-providers"
    │   └── src/                       ModelAdapter, ImageGen, openai_compat chat
    └── sagaline/                     the desktop binary
        ├── Cargo.toml                 name = "sagaline"
        └── src/{main.rs,env.rs}        AppEnv + run_agent + gpui main
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

Current member crates: `sagaline-store`, `sagaline-core`,
`sagaline-agent`, `sagaline-ui`, `sagaline-bridge`,
`sagaline-providers`, `sagaline`.
`sagaline-keys` (the old redb-backed BYOK + jobs crate) was
deleted this phase; its rows now live in the `provider_key`
and `jobs` tables of `sagaline-store` (V008).
Add new crates as siblings under `crates/` and add them to
`members = […]` in the workspace `Cargo.toml`.

The seven crates line up with the agent's anatomy (see
`SAGALINE.md`):

- `crates/sagaline-store/` — **world state (Source of Truth)**.
  Single SQLite database at `~/.sageline/data/world.db` holding
  story entities (`stories`, `characters`, `ages`, `appearances`,
  `references`, `environments`, `props`, `chapters`, `scenes`,
  `shots`, join tables), `proposals` + `proposal_actions`
  (AI mutation workflow), `story_versions` (rollback),
  `agent_actions` (audit log), and the existing `provider_key` /
  `jobs` tables. Typed repos
  (`StoryRepo`, `CharacterRepo`, `SceneRepo`, `ProposalRepo`,
  `VersionRepo`, `AuditRepo`) borrow from `World`; multi-table
  mutations (e.g. `create_scene`) take `&Connection` so the
  caller controls the transaction boundary.
- `crates/sagaline-core/` — story domain types (`Story` /
  `Bible` / `Character` / `Environment` / `Prop` / `Chapter` /
  `Scene` / `Shot` / `Reference`) and the Markdown render layer
  (`frontmatter`, `render_preview_lines`) used by the agent's
  context compiler and by `sagaline-ui`'s preview pane. No
  filesystem I/O — reads come through `sagaline-store`. The
  on-disk story directory becomes an optional exporter, not the
  database. Also serves the agent's reflection step
  (`validate()` against the in-memory domain types).
- `crates/sagaline-agent/` — the agent itself. `Tool` trait +
 `ToolRegistry` (each tool carries its own JSON Schema so an LLM
 client can advertise them via function-calling), `AgentEvent`
 stream (the chain-of-thought the UI subscribes to),
 `Agent::run_stream()` driving the OBSERVE → PLAN → ACT →
 REFLECT loop per scene and returning an
 `impl Stream<Item = AgentEvent>` whose `poll_next` does real I/O
 (e.g. `tokio::fs::read`). The runtime that polls the stream is
 the bridge's Tokio executor, never GPUI's — see
 `sagaline-bridge` below. The legacy `Agent::run(path, &mut sink)`
 is kept as a thin drain for callers that still want a
 `StepOutcome`. Tools: `ReadFileTool`, `WriteFileTool`,
 `ListDirTool`, `FindTool`, `ValidateStoryTool`,
 `GenerateImageTool`. PLAN / REFLECT are canned (deterministic
 over the scene's frontmatter); the LLM swap is the next
 priority.
- `crates/sagaline-ui/` — gpui-kit views. `WorkspaceView` is a
 two-pane layout: left pane = story tree grouped by entity type
 (Bible / Characters / Environments / Props / Chapters), right
 pane = tabbed (**Preview** for the selected entity's domain
 fields, **Activity** for a live scrollback of the agent's
 event stream, **Keys** for the BYOK panel). The UI never
 names a directory path, a slug, or a Markdown filename in its
 user-visible surface — see the "Storage vocabulary is
 off-limits" rule below. `AgentEventLog` is the gpui global the
 app shell pushes events into; the activity panel reads it.
 `StoryServiceSlot` is the gpui global the binary installs to
 bridge the UI to the binary's [`StoryStore`]; the UI reaches
 it through the `StoryService` trait and never sees the binary
 crate. **Any I/O that needs `tokio::fs` or `reqwest` must go
 through `sagaline_bridge::TokioBridge`** (the global installed
 by the binary) — `cx.spawn` runs on GPUI's scheduler, not
 Tokio, and calling Tokio APIs from inside a GPUI task panics
 with "no reactor running".
- `crates/sagaline-bridge/` — thin Tokio ↔ GPUI bridge, modelled
 on Zed's `gpui_tokio`. GPUI's `BackgroundExecutor` runs futures
 on GPUI's own scheduler (`gpui-pre-scheduler`), **not** on
 Tokio; calling `tokio::fs::read` / `tokio::task::spawn_blocking`
 from a GPUI task panics at runtime because no reactor / blocking
 pool is in scope. [`TokioBridge`] is the seam: `spawn` /
 `spawn_blocking` / `forward_to` / `forward_stream` schedule a
 future on the bridge's Tokio runtime via
 `Handle::spawn`, then poll the resulting `JoinHandle` on a
 GPUI background task. Dropping the GPUI task drops the
 `JoinHandle`, which aborts the Tokio future — the cancellation
 contract the binary's `run_agent` relies on when the user
 closes the window. The binary installs a `BridgeSlot` gpui
 global wrapping a `TokioBridge` so views reach it without
 depending on the binary crate. **Do not call any `tokio::*`
 API directly from `cx.spawn` / `cx.background_executor().spawn`
 — route through the bridge.**
- `crates/sagaline-providers/` — model-adapter traits
 ([`ModelAdapter`], [`ImageGen`], [`Tts`], [`ImageToVideo`]) and
 provider backends. Native `minimax` image backend; rig-core
 OpenAI-compatible chat factory in
 `openai_compat::build_chat` (covers OpenAI, DeepSeek, Ollama,
 MiniMax chat, Tongyi). `ProviderConfigSet` parses
 `~/.sageline/data/config.toml`. `ProviderRegistry` is the
 capability-keyed dispatch.
- `crates/sagaline/` keeps the binary thin — `main.rs` boots
 gpui, builds `AppEnv` (an `Arc<SagalineStore>` +
 `ProviderConfigSet` + `ProviderRegistry` + `Prefs` +
 `Arc<dyn StoryStore>`) and installs the relevant pieces as
 gpui globals (the `StoryService` slot, the `KeyStoreSlot`,
 the `AgentEventLog`, the `BridgeSlot`). It subscribes to
 `WorkspaceView::StoryOpened` to spawn the agent loop via
 `run_agent(env, Arc<Story>, cx)`. The agent's events flow
 `Agent::run_stream` → `bridge.forward_stream(...)` → mpsc →
 `AgentEventLog` global → activity panel. `AppEnv` owns the
 multi-thread Tokio runtime (`env.runtime()`) the bridge's
 `Handle` is cloned from — see `env.rs`. Persistent
 preferences (currently just the project location) live in
 `prefs.rs` and are written to

### Storage vocabulary is off-limits in the UI

The user talks in domain terms — story, character, scene, shot.
They never see "folder", "Markdown", "slug", "directory",
"parent directory", "front matter", or a bare `story.md`
filename in any user-visible string. The implementation
details belong to the on-disk model; the user surface is the
`Story` / `StoryStore` / `ProjectLocation` façade.

Concretely:

- **Tree labels** use `entity.title()` from the front matter,
  never `entity.slug`. Indentation is the tree widget's job;
  no leading-whitespace padding in labels. The five top-level
  groups are domain types (Bible / Characters / Environments /
  Props / Chapters); no "(folder)" annotation. A chapter's
  scenes are direct children of the chapter — no nested
  "Scenes in <slug>" sub-folder.
- **Preview pane** uses `sagaline_core::render_preview_lines`
  to render a `(label, value)` definition list keyed by entity
  type. Never dumps raw `serde_yaml` front matter or the
  Markdown body to the user.
- **New-story dialog** takes a story title only. The story's
  on-disk path / slug are derived internally by
  `FileStoryStore::create(title)`. The dialog shows a hint
  like "A new project will be created in {location}." — the
  location is the `ProjectLocation` chosen in Project
  Settings, not a "parent directory" field.
- **File picker prompts** are "Open a story" and "Where would
  you like to keep new stories?" — never "Open story
  directory" / "Choose parent directory for the new story".
- **Banner shortcut hint** is
  `⌘ N new story   ⌘ O open story   ⌘ , project settings`
  (no `—` separator, no `directory`).

The `StoryService` trait in `sagaline-ui/src/state.rs` is the
only seam the UI crosses into the binary crate; the binary's
concrete impl (`AppEnvStoryService`) is installed as the
`StoryServiceSlot` gpui global at startup. New UI code that
wants to mutate a story or change the project location goes
through this trait, never through `StoryRoot::init` or
`StoryRoot::new` directly.

### Storage model — SQLite World State (Source of Truth)

**The world DB is the database — and the database is the agent's
memory.** A story lives in `~/.sageline/data/world.db` (SQLite,
opened via `sagaline_store::World::open`). The agent never reads
or writes SQL directly; it goes through domain Tools in
`sagaline-agent` (e.g. `create_scene`, `update_character`,
`propose_change`) whose only allowed mutation path is
Proposal → Validation → Commit. The on-disk story directory is an
**optional exported view** (rendered by `sagaline-store`'s
`MarkdownExporter`) for git diffs and human inspection — agents
never touch it directly.

Layout the exporter produces (legacy / git-friendly view only):

```text
<user-chosen>/<story-slug>/
├── story.md                         # exported story entry
├── bible/<slug>.md
├── characters/<slug>/character.md
├── environments/<slug>/environment.md
├── props/<slug>.md
├── chapters/<NNN-slug>/
│   ├── chapter.md
│   └── scenes/<NNN-slug>.md
└── assets/                          # generated images / videos / audio (gitignored)
```

- **Single world DB**, `~/.sageline/data/world.db`. Holds
  `stories` / `chapters` / `scenes` / `shots` / `characters` /
  `ages` / `appearances` / `references` / `environments` /
  `props` / join tables, plus `proposals` + `proposal_actions`,
  `story_versions`, `agent_actions`, and the existing
  `provider_key` + `jobs` tables (the latter two fold in
  `sagaline-keys` (deleted this phase).
- **Stable IDs** are UUIDs; `slug` is a unique secondary index
  per table (exported path derives from slug).
- **Agent write path is always transactional.** Multi-table
  mutations (`create_scene` writes 6 tables) take `&Connection`
  so the caller controls the transaction; either all rows land
  or none do. `validate()` runs inside the same transaction
  before commit.
- **Agent never sees SQL.** Tools accept JSON args, return JSON
  results; the only allowed mutation channel is Proposal →
  Commit (live in Phase 3 + 4). The four gate tools live in
  `crates/sagaline-agent/src/tools/proposal.rs`:
  `propose_change`, `list_pending_proposals`,
  `approve_proposal`, `reject_proposal`. `Agent::dispatch_tool`
  honors `AgentConfig::commit_policy`: `Auto` (default — write
  inline) or `Manual` (record a pending proposal, never touch
  the world DB). The binary flips the policy via the
  `SAGALINE_COMMIT_POLICY` env var. **Phase 2.5** rewired
  `Agent::run_stream` to drive the SQLite `World` directly
  (no Markdown round-trip); the loop now invokes
  `validate_world` per scene.

### Tool tier surface (Phase 2 — landed)

Every agent tool declares a [`Capability`] tier via the
[`ToolDescriptor`] — see `crates/sagaline-agent/src/tool.rs`.
The tier is what `AgentConfig` (Phase 4) gates against, and
what the UI's "tools" tab in the activity panel groups by.

| Tier | Reads / Writes | Examples (Phase 2 surface) |
|---|---|---|
| `Capability::Read` | read-only world queries | `get_story`, `list_stories`, `search_story` |
| `Capability::Mutate` | domain mutations → Phase 3 Proposal gate | `create_story`, `update_character`, `add_character_age`, `add_character_appearance`, `create_chapter`, `create_scene` (3 tables, one tx), `assign_character_to_scene`, `assign_environment_to_scene`, `create_shot`, `create_environment`, `create_prop` |
| `Capability::Execute` | side-effects outside the world | `generate_image` (today) — image / video / audio backends route here as they land |

`validate_world` is `Capability::Read` — it reports issues but
doesn't mutate. Multi-table writes like `create_scene` (one
Scene row + `scene_characters` assignments + a
`scene_environments` link) all run inside a single
`Connection::transaction`, so either every row lands or none
does.

Phase 2 ships **15 domain tools** under
`crates/sagaline-agent/src/tools/` (excluding `generate_image`,
which is a thin wrapper around the provider registry):

- `story.rs` — `get_story`, `list_stories`, `search_story`
- `character.rs` — `create_character`, `update_character`,
  `add_character_age`, `add_character_appearance`
- `chapter.rs` — `create_chapter`
- `scene.rs` — `create_scene` (multi-table atomic tx: scene row
  + `scene_characters` assignments + `scene_environments` link),
  `assign_character_to_scene`, `assign_environment_to_scene`
- `shot.rs` — `create_shot`
- `environment.rs` — `create_environment`
- `prop.rs` — `create_prop`
- `world_validate.rs` — `validate_world` (replaces
  `validate_story`; checks cross-story + orphan
  age / appearance refs; FK constraints cover the rest)

Tool trait shape:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;          // now includes capability
    async fn execute(
        &self,
        ctx: ToolContext,                            // world + agent_id + proposal_id
        args: Value,
    ) -> Result<ToolResult, ToolError>;
}
```

`ToolContext` carries the shared `Arc<World>` so every tool
can resolve the right repo without the loop having to thread
it through. `proposal_id` is plumbed today but optional —
Phase 3 will make it required for `Capability::Mutate` tools.

`ToolRegistry::by_capability()` groups registered tool names
by tier for the UI; `descriptors_with_capability(cap)` is the
filtering helper `AgentConfig` will call.

Integration test coverage lives in
`crates/sagaline-agent/tests/domain_tools.rs` (15 tests, one
per tool, all passing).

#### Legacy filesystem tools (deleted in Phase 2.5)

`read_file`, `write_file`, `list_dir`, `find`, and
`validate_story` were deleted in Phase 2.5 once the loop
started driving `World` directly. `generate_image` remains —
it's a `Capability::Execute` provider tool, not a filesystem
one. If you find yourself reaching for filesystem I/O in a
new tool, route through the world DB or a provider backend
instead.

### Persistence & secrets

- **One world DB** at `~/.sageline/data/world.db` (SQLite,
  bundled). Story content, jobs, provider keys, and the audit
  log all live here. There is no separate `keys.db` once
  `sagaline-keys` was folded in (Phase 1 — completed this turn).
- **Domain types in `sagaline-core`** (`Story`, `Bible`,
  `Character`, …, `Reference`) are the in-memory row mappings
  used by the agent tools, the context compiler, and the UI.
  No filesystem I/O lives in `sagaline-core` anymore.
- **User preferences** (currently just `ProjectLocation`) live
  in `~/.sageline/data/prefs.toml` (a small plaintext TOML
  file, NOT in the world DB). Read by `Prefs::load` at
  `AppEnv::open`; written by `AppEnv::set_project_location`.
  Distinct from the world DB because the prefs are non-secret,
  machine-local, and frequently mutated.
- **Project location** — chosen by the user once, via the
  Project Settings modal (⌘ ,). Persisted in `prefs.toml`; the
  optional on-disk exporter is rebuilt on every export. The
  core crate does not assume any default location. The
  `OpenStory` action (⌘ O) still lets the user open a story
  from anywhere — it doesn't have to live under the project
  location.
- **BYOK keys** — machine-bound, age-encrypted. Stored in the
  world DB's `provider_key` table. Read at startup via
  `sagaline_store::World::keys().get(...)`, never written to a
  global location, never phoned home.

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
cargo test  --workspace         # ~156 passing tests across 7 crates
cargo build -p sagaline         # the desktop binary
```

A change is done when `cargo test --workspace` is clean. Core
adds 7 new tests in `sagaline-core/src/story.rs` (4 covering
`FileStoryStore::create` + `list` + slug-derivation + the
missing-location error path) and `sagaline-core/src/render.rs`
(3 covering `render_preview_lines` for Scene / Shot /
Character). The agent crate's integration test
(`tests/loop_smoke.rs`) builds a temp story, runs `Agent::run`,
and asserts the OBSERVE → PLAN → ACT → REFLECT event stream
end-to-end. `crates/sagaline-agent/tests/bridge_stream.rs` is
the end-to-end guard for the Tokio ↔ GPUI bridge — it routes
`Agent::run_stream` through `TokioBridge::forward_stream` and
asserts the `Act` event's `result_summary` proves `tokio::fs`
actually returned bytes (the regression that motivated the
bridge in the first place). The providers crate has
`tests/{minimax_image,minimax_image_smoke,openai_compat_chat}.rs`
that exercise the wire shape with `wiremock`. The bridge crate
has `tests/bridge.rs` covering the cancellation contract
(drop = abort) and the runtime-routing contract
(`Handle::current()` from inside the spawned future).

The 4 new `FileStoryStore` tests are the contract test for the
UI-facing API: they assert that `create(title)` returns an
`Arc<Story>` whose `title` is the input, and that the second
create with the same title produces a distinct
`StorySummary::handle` (proving slug auto-derivation). They do
NOT inspect the on-disk path, the `.md` filename, or the slug
string — the public `Arc<Story>` exposes none of those.

For visual changes, `cargo run` and confirm the window opens.
When the change touches the UI's storage surface, the manual
smoke is:

- Onboarding copy mentions the agent, not Markdown / folders.
- ⌘ , opens Project Settings; "Choose…" prompts
  "Where would you like to keep new stories?".
- ⌘ N opens a new-story dialog with only a title input.
- The tree labels are entity titles (no slugs, no
  "(folder)" annotations, no nested "Scenes in <slug>"
  sub-folders).
- The Preview pane shows a domain field list, not raw
  YAML.

## Known carry-over

- `crates/sagaline-agent/` PLAN / REFLECT use the LLM when one is
  attached, canned otherwise. The plumbing is done: `Agent::with_llm`
  takes an `Arc<dyn LlmClient>`, `loop_.rs` routes PLAN + REFLECT
  through it (`tracing::warn!` + canned fallback on LLM error), and
  `AppEnv::build_agent` wires `RigLlm` automatically whenever a chat
  provider + key are configured in `~/.sageline/data/`. Without that
  pair the loop runs the deterministic scaffold (used by tests and
  headless runs). `crates/sagaline-providers/` ships two image
  backends: a native `minimax` adapter and the OpenAI
  `gpt-image-1` adapter registered under the `openai` key, so
  `registry.pick_image("openai")` now routes correctly.
- `sagaline_agent::tools::generate_image` already patches the parent
  shot's front matter with `assets.keyframe` (path relative to the
  enclosing story root) and `status: succeeded` when `shot_path` is
  supplied on the tool call. `round_trip_smoke.rs` covers the
  round-trip. When `shot_path` is omitted the tool just writes bytes
  and does not touch any shot file.
- Preview / Activity tab switching is wired: `render_tab_label` in
  `crates/sagaline-ui/src/view.rs` dispatches `SwitchTab { tab }`
  on click, and the handler registered by `register_actions` calls
- The storage-agnostic `Story` / `StoryStore` / `ProjectLocation`
  façade in `sagaline-core` is wired into the UI (via the
  `StoryServiceSlot` gpui global) and into the binary's
  `AppEnv`. `StoryRoot::init` and `StoryRoot::new` remain `pub`
  for the agent crate and the integration tests to seed
  fixtures; the agent does not need the façade. If a future
  feature wants to build stories programmatically (e.g. a
  hosted project-sync intake), it MUST go through
  `FileStoryStore::create(title)` — not through
  `StoryRoot::init(parent, slug, title)`, which would re-expose
  the slug / parent path parameters the façade was built to
  hide.
- The new-story dialog and the tree / preview pane never
  display slug / directory / front-matter vocabulary. If you
  add a new user-visible string, grep for `folder`, `Markdown`,
  `directory`, `slug`, `parent`, `front matter` in the diff
  before merging. Any new entity field the user sees goes
  through `sagaline_core::render::render_preview_lines`, not
  through `serde_yaml::to_string(&entity.frontmatter)`.
- `tracing` is initialized in `crates/sagaline/src/main.rs` via
  a `tracing-subscriber` registry with two layers — stderr (ANSI
  on) and `<data_dir>/sagaline.log` (ANSI off, append). Both are
  gated by `RUST_LOG`; default level is `warn`. `try_init()`
  makes the boot idempotent.
- `Cargo.lock` is gitignored (intentional for a binary; keep it
  that way).


## Deferred (next turns)

In priority order — do not skip ahead. The order is driven by
`SAGALINE.md`: the agent loop is running end-to-end (LLM-backed
when a chat provider + key are configured; canned otherwise), the
providers feed it real actions, and the remaining work is the
chat-side UX surface (key entry, chat client selection, editing,
batch generation, project sync).

### Phase 3 — Proposal → Commit workflow (landed 2026-09-19)

The mutation gate is live:

- **Four gate tools** in `crates/sagaline-agent/src/tools/proposal.rs`:
  `propose_change` (Mutate) records a pending proposal +
  N rows in `proposal_actions` but does not mutate the world.
  `list_pending_proposals` (Read) lists them. `approve_proposal`
  (Mutate) validates, then replays each action via the
  registry's `Tool::execute` and flips the proposal to
  `committed`. `reject_proposal` (Mutate) flips to `rejected`.
- **Pre-flight validation.** `validate_world_in_tx` (moved from
  the `validate_world` tool into `sagaline_store::validation`)
  runs first inside `approve_proposal`. Any issue → world DB
  untouched, proposal stays `pending`, error returned.
- **Audit log.** Every tool call records a row in
  `agent_actions` with `(story_id, agent_id, tool_name,
  args_json, result_summary, proposal_id?, created_at)`. The
  `agent_actions` repo exposes `record` + `record_in_tx`,
  `list_by_story`, `list_by_proposal`, and `latest_for_tool`.
- **Atomicity.** Each replayed action opens its own
  connection in Phase 3, so a mid-replay failure leaves the
  world DB partially applied. Phase 5 changes this: see
  "Phase 5 — single-transaction atomicity" below.

Phase 3 adds no new external deps; it builds on Phase 2's
`ToolContext` and `Capability` plumbing. Schema was already
in V006 / V007 (landed Phase 1).

### Phase 4 — `AgentConfig::commit_policy` + manual gate (landed 2026-09-19)

The mutation gate is now configurable per-agent:

- **`CommitPolicy` enum** in `crates/sagaline-agent/src/loop_.rs`:
  `Auto` (default) — mutations land inline, same as before.
  `Manual` — every `Capability::Mutate` call is routed through
  `record_single_action` (a single-action pending proposal)
  instead of executing; the world DB stays untouched until
  `approve_proposal`. Read / Execute tools are unaffected.
- **`Agent::dispatch_tool(ctx, tool_name, args)`** — the public
  entry point that honors the policy. The binary calls it
  instead of `tool.execute` directly. Phase 2.5 will switch
  the loop's internal `tool.execute(...)` call site over too.
- **`AgentConfig::commit_policy`** field with `Auto` default;
  flipped via `Agent::with_config`.
- **Binary knob:** `AppEnv::build_agent` reads
  `SAGALINE_COMMIT_POLICY` (`auto` / `manual`; unknown values
  fall back to `Auto` with a `tracing::warn!`). The UI binding
  that surfaces the picker on the activity panel is the next
  turn.
- **Audit row tag:** when `Manual`, the audit row's `tool_name`
  is `record_single_action` (the gate), not the underlying
  tool. The underlying tool name + args live in the
  `proposal_actions` row, so the chain of evidence
  (`agent_actions.proposal_id` → `proposal_actions.tool_name`)
  is intact.

Phase 4 owns no new external deps.

### Phase 5 — single-transaction atomicity (landed 2026-09-19)

`approve_proposal` now wraps everything between the
pre-flight `validate_world` check and the `committed` flip
in ONE outer SQLite transaction. A failure anywhere inside
the transaction drops it without `commit()`; the world DB
stays at its pre-approve state and the proposal stays
`pending`.

- **New trait seam** in `crates/sagaline-agent/src/tool.rs`:
  `Tool::execute_in_tx(&ctx, &tx, args)` (sync — `rusqlite::
  Transaction` is `!Send`) plus `Tool::supports_in_tx()`
  defaulting to `false`. Contract: implementations must NOT
  call `tx.commit()`; the caller owns the boundary.
- **Tools that opted in** (return `supports_in_tx = true`):
  `create_character`, `create_chapter`, `create_environment`,
  `create_prop`, `create_scene`. Each has a paired
  `execute_in_tx` that calls the matching repo's
  `*_in_tx` method (`CharacterRepo::create_in_tx`,
  `SceneRepo::create_chapter_in_tx`,
  `EnvironmentRepo::create_in_tx`, `PropRepo::create_in_tx`,
  `SceneRepo::create_scene_in_tx`).
- **`approve_proposal` rewrite** in
  `crates/sagaline-agent/src/tools/proposal.rs`:
  - Pre-parses every action's args JSON and looks up every
    tool in the registry (catches unknown tools / malformed
    args BEFORE any state changes).
  - **Phase 1 (async replays).** Tools without
    `execute_in_tx` run via `tool.execute(...)` first. Each
    opens its own connection and commits independently. This
    is the only `.await` in the function, so we have to do it
    before opening the outer `Transaction` (the `!Send` tx
    can't cross the `.await`). Their writes cannot be rolled
    back by the outer tx — Phase 5 caveat for non-in_tx
    tools.
  - **Phase 2 (in-tx replays).** Opens `Connection::
    transaction`, runs `validate_world_in_tx(&tx, ...)`,
    then replays every in-tx action via `tool.execute_in_tx
    (&ctx, &tx, args)` (sync). Each replay's audit row lands
    via `agent_actions.record_in_tx(&tx, ...)`.
  - Flips the proposal to `committed` via
    `proposals.set_status_in_tx`, writes the approve-call
    audit row via `agent_actions.record_in_tx`, then
    `tx.commit()`. Any error inside Phase 2 → tx dropped
    without commit; the world DB is unchanged.
- **Tests** in `crates/sagaline-agent/tests/proposal_tools.rs`:
  - `approve_atomic_rolls_back_on_mid_failure` — proposes 3
    `create_character` actions where #2 collides with #1 on
    `slug`. Expects: world DB has 0 characters, proposal
    status `pending`, audit log has only the propose row.
  - `approve_atomic_commits_all_on_success` — proposes 3
    distinct `create_character` actions. Expects: all 3 land,
    proposal `committed`, 4 audit rows (3 replays + 1
    approve-call) on top of the propose row.
- **Phase 5 caveat.** Tools without `execute_in_tx`
  (`update_character`, `add_character_age`,
  `add_character_appearance`,
  `assign_character_to_scene`,
  `assign_environment_to_scene`, `create_shot`) still fall
  back to per-tool auto-commit. Their world-DB writes are
  NOT rolled back by the outer tx — they participate in
  Phase 1 and the user accepts that the world may end up
  partially applied if a later in-tx action fails. The
  supported-tools list grows as each domain tool opts in
  (Phase 5 follow-up work).

Phase 5 adds no new external deps.

### Phase 2.5 — rewire the agent loop (highest priority)

### Phase 2.5 — rewire the agent loop to drive `World` (landed 2026-09-19)

The loop now drives the SQLite [`sagaline_store::World`]
instead of the Markdown-era [`sagaline_core::StoryGraph`].
Phase 2.5 ships:

- **`Agent::run_stream(world: Arc<World>, story_id: &str)`** —
  new signature. Scene iteration comes from
  `world.scenes().list_scenes_for_story`; the OBSERVE event's
  `ResolvedContext` is built from `scene_characters` +
  `scene_environments` rows (no frontmatter re-parsing).
- **`Agent::run(world, story_id, sink)`** — the legacy
  `run(path, sink)` API is gone; the new sink API takes
  `Arc<World>` + `story_id`.
- **ACT step** now calls the `validate_world` tool (a
  `Capability::Read` tool) with `{story_id}`. The
  `result_summary` is `"ok"` or `"N validation issue(s)"`.
- **PLAN / REFLECT** dispatch through `LlmClient` as before;
  `llm::scene_body` now takes a `SceneRow` and renders the
  scene's title + synopsis.
- **Legacy filesystem tools deleted**: `find.rs`,
  `list_dir.rs`, `read_file.rs`, `write_file.rs`,
  `validate_story.rs` are gone from `tools/`. `generate_image`
  stays (it's a provider I/O tool, not filesystem).
- **`ToolRegistry` is now `Clone`** — internal storage is
  `BTreeMap<String, Arc<dyn Tool>>` instead of `Box<dyn>`,
  so the binary can snapshot a registry (`into_arc()`) and
  hand it to `ApproveProposalTool::with_registry` without
  keeping the agent's `&mut` borrow.
- **Tests migrated**: `loop_smoke.rs`, `bridge_stream.rs`,
  `llm_loop_smoke.rs` now seed an in-memory world via
  `World::in_memory()` and assert the new event shape
  (Act calls `validate_world`; summary is `"ok"`).
- **Scene repo additions**: `list_chapters_for_story`,
  `list_scenes_for_story`, `character_ids_for_scene`,
  `environment_ids_for_scene`, `_count`.

### Smaller items

1. **Tts + ImageToVideo backends** — `Tts` and `ImageToVideo`
   traits are defined but no provider has implemented them yet.
   The registry can route calls as soon as one does.
2. **Hosted project sync** — when `../homeweb/` ships the
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

- `mp4` crate v0.x — pure-Rust mp4 muxer for `compose_video`. Approved
  2026-09-16. Concrete version pinned when the tool lands.
- (superseded 2026-09-18; resolved 2026-09-22) Job store used
  to reuse `redb` from the existing `sagaline-keys` key store.
  Both `provider_key` and `jobs` now live in SQLite via the V008
  migration, and the `sagaline-keys` crate has been deleted from
  the workspace. Follow-up audit 2026-09-22 confirmed no `.rs`
  file imports `redb::*`; the workspace dep entry was removed
  from `client/Cargo.toml` on this turn.
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

Approved 2026-09-18 (this turn) for `sagaline-store` (Phase 1 of
the SQLite-world-state pivot):

- `rusqlite` v0.32 with `bundled` feature — SQLite driver,
  compiled from source so the desktop binary doesn't depend
  on a system libsqlite3. Approved 2026-09-18.
- `r2d2` v0.8 — generic synchronous connection pool. Approved
  2026-09-18. Final pick over `deadpool-sqlite` (which is
  async-only and we run sync calls via `spawn_blocking`).
- `r2d2_sqlite` v0.25 — `r2d2` adapter for `rusqlite`.
  Approved 2026-09-18. Pins to `rusqlite` 0.32.x.
- `refinery` v0.9 with feature `rusqlite-bundled` — embedded
  SQL migration runner. Approved 2026-09-18. Migration files
  under `crates/sagaline-store/migrations/` are compiled into
  the binary via `embed_migrations!`.
- `uuid` v1 with features `["v7", "serde"]` — stable entity
  IDs. Approved 2026-09-18. `v7` chosen for time-ordered
  (B+Tree-friendly) primary keys.
- `chrono` v0.4 with `default-features = false`, features
  `["std", "clock", "serde"]` — timestamps. Approved 2026-09-18
  (replaces an inline stdlib date-math implementation that
  turned out to be too brittle to maintain).

Approved 2026-09-22 (this turn):

- `tracing-subscriber` v0.3 with feature `env-filter` — the
  subscriber used to install the global tracing handler in
  `crates/sagaline/src/main.rs`. MIT licensed. Chosen over
  alternatives (`fern`, `log` + `env_logger`) because it is the
  official companion crate to `tracing` and already in the same
  workspace family as the `tracing = "0.1"` we use throughout.
  Two layers: stderr (ANSI on) for interactive runs, append to
  `<data_dir>/sagaline.log` (ANSI off) for post-mortem. Both
  gated by `RUST_LOG`; default level `warn`. `try_init()` keeps
  the boot idempotent.

On legacy `keys.db` (2026-09-18): the previous redb file at
`~/.sageline/data/keys.db` is intentionally NOT migrated into
the new world DB. On first launch of the world DB, the old
`keys.db` is deleted; users re-enter provider keys through the
BYOK panel.
