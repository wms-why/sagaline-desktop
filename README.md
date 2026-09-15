# Sagaline — desktop client

Open source AI video generator. Rust + GPUI Kit, Apache 2.0.
[中文版 →](./README.zh.md)

This is the desktop engine. The companion web project lives at
`../homeweb/` (TanStack Start, Cloudflare Workers).

## Design philosophy — Markdown as Source of Truth

A Sagaline story is not a database row. It is a **directory of Markdown
files** — the same files you'd edit in any text editor, version with
git, share as a zip, or point a different tool at. No proprietary
format, no vendor lock-in, no migration path to worry about.

```text
<story-slug>/
├── story.md                         # story entry (id / type / slug / title)
├── bible/<slug>.md                  # world / timeline / rules / lore
├── characters/<slug>/character.md
├── environments/<slug>/environment.md
├── props/<slug>.md
├── chapters/<NNN-slug>/
│   ├── chapter.md
│   └── scenes/<NNN-slug>.md
└── assets/                          # generated images / videos / audio
```

Each file is **Markdown + YAML front matter**:

```markdown
---
id: scene_001_laboratory
type: scene
slug: 001-laboratory
characters: [lin-mo, su-yan]
environment: laboratory
props: [energy-core]
---

# 实验室初遇

林默第一次见到陈博士……
```

The front matter is machine-readable (validated, indexable); the body
is human-readable (free Markdown). The two halves never get in each
other's way.

### What this means in practice

| Concern | Today (this turn) | Later phases |
|---|---|---|
| **Story content** | Filesystem only — Markdown + YAML | Stays files |
| **Generation jobs / assets / embeddings** | n/a | May live in `~/.sageline/data/index.db` (SQLite). Story content **does not move** into it |
| **BYOK keys** | n/a | `~/.sageline/data/keys.db`, age-encrypted, machine-bound |
| **Cross-machine sync** | n/a | Opt-in against the hosted cloud build (`homeweb/`) |

The rule is simple: **the filesystem is the database for everything
that is the story.** SQL reappears only for runtime state that has
no narrative meaning — running generation jobs, cached embeddings,
provider cost accounting.

### Why this is AI-native

A future AI agent operating on the story should not need a custom
API or a migration tool to read or write it. It just opens files:

- `read_file("characters/lin-mo/character.md")`
- `write_file("chapters/001-the-beginning/scenes/002-laboratory.md", ...)`
- `search_files("characters/**/*.md")`

This is exactly the toolset every coding agent already has. The
story workspace is, structurally, a code project — same shape, same
operations, same affordances.

## Stack

- **Language:** Rust 1.80+
- **UI:** [`gpui-kit`](https://crates.io/crates/gpui-kit) — a single
  facade crate that re-exports GPUI + gpui-base + gpui-component +
  default assets. Pin to `0.6`.
- **Core data model:** pure Rust, no DB. `sagaline-core` walks the
  story directory once and builds an in-memory graph.

## Layout

```text
client/
├── Cargo.toml              workspace root (resolver = "2")
├── README.md               you are here (English)
├── README.zh.md            中文版
├── AGENTS.md               conventions, verification, deferred list
└── crates/
    ├── sagaline-core/      story workspace model + validation
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── story_root.rs    # path validation, story.md loader
    │       ├── path.rs          # layout ↔ entity mapping
    │       ├── entity.rs        # EntityType / ParsedEntity / Reference
    │       ├── frontmatter.rs   # typed YAML accessors
    │       ├── markdown.rs      # front matter / body split
    │       ├── schema.rs        # JSON Schema registry
    │       ├── graph.rs         # StoryGraph::load + validate
    │       └── error.rs
    ├── sagaline-ui/        gpui-kit views over the core graph
    │   ├── Cargo.toml
    │   └── src/lib.rs      # WorkspaceState + WorkspaceView placeholders
    └── sagaline/           the desktop binary
        ├── Cargo.toml
        └── src/
            ├── main.rs     # hello-world banner (next phase: opens window)
            └── lib.rs      # App placeholder
```

## Build & run

```bash
# from this directory (client/)
cargo check --workspace         # typecheck everything
cargo test  -p sagaline-core    # run the 36 core tests
cargo build -p sagaline-core    # build the core crate

# launch the desktop binary (placeholder for now)
cargo run -p sagaline
```

A `sagaline-core` change is done when `cargo test -p sagaline-core`
passes (36 in-module tests covering path classification, markdown
splitting, scene-reference resolution, and graph validation).

## Next steps (deferred)

In priority order — see `AGENTS.md` for the full deferred list:

1. **`crates/sagaline-ui/`** — wire up real views: file tree,
   Markdown editor pane, scene reference preview.
2. **`crates/sagaline/src/main.rs`** — replace the `println!`
   placeholder with a real top-level `App` that opens a window.
3. **`crates/sagaline-providers/`** — `trait ModelAdapter` with
   per-provider implementations (OpenAI, Google Gemini, Kling,
   Runway, ComfyUI, Ollama).
4. **BYOK key store** — encrypted at `~/.sageline/data/keys.db`.
5. **Hosted project sync** — opt-in client against `homeweb/`.

## License

Apache 2.0. See the top-level `LICENSE` (or the
[`AGENTS.md`](../AGENTS.md) "Common constraints" section).