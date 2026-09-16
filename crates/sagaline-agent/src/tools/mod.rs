//! Built-in tools. Add new tools by implementing [`Tool`] and exposing
//! the type here.

pub mod find;
pub mod generate_image;
pub mod list_dir;
pub mod read_file;
pub mod validate_story;
pub mod write_file;

pub use find::{FindArgs, FindTool};
pub use generate_image::{GenerateImageArgs, GenerateImageTool};
pub use list_dir::{ListDirArgs, ListDirTool};
pub use read_file::{ReadFileArgs, ReadFileTool, ReadFileResult};
pub use validate_story::{ValidateStoryArgs, ValidateStoryTool};
pub use write_file::{WriteFileArgs, WriteFileTool};