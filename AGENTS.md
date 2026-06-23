# AGENTS.md

## Project overview

Patchloom is a Rust CLI for agent-grade repo operations. It provides twenty-two commands (`search`, `replace`, `patch`, `md`, `doc`, `tidy`, `append`, `create`, `delete`, `rename`, `read`, `status`, `tx`, `batch`, `explain`, `undo`, `init`, `completions`, `agent-rules`, `schema`, `ast`, `mcp-server`) that let AI coding agents perform structured file searches, mechanical replacements, diff-based patching, markdown section editing, JSON/YAML/TOML document manipulation, whitespace normalization, file appending, file creation, file deletion, file renaming, multi-operation atomic transactions, line-oriented batch operations, human-readable plan summaries, undo safety net with backup restoration, project setup, shell completion generation, end-user agent rules generation, operation schema export with tier filtering, AST-aware code operations (list, read, rename, validate via tree-sitter), and MCP protocol server for structured tool calls. All write operations are dry-run by default and support `--check` (report changes), `--diff` (preview), and `--apply` (mutate) modes.

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
| `make test-library-hygiene` | Enforce Bline library set: clippy + tests under `--no-default-features --features "ast,files"` (catches dead_code, hygiene for #800 #802) |
| `make clippy` | Run `cargo clippy --all-targets --all-features -- -D warnings` |
| `make check` | Run fmt-check, clippy, test, integration-test, pty-test, check-patchloom-md, check-readme |
| `make check-fast` | Fast check: fmt-check, clippy, test, test-library-hygiene, integration-test, pty-test (skips doc verification; enforces library hygiene) |
| `make update-readme` | Update README.md rounded test count (only changes when hundreds digit changes) |
| `make check-readme` | Verify README.md rounded test count is accurate (part of `check`) |
| `make sync-patchloom-md` | Regenerate PATCHLOOM.md from `patchloom agent-rules` output |
| `make check-patchloom-md` | Verify PATCHLOOM.md matches `patchloom agent-rules` output (part of `check`) |
| `make audit-test-hygiene` | Audit test names and weak assertions for staleness after refactors (run after MPI or breaking changes) |
| `make agent-test` | Run agent integration tests (requires LLM API key, not part of `check`). Use `MODEL=X` to switch LLM (e.g. `make agent-test MODEL=sxs-gpt-5-4`) |
| `make fuzz` | Run fuzz tests (8 targets: selector parse, patch parse, patch apply, batch tokenize, selector eval, doc parse, containment_check, fallback_resolve). Requires nightly, not part of `check`. Use `FUZZ_TIME=N` for seconds per target |
| `make bench-cli` | Run CLI benchmarks vs native tools (requires `hyperfine`, not part of `check`) |
| `make bench-mcp` | Run MCP benchmarks: per-call latency vs CLI process spawn (not part of `check`) |
| `make bench-agent` | Run LLM agent A/B benchmarks (requires API key, not part of `check`). Use `MODEL=X RUNS=N` to configure runs |
| `make bench-agent-dry-run` | Preview agent benchmark prompts without calling the LLM API |
| `make bench-agent-report` | Generate comparison report from saved agent benchmark results. Use `FILE=path` for specific file |
| `make git-clean` | Remove known temp files that pollute `git status` (e.g. `.lycheecache` from lychee) |
| `make clean` | Remove build artifacts and temp files |

Always run `make check` before committing. It is the full CI gate.

## Git hygiene

Keep the working tree clean:

- `git status --short` should be empty (except when intentionally on the release-please synthetic branch).
- Run `make git-clean` to remove temp files such as `.lycheecache` (created when `cache = true` in `lychee.toml`).
- At the end of any session or before switching tasks: `git fetch --all --prune`, `make git-clean`, `git status --short`, and ensure you are on a clean `origin/main` (or the allowed release-please branch).
- Common sources of "dirty" state: lychee cache, local edits during rebase/force work on release-please, Cargo.lock drift from different tool versions. Fix them explicitly rather than carrying them.
- After a core PR merges mid-session (e.g. #753 while polish for #754 was in flight): the feature branch tip is no longer ancestor of main ("has merged PR" from pre-commit hook). Recovery: `gh pr view N --json state` (confirm merged), `git checkout -b rescue-YYYYMMDD origin/main`, cherry-pick the useful commits (or `git show <oldsha> | patch -p1`), `git add <explicit files only>`, commit -s, push, create PR. See patchloom-contrib for full "Follow-up polish after base PR" and #759.

See also the branch hygiene rules in `~/.grok/skills/patchloom-contrib/SKILL.md`.

## Release PRs (release-please)

- The open release-please PR (#724 etc.) title must be correct. Use `gh pr edit --title` when it shows the wrong version.
- The PR *body* can be very long and may temporarily show the wrong next version header (release-please behavior). This is tracked as tech-debt #740.
- When updating library embedding examples (in lib.rs, README, docs/), keep the version string in sync with the current Cargo.toml / .release-please-manifest.json (avoids the 0.4 vs 0.5 drift reported in #816 follow-up).
- Primary curation is done via `RELEASE_NOTES.md` (applied to the final GitHub Release by the host job, not the PR body).
- See `patchloom-contrib` skill ("Curated release notes" and "Major version bumps" sections) for the full process.

## Project structure

```
src/
  main.rs             Thin entrypoint; calls patchloom::run(), maps Result to ExitCode
  lib.rs              Parses CLI with clap, delegates to cmd::dispatch; re-exports modules
  files.rs             File-walking utilities: is_binary, collect_file_paths, build_glob_matcher,
                       matches_glob. Used by search, replace, tidy, and status commands.
  cli/mod.rs           Defines Cli struct (clap Parser) with GlobalFlags and Command subcommand
  cli/global.rs        GlobalFlags (read-only: --json, --jsonl, --quiet, --cwd, --glob,
                       --files-from) and WriteFlags (--diff, --apply, --check,
                       --ensure-final-newline, --normalize-eol, --trim-trailing-whitespace,
                       --respect-editorconfig, --confirm). Write flags are only available on write commands.
  cmd/mod.rs           Command enum (clap Subcommand), dispatch(), built-in agent-rules
                       generator, and inline Completions command
  cmd/append.rs        Append content to an existing file
  cmd/batch.rs         Line-oriented batch operations, parses positional args, delegates to tx engine
  cmd/mcp.rs           MCP server (feature-gated): exposes patchloom operations as structured tool calls
  cmd/search.rs        Literal/regex search across files with context, count, files-with-matches, -i
  cmd/replace.rs       Literal/regex string replacement with diff preview, --nth, -i, atomic write
  cmd/delete.rs        Delete a file (with --apply/--check modes)
  cmd/rename.rs        Rename (move) a file (with --apply/--check modes, --force for overwrite)
  cmd/patch.rs         Preview or apply unified diffs
  cmd/md.rs            Markdown section-aware operations (replace section, insert before/after heading,
                       upsert bullet, table append, dedupe headings, lint)
  cmd/ast.rs           AST-aware operations (list, read, rename, validate) using tree-sitter
  cmd/doc.rs           Parser-backed JSON, YAML, TOML operations (get, has, keys, len, set,
                       delete, merge, append, prepend, update, move, ensure, delete-where,
                       select, flatten, diff)
  cmd/tidy.rs          Final newline, line ending, and trailing whitespace normalization
  cmd/create.rs        Create a new file with content
  cmd/read.rs          Read file contents with optional line range
  cmd/schema.rs        Export operation schemas with tier filtering and system prompt generation
  cmd/status.rs        Show uncommitted file changes vs git HEAD
  cmd/tx.rs            Transaction engine: execute a multi-operation plan atomically
  cmd/explain.rs       Parse a tx plan and print a human-readable summary
  cmd/undo.rs          Restore files from backup sessions created by --apply
  cmd/init.rs          Project setup: shell completion install, AGENTS.md generation
  config.rs            Project config file (.patchloom.toml) loading and merging
  backup.rs            Backup session management for undo safety net
  schema.rs            Intent format spec: OperationSchema, Tier, operation_schemas(),
                       operations_for_tier(), system_prompt_for_tier(), INTENT_FORMAT_VERSION
  fallback.rs          Multi-strategy fallback chain: EditError, EditErrorKind, validate_edit(),
                       find_similar_targets(), anchor_match(), resolve_with_fallback()
  selector/mod.rs      Re-exports selector parser and evaluator
  selector/parser.rs   Path selector parser (key, index, wildcard, predicate segments)
  selector/eval.rs     Evaluate parsed selectors against serde_json::Value trees
  exit.rs              Exit code constants: SUCCESS=0, FAILURE=1, CHANGES_DETECTED=2,
                       NO_MATCHES=3, PARSE_ERROR=4, AMBIGUOUS=5, VALIDATION_FAILED=6, ROLLBACK=7, CONFLICTS=8
  diff.rs              Unified diff generation using similar::TextDiff; FileDiff and DiffResult types
  ops.rs               Shared operation helpers used by cmd/tx.rs, cmd/doc.rs, cmd/md.rs:
                       replace (validation, replacement text, content replacement),
                       doc (format detection, parsing, navigation, merge, update),
                       md (heading parse, section replace, bullet upsert, table append),
                       patch (parse, apply hunks with fuzz, loader). Each is a pub(crate) submodule.
  write.rs             Atomic file writes via tempfile; WritePolicy applies trim, EOL, final newline
  plan.rs              Transaction plan format: Plan, Operation, FormatStep, ValidationStep;
                       25 operation types including all doc/md/replace/tidy/file/patch/read/search ops
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

### Testing

- Tests go in `#[cfg(test)] mod tests` blocks at the bottom of each file.
- Use `tempfile::TempDir` for test fixtures that need a filesystem.
- Use `GlobalFlags::default()` for test helpers. Override specific fields with struct update syntax: `GlobalFlags { apply: true, ..GlobalFlags::default() }`.
- Test both the internal functions and the public `run()` function to verify exit codes.
- When embedding file paths in YAML or TOML plan strings in integration tests, use `portable_path_str(&path)` (defined in `tests/integration.rs`) to convert backslashes to forward slashes. Windows paths like `C:\Users` contain `\U` which YAML and TOML parsers interpret as a unicode escape sequence.
- For non-existent file paths in tests, use `nonexistent_path("name")` which returns a platform-appropriate path.
- `cargo test --lib` runs tests in parallel (CI too). For test-only failure-injection hooks, use `thread_local!` plus an RAII guard (e.g. `RestoreFailGuard`, defined in `src/tx.rs` and re-exported via `cmd::tx` for CLI/test paths), not a process-global `static`. Verify hook-related unit tests with `cargo test --lib <filter> -- --test-threads=16` before push.
- Integration tests that need `#[cfg(test)]` hooks on tx commit/rollback paths must call in-process helpers such as `execute_plan_direct()` in `tests/integration.rs`. `assert_cmd::cargo_bin` subprocesses load the release binary and cannot see library `cfg(test)` hooks.

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
    <name>::run(args, &global)
}
// For read-only commands:
Command::<Name>(args) => <name>::run(args, &global),
```

5. If the command scans multiple files, use `crate::par_process_files()` for adaptive parallelism instead of a sequential loop. The closure must be `Fn + Sync` (no mutable captures). Write-back stays serial.

6. Add tests that cover success, failure, and edge-case exit codes.

7. Update ancillary files that integration tests auto-verify:
   - `tests/agent/drivers/base.py`: add the command name to `_PATCHLOOM_SUBCOMMANDS`.
   - `docs/reference/README.md`: add a `<!-- ref:command:<name> -->` marker with a `## \`<name>\`` heading, description, **Use when:** stanza, and **Related:** links.
   - `docs/blog/launch-announcement.md`: update the command count ("N commands cover...").

8. Run `make sync-patchloom-md && make update-readme && make check`.

## Adding a new MCP tool

MCP tools live in `src/cmd/mcp.rs` behind the `mcp` feature gate. To add a new tool:

1. **Define a params struct** with `Deserialize` and `schemars::JsonSchema`:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NewToolParams {
    /// File path (relative to working directory).
    pub path: String,
    // ... other fields
}
```

2. **Add a handler method** in the `#[tool_router] impl PatchloomService` block:

```rust
#[tool(description = "Short description of what the tool does.")]
async fn new_tool(
    &self,
    Parameters(p): Parameters<NewToolParams>,
) -> Result<CallToolResult, McpError> {
    validate_path_contained(&p.path)?;
    // For write tools: build an Operation and call execute_plan() (pass guard for PathGuard support, #755)
    execute_plan(
        make_plan(vec![Operation::Variant { /* fields */ }]),
        &self.cwd,
        None, // or Some(&guard)
    )
    // For read-only tools: call run_readonly_command()
}
```

3. **Add the tool name** to the `mcp_lists_expected_tools` test and update the expected count.

4. **Add integration tests** in `tests/integration.rs` under `#[cfg(feature = "mcp")]`.

5. **Update the tool list** in `src/cmd/mod.rs` (agent-rules generator) and `docs/getting-started/mcp-setup.md`.

6. Run `make sync-patchloom-md && make update-readme && make check`.

## Removing an MCP tool

1. **Remove the handler method and params struct** from `src/cmd/mcp.rs`.

2. **Remove the tool name** from the `mcp_lists_expected_tools` test and update the expected count.

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

## Coding conventions

- Run `cargo fmt` before every commit.
- `cargo clippy --all-targets --all-features -- -D warnings` must produce zero warnings.
- `make check` is the full gate. Nothing merges unless it passes.
- All commits require a `Signed-off-by` line (DCO). Use `git commit -s`.
- Keep `main.rs` thin. No business logic in `main.rs` or `lib.rs`.
- Prefer returning exit codes over panicking. Never use `unwrap()` in non-test code.
- `unsafe_code = "deny"` is enforced via `[lints.rust]` in Cargo.toml. No unsafe Rust.
- Use `anyhow::Context` to add context to errors rather than custom `.map_err()` chains.

- When changing how results are populated or filtered (e.g., adding an optimization that skips building result objects), add an integration test that verifies the exit code is still correct for the affected mode. Exit code regressions are invisible to unit tests that only check output format.

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
