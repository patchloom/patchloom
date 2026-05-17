# Changelog

All notable changes to Patchloom are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- 12 commands: `search`, `replace`, `patch`, `md`, `doc`, `hygiene`, `create`, `delete`, `read`, `status`, `tx`, `completions`
- 24 transaction plan operation types for atomic multi-file changes
- `format` and `validate` lifecycle arrays in tx plans with configurable timeout
- `--nth N` flag for replace (standalone and tx) to target a specific occurrence
- `--case-insensitive` / `-i` for search and replace
- `--glob` flag is repeatable for multi-pattern filtering via GlobSet
- `md insert-before-heading` subcommand and tx operation
- `delete` standalone command with `--apply` / `--check` modes
- `file.create` tx operation with `force: true` option to overwrite
- `doc.prepend`, `doc.update`, `doc.move`, `doc.ensure`, `doc.delete_where` tx operations
- `md.table_append`, `md.dedupe_headings`, `md.insert_before_heading` tx operations
- `patch.apply` and `file.delete` tx operations
- Depth guard (128 levels) on `deep_merge` to prevent stack overflow
- File path context in `with_doc` error messages
- Dual license: MIT OR Apache-2.0
- CONTRIBUTING.md, SECURITY.md, AGENTS.md
- CI with fmt, clippy, tests, MSRV check, and dependency audit
- `read` command for file content inspection with optional line range and multi-file batch support
- `status` command showing uncommitted changes vs git HEAD
- `replace --insert-before` and `--insert-after` modes for inserting text around matches
- `replace --if-exists` flag for idempotent replacements that succeed on no match
- `search --assert-count N` mode for CI invariant checks
- YAML and TOML plan format support for `tx` (auto-detected from file extension, or `--plan-format`)
- `--plan -` reads tx plan from stdin
- tx replace `case_insensitive` and `multiline` fields for parity with standalone replace
- tx replace `if_exists` field for idempotent replacements inside transactions
- `delete --json` structured output for consistency with other write commands
- `agent-rules` command that prints an end-user AGENTS.md teaching AI agents how to use patchloom
- `search --before-context` (`-B`) and `--after-context` (`-A`) for asymmetric context around matches
- `read` operation in `tx` plans for inspect-then-edit workflows in a single call
- `search` operation in `tx` plans for locate-then-edit workflows in a single call
- Stderr diagnostics for silently skipped files in search, replace, and tx glob replace
- Documentation for tx operation ordering semantics
- Documentation for `write_policy` in tx plans (applies to all operations including `file.create`)
- `strict` mode for tx plans: reverts all writes on format/validate failure (exit code 7)
- Thread-based timeout for format/validate steps (replaces polling loop)
- JSON output mode for `tx` command via `--json` flag
- JSON error output on all tx failure paths, with explicit `error_kind` values for parse_error, rollback, validation_failed, and format_failed while preserving backward-compatible legacy `error` prefixes
- 421 tests (166 unit + 255 integration)

### Fixed

- `file.create` after `file.delete` in the same tx plan no longer silently loses the file
- Makefile `update-readme` dynamically reads version, command count, and test counts instead of hardcoding
- Empty `--from` string in replace and tx replace is now rejected instead of silently inserting between every character
- tx replace with conflicting fields (`to` + `insert_before`, or `insert_before` + `insert_after`) now returns PARSE_ERROR instead of undefined behavior
- tx replace plans missing all of `to`, `insert_before`, and `insert_after` now return PARSE_ERROR instead of silently deleting matches
- Replace-only tx plans with zero matches now return NO_MATCHES (exit 3) instead of SUCCESS
- tx glob replace no longer buffers non-matching files into pending state
