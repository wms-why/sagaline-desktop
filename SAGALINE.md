# Sagaline — What We Are

Sagaline is a **GUI-equipped AI agent specialized in structured
AI video generation**. It is not a file editor. It is not a
prompt playground. It is not a workflow automation tool that the
user operates.

It is an **agent** that:

- **Observes** a story workspace (Markdown + YAML in a directory).
- **Plans** a video production by decomposing scenes into shots.
- **Acts** by calling model providers (image / video / audio).
- **Reflects** via `StoryGraph::validate()` + LLM self-critique.
- **Reports** its chain-of-thought to the GUI in real time.

The user sets the high-level goal. The agent does the rest, with
the user able to interrupt, correct, or steer at any point.

---

## How we map to AI agent concepts

| Concept            | Sagaline's analog                                         |
| ------------------ | --------------------------------------------------------- |
| Goal               | User's high-level request in the GUI                      |
| Memory (long-term) | `story/` directory — Markdown + YAML frontmatter          |
| Working memory     | The current scene/shot the agent is processing            |
| Tools              | `sagaline-agent::tools::*` — typed, JSON-Schema-described |
| Plan               | `chapter → scene → shot` hierarchy in the story tree      |
| Observation        | `StoryGraph::load` + loaded entity bodies as context      |
| Action             | `ModelAdapter` calls (image / video / audio generation)   |
| Reflection         | `StoryGraph::validate()` + LLM self-critique prompt       |
| Human-in-the-loop  | GUI panels: approve / edit / retry the agent's actions    |
| Chain-of-thought   | The right-side panel of the desktop window, live          |

---

## The GUI is the agent's transparency surface

The right panel of the desktop window is the agent's
**chain-of-thought**, not a log file, not a debug console.
It is the *thinking* of the agent, live, while it works:

```
🤖 Processing scene: 001-intro

[10:00:01] OBSERVE
  • Loaded scene.frontmatter
  • Resolved: lin-mo (character), laboratory (environment)
  • Loaded 2 shots already done

[10:00:03] PLAN
  → shot_004: 林默特写, 表情紧张, 3s
  → shot_005: 实验室全景, 能量核心发光, 2s

[10:00:05] ACT  shot_004
  🔧 tool: generate_image
    prompt: "close-up of Lin Mo, anxious expression, ..."
    model:  gemini-2.5-flash-image
    → assets/.../shot_004/keyframe.png

[10:00:08] REFLECT
  ✓ StoryGraph::validate() — 0 errors
  🤔 self-critique: "shot_004 林默表情不够紧张..."

[10:00:09] → next shot
```

The user can pause, edit a frontmatter field, retry a tool call,
or rewrite the plan — all from the same surface.

---

## Why Markdown + YAML as the agent's memory

Because the agent's memory and plan must be:

1. **Inspectable by humans** — audit, edit, version the agent's
   state with the same affordances as code.
2. **Persistable without a database** — no vendor lock-in, no
   migration path. `cp -a` is a backup.
3. **Composable with standard tooling** — `git diff`, `grep`,
   `find`, any text editor. The same toolset every coding agent
   already has.
4. **Tool-callable by the agent itself** — `read_file`,
   `write_file`, `glob`, `grep` are exactly the tools the agent
   uses to manipulate its own memory.

This is the same design rationale as MemGPT / AWM / AutoGPT
memory layers, but specialized for narrative video: the
frontmatter is a typed, schema-validated state record; the body
is free-form reasoning.

---

## What we are NOT

- ❌ A Markdown editor — use any text editor; the GUI's editor
  pane exists only to let the human steer the agent, not to
  replace VS Code.
- ❌ A static file validator — `StoryGraph::validate()` exists to
  serve the agent's reflection loop, not to be run by hand.
- ❌ A no-code workflow tool — the agent is the executor; the
  user does not wire nodes together.
- ❌ A prompt management UI — prompts are agent-internal. The
  user sets goals and reviews output, not prompts.

---

## The agent loop

For every scene the agent processes, the same four steps:

```text
OBSERVE   — load the scene + all referenced entities
PLAN      — decompose into N shots with duration / camera / mood
ACT       — for each shot: generate keyframe, animate, voice, write assets
REFLECT   — validate graph + LLM self-critique; loop or escalate
```

The `sagaline-agent` crate owns this loop. The `sagaline-ui`
crate subscribes to its event stream and renders the
chain-of-thought. The `sagaline-providers` crate supplies the
tool backends. The `sagaline-core` crate is the agent's memory
layer.

---

## Crate map (current)

| Crate                | Role in the agent                                  |
| -------------------- | -------------------------------------------------- |
| `sagaline-core`      | Agent's memory + reflection (`StoryGraph`)         |
| `sagaline-agent`     | Loop, tool registry, prompt assembly (this work)   |
| `sagaline-ui`        | GUI: subscribes to agent events, renders the panel |
| `sagaline-providers` | Model adapters (text / image / video / audio)      |
| `sagaline`           | Binary entry: boots, owns the top-level `App`      |

---

## License

Apache 2.0. See the top-level `LICENSE`.
