//! Sagaline agent — the OBSERVE → PLAN → ACT → REFLECT loop.
//!
//! See `SAGALINE.md` at the repo root for the product framing. This crate
//! is the agent: it owns the loop, the tool registry, and an event stream
//! the UI subscribes to for live chain-of-thought.
//!
//! In this scaffold the agent runs end-to-end against mock / canned
//! responses — there is no LLM client and no real model adapter yet.
//! That is intentional: the loop is testable without keys, and a later
//! phase swaps the canned PLAN / REFLECT for real LLM calls and the mock
//! `generate_image` / `generate_video` tools for real ones.

pub mod event;
pub mod loop_;
pub mod prompt;
pub mod tool;
pub mod tools;

pub use event::{AgentEvent, EventSink};
pub use loop_::{Agent, AgentConfig, StepOutcome};
pub use tool::{Tool, ToolDescriptor, ToolError, ToolRegistry, ToolResult};
