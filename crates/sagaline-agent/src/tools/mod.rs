//! Built-in tools. Add new tools by implementing [`Tool`] and exposing
//! the type here.

pub mod read_file;

pub use read_file::{ReadFileArgs, ReadFileTool, ReadFileResult};
