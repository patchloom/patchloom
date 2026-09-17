//! MCP `list_files` re-exports the shared inventory walker (#2076 / #2541).
//!
//! Collector lives in [`crate::cmd::list_files`] so CLI compiles without `mcp`.

pub(crate) use crate::cmd::list_files::collect_list_files;
