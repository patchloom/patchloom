# Patchloom 0.8.0

This release focuses on library API expansion, MCP usability for AI agents, and multi-language AST support. 4 new features, 3 bug fixes, and 49 new tests across 11 commits.

## Highlights

### Better MCP tool discovery for AI agents

AI models connected via MCP now receive a categorized tool guide in the server instructions, grouping all 54 tools into 7 categories (Document, Markdown, Text, File, AST, Plan, Server). This steers models toward `doc_*` tools for JSON/YAML/TOML mutations instead of falling back to verbose `replace_text` calls. Path parameter descriptions now explicitly state that paths are relative to the working directory, and a new `server_info` tool returns the server's working directory for path discovery. (#1272, #1275)

### Library API for embedders

Two new public APIs let library embedders (like Bline) drop separate dependencies:

- **`text_diff(original, modified, path)`** generates unified diffs in memory, exposing patchloom's internal diff engine as a standalone function.
- **`parse_unified_diff(input)`** exposes patchloom's diff parser, re-exporting `PatchFile`, `Hunk`, and `PatchLine` types through the public API. (#1269, #1272)

### Replace gains `unique` mode and `match_count`

The replace operation now supports `unique: true`, which fails with an error when the search pattern matches more than once. This prevents accidental multi-site replacements that corrupt files. A companion `match_count` field in the result reports how many matches were found, even in dry-run mode. (#1269)

## New features

- **Multi-language `rewrite_function_signature`.** The AST operation now supports all languages with tree-sitter grammars, not just Rust. Rust retains full-reconstruction logic (preserving async/unsafe/const/extern qualifiers); all other languages use surgical node replacement that preserves surrounding code exactly. (#1271)
- **`server_info` MCP tool.** Zero-argument tool that returns the MCP server's working directory, letting agents discover the correct base path for relative file operations. (#1272)
- **`text_diff` and `parse_unified_diff` public APIs.** In-memory diff generation and unified diff parsing for library embedders. (#1269, #1272)
- **`unique` mode and `match_count` for replace.** Fail-safe single-match enforcement and match counting for both CLI and library API. (#1269)

## Bug fixes

- **Permission-based tests no longer fail as root.** Tests that set file permissions to `000` now detect when running as root (common in Docker containers) and skip gracefully instead of failing. (#1277)
- **Clarified misleading MCP code comments.** Fixed a doc comment that incorrectly described validation behavior, renamed a misleading variable (`log_clone` to `log_path`), and simplified an error conversion that discarded type information. (#1278)
- **MCP no-match error signaling.** Operations that find zero matches now return proper error responses instead of silent success, making failures visible to connected agents. (#1272)

## Test quality

- Replaced bare `assert!(x.is_ok())` calls with `.expect()` for actionable panic messages on failure. (#1280)
- Strengthened weak assertions that matched substrings too broadly (e.g., `contains("a")` passing on error messages). (#1279)
- Updated `clap_complete` dependency for shell completion generation. (#1279)
- Added missing MCP integration tests and corrected stale test counts in documentation. (#1262)

## Numbers

| Metric | v0.7.0 | v0.8.0 | Delta |
|--------|--------|--------|-------|
| Unit tests | 1,938 | 1,983 | +45 |
| Integration tests | 834 | 838 | +4 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **2,782** | **2,831** | **+49** |
| CLI commands | 22 | 22 | -- |
| MCP tools | 53 | 54 | +1 |
| Commits | -- | 11 | -- |
