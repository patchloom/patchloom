# Patchloom 0.7.0

The 0.7.0 release is the most thoroughly tested version of patchloom to date. 98 bug fixes from 480+ rounds of runtime testing, 15 refactoring PRs that unified the execution engine, and 9 new features. 2,782 tests (up from 1,816 in 0.6.0), 140 commits, 50,946 lines added.

## Highlights

### API field names aligned with LLM priors

**Breaking change.** CLI arguments and plan field names were renamed to match what LLMs naturally generate. Agents that previously struggled with parameter names now get them right on the first try.

| Before | After | Why |
|--------|-------|-----|
| `--from` / `--to` | positional `OLD` + `--new` | "replace X with Y" maps to `replace X --new Y` |
| `--file` | positional `FILE` | Natural first argument |
| `hygiene` | `tidy` | Clearer intent |
| `--plan` | positional `PLAN` | Direct argument |

Serde aliases preserve backward compatibility for plan files: both `"from"` and `"old"` are accepted in JSON/YAML plans.

### 480+ rounds of runtime testing

The `/fixrealloop` methodology was applied to patchloom itself for the first time. Every command was exercised with real files, edge-case inputs, and adversarial patterns. Combined with 12 rounds of systematic code audit and 8 multi-perspective improvement cycles, this surfaced 98 bug fixes across all major modules.

Areas of highest bug density:
- **YAML operations:** quote style preservation, comment migration after key deletion, multi-document detection, CST trailing whitespace cleanup
- **AST operations:** C/C++ pointer-returning function extraction, Ruby symbol extraction, shell/bash word node handling, template interpolation traversal
- **Transaction engine:** strict rollback restoring collateral files, validation step label reporting, context-filtered replace accuracy
- **Regex handling:** consistent `multi_line(true)` across all regex builders (search, replace, tx engine, library API, AST replace), phantom EOF match filtering

### Execution engine unification

All write commands now route through the transaction engine. Previously, some commands used `atomic_write()` directly, bypassing backup, rollback, and format/validate lifecycle steps. The unification delivered 110 new tests and eliminated a class of bugs where new commands forgot a write mode.

Three execution paths serve different needs:

| Path | Use case | Example |
|------|----------|---------|
| `execute_via_engine()` | Single-operation writes | doc, md, create, delete, append |
| `execute_operations()` | Multi-file writes with pre-filtering | ast rename |
| `execute_precomputed()` | Parallel scan + batch commit | replace (multi-file regex) |

### Post-write formatter hook

New `--format` flag runs an external command after successful writes. Supports per-extension configuration via `.patchloom.toml`:

```toml
[format]
rs = "rustfmt {file}"
py = "black {file}"
go = "gofmt -w {file}"
```

The formatter integrates with `--confirm` interactive mode and transaction plans (via `format` steps). Format commands run after writes but before validation steps.

### Symbol verification for transaction plans

Transaction plans can now include `verify` checks that run before and/or after operations:

```json
{
  "verify": [
    {"symbol": "MyClass", "file": "src/lib.rs", "when": "before"},
    {"symbol": "MyClass::new", "file": "src/lib.rs", "when": "after"}
  ]
}
```

Pre-checks confirm symbols exist before modification. Post-checks confirm the result compiles or the expected symbols appear. Failed checks trigger rollback in strict mode.

## New features

- **Windows ARM64 and Linux musl release targets** (#947): Pre-built binaries now cover `aarch64-pc-windows-msvc` and `x86_64-unknown-linux-musl`.
- **`tidy --dedent` and `--indent`** (#1034): Adjust indentation levels across files. `--dedent 4` removes 4 spaces of leading indentation; `--indent 2` adds 2 spaces.
- **AST Phase C operations** (#1039, #1040): `ast.insert`, `ast.wrap`, `ast.imports`, `ast.group`, `ast.reorder`, `ast.move`, `ast.extract`, `ast.split`, and `for_each` glob batching.
- **Library API parity** (#1047): `FilePrepend`, `replace_in_content()`, and `SearchOptions` with `WalkBuilder` support for exclude patterns and custom ignore files.
- **Template interpolation traversal** (#1090): `ast rename` and `ast refs` now follow identifiers inside template literals (`${varName}` in JS/TS, `f"{var}"` in Python).
- **MCP `md_dedupe_headings` tool** (#1097): Deduplicate repeated markdown headings in a single MCP call.

## Refactoring

- Consolidated shared logic into ops layer, separating business logic from CLI wiring (#970).
- Extracted inline tests to companion files and organized by concern category (#1010, #1023).
- Split `cmd/mcp/mod.rs` into focused submodules (#1024).
- Routed all API write functions (AST, doc, file, replace, tidy, patch) through the tx engine (#1027-#1032).
- Reduced `DocAction` and batch parsing boilerplate (#1026).
- Extracted `InsertContext` struct to eliminate `clippy::too_many_arguments` (#1123).
- Migrated test helpers to `TxStateFixture` for consistent setup (#1124).

## Bug fixes (selected)

98 bug-fix commits span every major module. Selected highlights:

- **Regex anchors:** `^` and `$` now behave consistently across search, replace, tx engine, library API, and AST replace. All regex builders use `multi_line(true)`. Phantom EOF matches at `content.len()` are filtered (#1254).
- **YAML fidelity:** doc delete preserves quote styles and key order (#1239). Orphaned comments that migrate inline after key deletion are stripped (#1238). Trailing whitespace from YAML CST after key deletion is cleaned (#1237).
- **Multi-document YAML:** improved detection and multi-line context matching (#1248, #1252).
- **Context-filtered replace:** before/after context lines now compare multiple lines instead of single-line matching (#1248).
- **Nested predicates:** doc select/delete-where predicates support nested paths (#1248).
- **Patch apply:** supports file creation and deletion within unified diffs (#1235).
- **Create with empty content:** `create` now actually creates the file instead of silently doing nothing (#1233).
- **Symlink handling:** `atomic_write` resolves symlinks before writing (#1232).
- **Undo advancement:** sequential undo now advances through backup sessions instead of replaying the same one (#1201).
- **EditorConfig:** `tidy check --respect-editorconfig` detects EOL mismatches and honors `trim_trailing_whitespace` (#1199, #1203).
- **AST:** C/C++ pointer-returning functions extracted correctly (#1242). Ruby gets a dedicated symbol extractor (#1241). Shell/bash `word` node kind included in AST rename (#1243).
- **Strict rollback:** restores collateral files modified by format steps (#1116).

## Test quality

- Removed 14 duplicate test functions accumulated during automated bug-hunting sessions (#1255).
- Replaced 2 dead defensive guards (unreachable through any API path) with `debug_assert!()` (#1255).
- PTY test expect timeout increased from 10s to 30s to eliminate flakiness under load (#1256).
- Replaced 19 bare `assert!(x.is_ok())` with `.unwrap()` for proper panic messages (#955).
- Strengthened weak `contains("a")` assertions that passed by matching substrings in error messages (#1125).

## Dependency upgrades

- `rmcp` 1.8.0 to 2.0.0 (#1221)
- `axum-server` 0.7 to 0.8 (#953)
- `expectrl` 0.7 to 0.9 (#953)
- GitHub Actions group update (#971)

## Numbers

| Metric | v0.6.0 | v0.7.0 | Delta |
|--------|--------|--------|-------|
| Unit tests | 1,056 | 1,938 | +882 |
| Integration tests | 750 | 834 | +84 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **1,816** | **2,782** | **+966** |
| CLI commands | 22 | 22 | -- |
| MCP tools | 43 | 43 | -- |
| Bug fixes | -- | 98 | -- |
| Commits | -- | 140 | -- |

## Upgrading

The API field name changes (#1214) are the only breaking change. If you use patchloom as a library or construct plan files programmatically:

- CLI: `--from`/`--to` are now positional `OLD` + `--new NEW`
- Plans: serde aliases accept both old and new field names (`"from"` and `"old"` both work)
- MCP: tool parameter names follow the new convention

No action needed if you only use the MCP server or transaction plans with the alias support.
