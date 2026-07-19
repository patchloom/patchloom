# AGENTS.md

## Project overview

Patchloom is a Rust CLI for agent-grade repo operations. It provides twenty-three commands (`search`, `replace`, `patch`, `md`, `doc`, `tidy`, `append`, `prepend`, `create`, `delete`, `rename`, `read`, `status`, `tx`, `batch`, `explain`, `undo`, `init`, `completions`, `agent-rules`, `schema`, `ast`, `mcp-server`) that let AI coding agents perform structured file searches, mechanical replacements, diff-based patching, markdown section editing, JSON/YAML/TOML document manipulation, whitespace normalization, file appending, file prepending, file creation, file deletion, file renaming, multi-operation atomic transactions, line-oriented batch operations, human-readable plan summaries, undo safety net with backup restoration, project setup, shell completion generation, end-user agent rules generation, operation schema export with tier filtering, AST-aware code operations (list, read, rename, validate via tree-sitter), and MCP protocol server for structured tool calls. All write operations are dry-run by default and support `--check` (report changes), `--diff` (preview), and `--apply` (mutate) modes.

The `cli` feature (clap + command implementations) is enabled by default. Use `default-features = false` for pure library use (no clap). The `mcp` and `ast` features are also enabled by default. Build with `--no-default-features --features ast` (or similar) for a smaller library.

## Dev commands

| Command | What it does |
|---------|-------------|
| `make fmt` | Run `cargo fmt --all` |
| `make fmt-check` | Check formatting without modifying files |
| `make build` | Run `cargo build --all-features` |
| `make test` | Run unit tests (`cargo test --lib --all-features`) |
| `make integration-test` | Run integration tests (`cargo test --test integration --all-features`) |
| `make pty-test` | Run PTY-based interactive terminal tests (`cargo test --test pty --all-features -- --test-threads=1`) |
| `make test-library-hygiene` | Enforce pure-library embedder set: clippy + tests under `--no-default-features --features "ast,files"` (catches dead_code, hygiene for #800 #802) |
| `make test-mcp-no-ast` | Lib tests with `mcp,cli,files` and no `ast` (MCP inventory/router/instructions honesty) |
| `make clippy` | Run `cargo clippy --all-targets --all-features -- -D warnings` |
| `make check` | Run fmt-check, clippy, test, test-no-default, test-ast-only, test-mcp-no-ast, test-library-hygiene, integration-test, pty-test, verify-release-notes, audit-test-hygiene, check-patchloom-md, check-readme |
| `make check-fast` | Fast check: same as `check` minus `check-patchloom-md` (still runs `check-readme` so test-count drift fails locally before CI) |
| `make update-readme` | Update README.md rounded test count (only changes when hundreds digit changes) |
| `make check-readme` | Verify README.md rounded test count is accurate (part of `check`) |
| `make sync-patchloom-md` | Regenerate PATCHLOOM.md from `patchloom agent-rules` output |
| `make check-patchloom-md` | Verify PATCHLOOM.md matches `patchloom agent-rules` output (part of `check`) |
| `make audit-test-hygiene` | Audit test names and weak assertions for staleness after refactors (run after MPI or breaking changes) |
| `make audit` | Run `cargo audit` for known vulnerabilities (optional locally; also in CI) |
| `make deny` | Run `cargo deny check` for licenses/bans/sources (`deny.toml`; required in CI) |
| `make pack-mcpb` | Pack `mcpb/` into `target/mcpb/patchloom-<ver>.mcpb` (Smithery / desktop). Honors `$VERSION` over Cargo.toml |
| `make pack-mcpb-test` | Unit tests for pack version override and stamped manifest (`scripts/test_pack_mcpb.py`; skips full pack when `mcpb` CLI missing) |
| `make agent-test` | Run agent integration tests (requires LLM API key, not part of `check`). Use `MODEL=X` to switch LLM (e.g. `make agent-test MODEL=sxs-gpt-5-4`) |
| `make embedder-smoke` | Pre-release host contracts (fuzzy token span, nested undo list, plan `key` alias). Not part of `check`; run before tagging a release |
| `make fuzz` | Run fuzz tests (11 targets: selector parse, patch parse, patch apply, batch tokenize, selector eval, doc parse, containment_check, fallback_resolve, ast_parse, md_heading, replace_regex). Requires nightly, not part of `check`. Use `FUZZ_TIME=N` for seconds per target |
| `make bench-cli` | Run CLI benchmarks vs native tools (requires `hyperfine`, not part of `check`) |
| `make bench-mcp` | Run MCP benchmarks: per-call latency vs CLI process spawn (not part of `check`) |
| `make bench-agent` | Run LLM agent A/B benchmarks (requires API key, not part of `check`). Use `MODEL=X RUNS=N` to configure runs |
| `make bench-agent-dry-run` | Preview agent benchmark prompts without calling the LLM API |
| `make bench-agent-report` | Generate comparison report from saved agent benchmark results. Use `FILE=path` for specific file |
| `make git-clean` | Remove known temp files that pollute `git status` (e.g. `.lycheecache` from lychee) |
| `make clean` | Remove build artifacts and temp files |

Always run `make check` before committing. It is the full CI gate.

Before tagging a release, also run `make embedder-smoke` (host-facing contracts that unit tests historically missed).

## Git hygiene

Keep the working tree clean:

- `git status --short` should be empty (except when intentionally on the release-please synthetic branch).
- Run `make git-clean` to remove temp files such as `.lycheecache` (created when `cache = true` in `lychee.toml`).
- At the end of any session or before switching tasks: `git fetch --all --prune`, `make git-clean`, `git status --short`, and ensure you are on a clean `origin/main` (or the allowed release-please branch).
- Common sources of "dirty" state: lychee cache, local edits during rebase/force work on release-please, Cargo.lock drift from different tool versions. Fix them explicitly rather than carrying them.
- After a core PR merges mid-session (e.g. #753 while polish for #754 was in flight): the feature branch tip is no longer ancestor of main ("has merged PR" from pre-commit hook). Recovery: `gh pr view N --json state` (confirm merged), `git checkout -b fix/review-continue-YYYYMMDD origin/main`, cherry-pick the useful commits (or `git show <oldsha> | patch -p1`), `git add <explicit files only>`, commit -s, push, create PR. See patchloom-contrib for full "Follow-up polish after base PR".
  For review/polish sessions you can temporarily set `REVIEW_CONTINUE=1` (or `ALLOW_MERGED_COMMIT=1`) to skip the hook block (see the global pre-commit hook for details). Always unset after the session.

- **PR bodies must contain explicit issue links for traceability (addresses #819).** Every PR that resolves GitHub issues (including library follow-ups after a base PR has merged, embedder/API polish, etc.) MUST list `Closes #N` or `Fixes #N` (one per line) in the PR *body/description*. GitHub only auto-closes from the PR body under squash-merge (individual commit messages are dropped). Use `Ref #N` for related but non-closing references. Never rely on commit message only. See `~/.grok/skills/owned-repo-gate/SKILL.md` (Phase 4) and `~/.grok/skills/github-interaction/SKILL.md` for the full rule and recovery. For follow-up PRs, edit the body with `gh pr edit` if the initial description was minimal. Verify with issue audit before claiming closure.

See also the branch hygiene rules in `~/.grok/skills/patchloom-contrib/SKILL.md`.

## Release PRs (release-please)

- The open release-please PR (#724 etc.) title must be correct. Use `gh pr edit --title` when it shows the wrong version.
- The PR *body* can be very long and may temporarily show the wrong next version header (release-please behavior). This is tracked as tech-debt #740.
- When updating library embedding examples (in lib.rs, README, docs/), keep the version string in sync with the current Cargo.toml / .release-please-manifest.json (avoids the 0.4 vs 0.5 drift reported in #816 follow-up).
- **Library follow-up PRs and high-level API changes must use explicit Closes links in the PR body** (see #819 and the new rule in Git hygiene above). The #811-#815 library embedder work + #817/#818 follow-ups exposed the gap where minimal PR bodies left issues open after squash-merge. Always include them for traceability.
- Primary curation is done via `RELEASE_NOTES.md` (applied to the final GitHub Release by the host job, not the PR body).
- See `patchloom-contrib` skill ("Curated release notes" and "Major version bumps" sections) for the full process.

## Project structure

```
src/
  main.rs             Thin entrypoint; calls patchloom::run(), maps Result to ExitCode
  lib.rs              Parses CLI with clap, delegates to cmd::dispatch; re-exports modules
  files.rs             File-walking utilities: is_binary, collect_file_paths, build_glob_matcher,
                       matches_glob. Used by search, replace, tidy, and status commands.
  api/mod.rs           Public library API for embedding patchloom in Rust applications.
                       CLI-independent interface: replace_text, search_file, doc_set, md_replace_section,
                       execute_plan, parse_unified_diff, etc. All write ops accept ApplyMode
                       (Preview/Apply/Check) and optional PathGuard for containment.
  cli/mod.rs           Defines Cli struct (clap Parser) with GlobalFlags and Command subcommand
  cli/global.rs        GlobalFlags (read-only: --json, --jsonl, --quiet, --cwd, --glob,
                       --files-from) and WriteFlags (--diff, --apply, --check,
                       --ensure-final-newline, --normalize-eol, --trim-trailing-whitespace,
                       --respect-editorconfig, --confirm). Write flags are only available on write commands.
  cmd/mod.rs           Command enum (clap Subcommand), dispatch(), built-in agent-rules
                       generator, and inline Completions command
  cmd/append.rs        Append content to an existing file
  cmd/batch.rs         Line-oriented batch operations, parses positional args, delegates to tx engine
  cmd/mcp/             MCP server (feature-gated): registry (1:1 Operation tools) + hand-written
                       handlers. Surface policy and custom-tool inventory: mcp/surface.rs
                       (registry default; custom must be justified). Dynamic ToolRoute registration.
  cmd/search.rs        Literal/regex search across files with context, count, files-with-matches, -i
  cmd/replace.rs       Literal/regex string replacement with diff preview, --nth, -i, atomic write
  cmd/delete.rs        Delete a file (with --apply/--check modes)
  cmd/rename.rs        Rename (move) a file (with --apply/--check modes, --force for overwrite)
  cmd/patch.rs         Preview or apply unified diffs
  cmd/md.rs            Markdown section-aware operations (replace section, insert before/after heading,
                       upsert bullet, table append, dedupe headings, lint)
  cmd/ast/             AST-aware operations (list, read, rename, validate, …) using tree-sitter
                         mod.rs dispatch; common helpers; query (read-only); mutate (rename/replace)
  cmd/doc.rs           Parser-backed JSON, YAML, TOML operations (get, has, keys, len, set,
                       delete, merge, append, prepend, update, move, ensure, delete-where,
                       select, flatten, diff)
  cmd/tidy/            Final newline, line ending, and trailing whitespace normalization
                         (mod dispatch; check scan/report; fix scan+stage+finalize)
  cmd/create.rs        Create a new file with content
  cmd/read.rs          Read file contents with optional line range
  cmd/schema.rs        Export operation schemas with tier filtering and system prompt generation
  cmd/status.rs        Show uncommitted file changes vs git HEAD
  cmd/tx.rs            Transaction engine: execute a multi-operation plan atomically
  cmd/explain/         Parse a tx plan and print a human-readable summary
                         (mod + describe; JSON includes schema catalog blurb per op)
  cmd/undo.rs          Restore files from backup sessions created by --apply
  cmd/init.rs          Project setup: shell completion install, AGENTS.md generation
  config.rs            Project config file (.patchloom.toml) loading and merging
  backup.rs            Backup session management for undo safety net
  schema.rs            Intent format spec: OperationSchema, Tier, OPERATION_REGISTRY (metadata table),
                       operation_variant_schema() (extract single variant JSON Schema from Operation),
                       operation_schemas(), operations_for_tier(), system_prompt_for_tier(),
                       INTENT_FORMAT_VERSION
  fallback.rs          Multi-strategy fallback chain: EditError, EditErrorKind, validate_edit(),
                       find_similar_targets(), anchor_match(), resolve_with_fallback()
  selector/mod.rs      Re-exports selector parser and evaluator
  selector/parser.rs   Path selector parser (key, index, wildcard, predicate segments)
  selector/eval.rs     Evaluate parsed selectors against serde_json::Value trees
  exit.rs              Exit code constants: SUCCESS=0, FAILURE=1, CHANGES_DETECTED=2,
                       NO_MATCHES=3, PARSE_ERROR=4, AMBIGUOUS=5, VALIDATION_FAILED=6, ROLLBACK=7,
                       CONFLICTS=8, OPERATION_FAILED=9
  diff.rs              Unified diff generation using similar::TextDiff; FileDiff and DiffResult types
  ops.rs               Shared operation helpers used by cmd/tx.rs, cmd/doc.rs, cmd/md.rs:
                       replace (validation, replacement text, content replacement),
                       doc (format detection, parsing, navigation, merge, update),
                       md (heading parse, section replace, bullet upsert, table append),
                       patch (parse, apply hunks with fuzz, loader). Each is a pub(crate) submodule.
  write.rs             Atomic file writes via tempfile; WritePolicy applies trim, EOL, final newline
  plan/                Transaction plan format (`mod.rs`, `operation.rs`, `tests.rs`): Plan, Operation, …
                       25 operation types including all doc/md/replace/tidy/file/patch/read/search ops.
                       VerifyCheck defines pre/post symbol verification specs (must live here, not in
                       feature-gated modules, because Plan is always compiled)
tests/
  integration.rs       Rust integration tests (cargo test --test integration)
  agent/               Python (pytest) agent integration tests verifying AI agents use patchloom
    conftest.py        Fixtures: workspace with AGENTS.md, patchloom shim for invocation capture
    drivers/           Pluggable agent drivers (GrokDriver first, extensible)
    test_basic.py      Search, replace, read scenarios
    test_batch.py      Batch replace, tx multi-file, tidy scenarios
    test_files.py      Create, delete, status, patch scenarios
    test_structured.py Doc set, md table-append scenarios
    shim.sh            Patchloom invocation-capture shim template
PATCHLOOM.md           Generated CLI usage guide for AI agents (from patchloom agent-rules)
```

## Architecture conventions

### Entrypoint

`main.rs` is thin. It calls `patchloom::run()` and converts the `Result<u8>` into `ExitCode`. All logic lives in `lib.rs` and below.

### Command structure

Each command lives in `src/cmd/<name>.rs` and exports:

```rust
pub struct <Name>Args { /* clap Args */ }

pub fn run(args: <Name>Args, global: &GlobalFlags) -> anyhow::Result<u8> {
    // command logic
    // return exit code from exit.rs constants
}
```

The `Command` enum in `src/cmd/mod.rs` has one variant per command. The `dispatch()` function matches on the enum and forwards to the corresponding `run()`.

### Global flags

All subcommands receive a `&GlobalFlags` reference. Read-only flags (`--json`, `--jsonl`, `--quiet`, `--cwd`, `--glob` (repeatable), `--files-from`) are global. Write-only flags (`--apply`, `--check`, `--diff`, `--confirm`, `--ensure-final-newline`, `--normalize-eol`, `--trim-trailing-whitespace`, `--respect-editorconfig`) are defined in `WriteFlags` and flattened only into write commands. The dispatcher merges them via `GlobalFlags::merge_write()`.

### Error handling

- Use `anyhow::Result` for propagating errors.
- Return exit codes directly using constants from `src/exit.rs` (e.g. `exit::NO_MATCHES`, `exit::CHANGES_DETECTED`).
- Return `Ok(exit::SUCCESS)` for success, `Ok(exit::NO_MATCHES)` for no-match, etc.
- **Typed errors for agent JSON.** Prefer `exit::InvalidInputError`, `NoMatchError`, `ParseErrorError`, `FormatFailedError`, etc. over bare `bail!` for conditions that agents branch on. Global `--json` dispatch and command remappers classify via `exit::classify_typed_error` (single table of kind + exit code). Library hosts use `api::edit_error_kind(&err)`, which peels the same typed errors.
- When returning a soft no-match or typed failure with a message, use `global.emit_error_json_kind(Some("no_matches"), &msg)?` (or the matching kind). Prefer `emit_error_json_kind` over bare `emit_error_json` so `error_kind` is always set for agent-facing failures.
- **Never use `global.show_status()` on error diagnostic paths.** `show_status()` requires a TTY (returns `false` when stderr is piped), which silently suppresses error messages in scripts and pipelines. Use `!global.quiet` instead. Reserve `show_status()` for optional progress hints and status messages that are genuinely TTY-only (e.g., "hint: use --apply", file count summaries). See #1340 and #1341 for bugs caused by this confusion.
- **Preview mode must return `CHANGES_DETECTED` (exit 2), not `SUCCESS` (exit 0).** When a write command runs in default mode (no `--apply`, `--check`, or `--diff`) and the operation would produce changes, the exit code must be `exit::CHANGES_DETECTED`. Returning `exit::SUCCESS` in preview mode is a bug: it makes scripts think no changes exist. **Write singularity:** stage with `tx::engine::stage(WriteRequest)` (or CLI `cmd::output::run_write` / `stage_for_write`), then finalize with `cmd::write_mode::finalize_execution_result` (or `finalize_callback_write` for binary/case-only renames). Mode and exit codes are owned only by `write_mode.rs`. Do not invent a parallel check/apply/preview matrix in a command. Contract lock: `tests/integration/write_path_contract_tests.rs` and `docs/plans/write-pipeline.md`. Historical multi-path bugs: #1345-#1348; singularity: #1373.
- **Post-write `--format` failures** use `FormatFailedError` (`error_kind: format_failed`, exit 1). Files may already be written; do not drop this kind in command-specific remappers (use `classify_typed_error`).

### Testing

- Tests go in `#[cfg(test)] mod tests` blocks at the bottom of each file.
- Use `tempfile::TempDir` for test fixtures that need a filesystem.
- Use `GlobalFlags::default()` for test helpers. Override specific fields with struct update syntax: `GlobalFlags { apply: true, ..GlobalFlags::default() }`.
- Test both the internal functions and the public `run()` function to verify exit codes.
- When embedding file paths in YAML or TOML plan strings in integration tests, use `portable_path_str(&path)` (defined in `tests/integration.rs`) to convert backslashes to forward slashes. Windows paths like `C:\Users` contain `\U` which YAML and TOML parsers interpret as a unicode escape sequence.
- For non-existent file paths in tests, use `nonexistent_path("name")` which returns a platform-appropriate path.
- `cargo test --lib` runs tests in parallel (CI too). For test-only failure-injection hooks, use `thread_local!` plus an RAII guard (e.g. `RestoreFailGuard`, defined in `src/tx.rs` and re-exported via `cmd::tx` for CLI/test paths), not a process-global `static`. Verify hook-related unit tests with `cargo test --lib <filter> -- --test-threads=16` before push.
- Integration tests that need `#[cfg(test)]` hooks on tx commit/rollback paths must call in-process helpers such as `execute_plan_direct()` in `tests/integration.rs`. `assert_cmd::cargo_bin` subprocesses load the release binary and cannot see library `cfg(test)` hooks.
- **Permission-based tests (`chmod 000`) must include a root-skip guard.** Root bypasses UNIX permission checks, so `from_mode(0o000)` tests fail inside Docker containers and root CI runners. After setting mode 000, try to read the file; if the read succeeds, restore permissions and return early. This tests actual behavior rather than checking the UID:

```rust
fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
// Root (common in Docker) can still read mode-000 files. Skip when
// permissions do not actually block reading.
if fs::read_to_string(&file).is_ok() {
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    return;
}
```

- **Confirm / TTY-gated unit tests must not depend on ambient stdin.** Docker, devcontainers, and eval harnesses often allocate a pseudo-TTY on stdin. Unit tests that set `confirm = true` and assume non-TTY auto-decline hang, print `Apply? [Y/n]`, default to yes, and return exit 0 instead of decline. Prefer inject (`confirm_prompt_interactive(false, reader)`) or force decline with `ConfirmAnswerGuard::force(false)` (thread-local, same RAII pattern as `RestoreFailGuard`). If you must call production confirm on real stdin, skip when `IsTerminal::is_terminal(&stdin)`. `assert_cmd` subprocesses (piped stdin) and `tests/pty.rs` (intentional TTY) are separate paths. After adding confirm unit tests, grep for `confirm = true` without a guard or inject nearby. See #579, #1556.

### Writes

All file mutations go through `write::atomic_write()`, which uses `tempfile::NamedTempFile` + rename for crash safety. The `WritePolicy` struct controls transformations applied before writing.

## Adding a new command

1. Create `src/cmd/<name>.rs`:

```rust
use crate::cli::global::GlobalFlags;
use crate::exit;
use clap::Args;

#[derive(Debug, Args)]
pub struct <Name>Args {
    // command-specific arguments

    // Include WriteFlags if the command mutates files:
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

pub fn run(args: <Name>Args, global: &GlobalFlags) -> anyhow::Result<u8> {
    let cwd = global.resolve_cwd()?;

    // Use cwd.join(path) for file resolution instead of set_current_dir
    // (set_current_dir is process-global and not thread-safe for parallel tests)

    Ok(exit::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_test() {
        // test with tempfile::TempDir
    }
}
```

2. Add `pub mod <name>;` to `src/cmd/mod.rs`.

3. Add a variant to the `Command` enum:

```rust
/// Description of the command.
<Name>(<name>::<Name>Args),
```

4. Add a dispatch arm in `dispatch()`:

```rust
// For write commands:
Command::<Name>(args) => {
    global.merge_write(&args.write);
    load_project_config(&mut global);
    <name>::run(args, &global)
}
// For read-only commands:
Command::<Name>(args) => {
    load_project_config(&mut global);
    <name>::run(args, &global)
}
```

5. **Write path for commands that mutate files (rewrite singularity):**

| Pattern | When to use | Example commands |
|---------|-------------|------------------|
| `run_write` / `run_write_op` | Standard single-op + standard JSON schema | doc, create, delete, append, ast replace |
| `stage_for_write` + `finalize_execution_result` | Multi-op / standard phase schema | ast rename |
| `stage_for_write` + `finalize_report` | Custom JSON/text **hooks only** (never re-match modes) | replace, tidy fix, patch, md dedupe |
| `execute_write` (`write_dispatch`) | Binary/case-only only (`finalize_callback_write`) | rename (binary / case-only) |

Engine staging is always `tx::engine::stage(WriteRequest)` (`Operations` or `Precomputed`). Prefer `cmd::output` helpers at the CLI boundary. **`match classify_write_mode` is only allowed in `write_mode.rs`.** Do not use `atomic_write()` directly in command implementations.

6. If the command scans multiple files, use `crate::par_process_files()` for adaptive parallelism instead of a sequential loop. The closure must be `Fn + Sync` (no mutable captures). Write-back stays serial.

7. Add tests that cover success, failure, and edge-case exit codes.

8. Update ancillary files that integration tests auto-verify:
   - `tests/agent/drivers/base.py`: add the command name to `_PATCHLOOM_SUBCOMMANDS`.
   - `docs/reference/README.md`: add a `<!-- ref:command:<name> -->` marker with a `## \`<name>\`` heading, description, **Use when:** stanza, and **Related:** links.
   - `docs/blog/launch-announcement.md`: update the command count ("N commands cover...").

9. Run `make sync-patchloom-md && make update-readme && make check`.

**PR body requirement (see #819):** When opening the PR for this work, ensure the body contains `Closes #NNN` (or `Fixes`) lines for every targeted issue. Library follow-ups and polish PRs are the most common place this is missed. Edit via `gh pr edit` if needed before merge.

## Adding a new Plan field

When adding a field to the `Plan` struct in `src/plan/mod.rs`:

1. **Types must live in `plan/` (unconditional).** The `Plan` struct is always compiled (no feature gate). Any type referenced by a `Plan` field must also be unconditional. Do NOT define the type in a feature-gated module (e.g. `tx/verify.rs` behind `cli`/`files`) and import it into `plan`. This breaks `--no-default-features`. Define the type in `plan/` and have the gated module import from `crate::plan` instead. This follows the existing pattern for `FormatStep`, `ValidationStep`, and `Operation`.

2. **Add a reference doc marker.** The test `test_reference_doc_covers_meaningful_feature_inventory` collects all field names from `pub struct Plan` and requires a `<!-- ref:tx-field:fieldname -->` marker in `docs/reference/README.md`. Add a section with the marker, a `### \`fieldname\`` heading, and bullet points describing what it does, when to use it, and failure behavior.

3. **Update all Plan construction sites.** Every place that constructs a `Plan` (tests, engine, batch, MCP, explain) needs the new field. Grep for `Plan {` and `Plan{` across all `.rs` files. Use `#[serde(skip_serializing_if = "Option::is_none")]` and `#[serde(default)]` for backward-compatible optional fields.

4. Run `make sync-patchloom-md && make check`.

**Note:** The same auto-inventory test pattern applies to `WriteFlags` (with `<!-- ref:write-flag:flagname -->` markers). When adding a new field to any struct that has reference doc auto-inventory coverage, grep `tests/integration.rs` for the struct name to find the corresponding inventory test and its expected marker format.

## Adding a new MCP tool

MCP tools live under `src/cmd/mcp/` behind the `mcp` feature gate.

### Surface policy (registry default, custom exception)

| Path | When to use | Where |
|------|-------------|--------|
| **A. Registry** | 1:1 mapping to a write `plan::Operation`, no multi-file preflight, no multi-op batch, no non-plan read UX | `MCP_TOOL_REGISTRY` in `registry.rs` |
| **B. Custom** | Multi-file scan, batch/plan, readonly query, AST analyze/mutate, patch conflict UX, server meta | `handlers.rs` / `ast_tools.rs` **and** a row in `surface::CUSTOM_MCP_TOOLS_CORE` or `_AST` (feature-gated) |

**Do not** move search, batch, `execute_plan`, or AST-read tools into the registry just to reduce the custom count. The metric is “no *unjustified* custom tools,” not “zero custom tools.” Inventory and tests live in [`src/cmd/mcp/surface.rs`](src/cmd/mcp/surface.rs).

### Path A: Auto-generated tool (1:1 Operation mapping)

1. **Ensure the op is in the schema `OpMeta` registry** (`src/schema.rs`) with description + example.
2. **Add an entry to `MCP_TOOL_REGISTRY`** in `src/cmd/mcp/registry.rs`:

```rust
McpToolMeta {
    tool_name: "new_tool",
    op_name: "new_op",  // must match the Operation variant's serde name
    extra: None, // or Some("MCP-only guidance…") — base prose/example come from schema (#1383)
    has_strict: true,   // true if the tool should accept a `strict` parameter
    validations: &[FieldValidation::Path("path")],  // field validations
},
```

The tool description is built by `schema::mcp_tool_description(op_name, extra)`. The input schema is auto-derived via `operation_variant_schema()`. The handler is `handle_simple_op()`.

3. **Update** `mcp_lists_expected_tools` total count and `surface` tests if totals change.
4. **Add integration tests** under `#[cfg(feature = "mcp")]` as needed.
5. **Update** agent-rules / `docs/getting-started/mcp-setup.md` tool tables.
6. Run `make sync-patchloom-md && make update-readme && make check`.

### Path B: Custom hand-written tool

1. **Define a params struct** in `src/cmd/mcp/params.rs` (`Deserialize` + `schemars::JsonSchema`).
2. **Add a `#[tool]` handler** in `src/cmd/mcp/handlers.rs` (AST: thin stub + `ast_tools`).
3. **Add a row** to `CUSTOM_MCP_TOOLS_CORE` or `CUSTOM_MCP_TOOLS_AST` in `src/cmd/mcp/surface.rs` with a one-line `why` (required; AST-gated tools go in `_AST`).
4. Follow Path A steps 3–6 for counts, tests, and docs.

**PR body requirement (see #819):** include `Closes #NNN` / `Fixes #NNN` in the PR body for every resolved issue.

## Removing an MCP tool

1. **For auto-generated tools:** Remove the `McpToolMeta` entry from `MCP_TOOL_REGISTRY` in `src/cmd/mcp/registry.rs`.
   **For custom tools:** Remove the handler from `handlers.rs` / `ast_tools.rs`, the params struct from `params.rs`, and the row from `surface::CUSTOM_MCP_TOOLS_CORE` or `_AST`.

2. **Remove the tool name** from the `mcp_lists_expected_tools` test and update the expected total (and surface tests).

3. **Remove integration tests** for the tool from `tests/integration.rs`.

4. **Remove references** from all documentation that lists MCP tools:
   - `src/cmd/mod.rs` (agent-rules generator)
   - `docs/getting-started/mcp-setup.md`
   - `examples/README.md` (example descriptions)
   - `benches/README.md` (MCP benchmark table)

5. Grep for the tool name across the repo to catch remaining references:

```bash
grep -ri "tool_name" --include="*.md" --include="*.rs" --include="*.json" .
```

6. Run `make sync-patchloom-md && make update-readme && make check`.

## AST module naming (#1376)

- `ast::extract_to_file` — move a named symbol into another source file (plan op `ast.extract_to_file`).
- `ast::symbol_extract` — per-language tree-sitter visitors that build `SymbolDef` lists.
- `ast::rewrite` — function signature rewrite helpers (use this path only; `ast::symbols` no longer re-exports them, #1386).
- Shared position parsers: `ast::group::parse_group_position`, `ast::move_symbols::parse_position` (tx must call these, not re-parse `after:`).

## Module size (hygiene, not a split mandate)

The 1000-line check in `module_hygiene_tests` is a **multi-concern alert**, not a requirement to split single-domain files.

- Prefer **one conceptual unit per module**. Cohesive domain logic (edit fallback chain, YAML comment-preserving splice, multi-language signature rewrite) and co-located MCP unit tests may stay large.
- Production sources over ~1000 lines (excluding co-located `tests.rs` / `*_tests.rs`) need a short `size-waiver: … #NNNN` note naming the domain reason and a policy/history issue for provenance (enforced by `module_hygiene_tests`). Closed issues are fine (policy decision, not open tech-debt).
- **Do not** split only to shrink line counts. Vanity splits that create dual writers, re-export noise, or weaker navigation are a net loss (see closed #1408).
- Split when a **natural seam** appears during real work (a second independent subsystem), not because a counter crossed a threshold.

## Coding conventions

- Run `cargo fmt` before every commit.
- `cargo clippy --all-targets --all-features -- -D warnings` must produce zero warnings.
- `make check` is the full gate. Nothing merges unless it passes.
- All commits require a `Signed-off-by` line (DCO). Use `git commit -s`.
- Keep `main.rs` thin. No business logic in `main.rs` or `lib.rs`.
- Prefer returning exit codes over panicking. Never use `unwrap()` or `expect()` in non-test code; use `?` with `anyhow::Context` or `.ok_or_else(|| anyhow!("msg"))?` instead. Exception: `expect()` is acceptable on infallible internal invariants (e.g. `Mutex::lock`, compile-time-constant `Regex::new`) where failure indicates a logic bug, not a runtime condition.
- `unsafe_code = "deny"` is enforced via `[lints.rust]` in Cargo.toml. No unsafe Rust.
- Use `anyhow::Context` to add context to errors rather than custom `.map_err()` chains.

- When changing how results are populated or filtered (e.g., adding an optimization that skips building result objects), add an integration test that verifies the exit code is still correct for the affected mode. Exit code regressions are invisible to unit tests that only check output format.

- **Exit code audit methodology.** Prefer fixing `write_mode` once. If a command still open-codes exits, migrate it to `write_exit_code` / `finalize_*`. Regression lock: `write_path_contract` integration tests (one representative per staging entry). Historical note: #1345-#1348 fixed the same preview-exit bug independently per path before the shared owner existed.

- Internal refactors and performance optimizations (no user-visible behavior change) still require a targeted unit or integration test on the changed helper or code path. Existing higher-level tests may provide coverage, but a focused test prevents silent regression of the optimization or guard in future refactors.

- When asserting `Send + Sync` bounds on public types, use the `const` static assertion pattern (compile-time, no dead-code warnings):

```rust
const _: () = {
    fn _assert<T: Send + Sync>() {}
    let _ = _assert::<MyType>;
};
```

  Do NOT use a named function calling a named helper (produces dead_code warnings):

```rust
// BAD: generates dead_code warning
fn assert_send_sync<T: Send + Sync>() {}
fn check() { assert_send_sync::<MyType>(); }
```

- Clippy `collapsible_if` with `if let` chains (Rust 2024 edition): nested `} else if cond { if let Err(e) = expr {` must be collapsed to `} else if cond && let Err(e) = expr {`. This fires frequently when validating structured file formats (JSON/YAML/TOML) by file extension.

## Safety rules

- Never use `git add .` or `git add -A`. Stage only the files you changed.
- Never modify `Cargo.toml` without running `cargo build` afterward to regenerate `Cargo.lock`. Both files must be committed together.
- Always run `make check` before committing.
- Do not push directly to `main`. CI runs on pull requests and pushes to `main`.
