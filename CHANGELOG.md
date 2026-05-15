# Changelog

All notable changes to Patchloom are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- 10 commands: `search`, `replace`, `patch`, `md`, `doc`, `hygiene`, `create`, `delete`, `tx`, `completions`
- 22 transaction plan operation types for atomic multi-file changes
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
- `--plan -` reads tx plan from stdin
- Documentation for tx operation ordering semantics
- Documentation for `write_policy` in tx plans (applies to all operations including `file.create`)
- 305 tests (152 unit + 153 integration)

### Fixed

- `file.create` after `file.delete` in the same tx plan no longer silently loses the file
- Makefile `update-readme` dynamically reads version and command count instead of hardcoding
