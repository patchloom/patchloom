# AGENTS.md

## Project overview

Patchloom is a Rust CLI for agent-grade repo operations. It provides thirteen commands (`search`, `replace`, `patch`, `md`, `doc`, `hygiene`, `create`, `delete`, `read`, `status`, `tx`, `completions`, `agent-rules`) that let AI coding agents perform structured file searches, mechanical replacements, diff-based patching, markdown section editing, JSON/YAML/TOML document manipulation, whitespace normalization, file creation, file deletion, multi-operation atomic transactions, shell completion generation, and end-user agent rules generation. All write operations are dry-run by default and support `--check` (report changes), `--diff` (preview), and `--apply` (mutate) modes.

## Dev commands

| Command | What it does |
|---------|-------------|
| `make fmt` | Run `cargo fmt --all` |
| `make fmt-check` | Check formatting without modifying files |
| `make build` | Run `cargo build` |
| `make test` | Run unit tests (`cargo test --lib`) |
| `make integration-test` | Run integration tests (`cargo test --test integration`) |
| `make clippy` | Run `cargo clippy --all-targets --all-features -- -D warnings` |
| `make check` | Run all of the above in sequence plus `check-patchloom-md` |
| `make update-readme` | Update README.md and CHANGELOG.md test counts from actual `cargo test` output |
| `make sync-patchloom-md` | Regenerate PATCHLOOM.md from `patchloom agent-rules` output |
| `make check-patchloom-md` | Verify PATCHLOOM.md matches `patchloom agent-rules` output (part of `check`) |
| `make agent-test` | Run agent integration tests (requires LLM API key, not part of `check`). Use `MODEL=X` to switch LLM (e.g. `make agent-test MODEL=sxs-gpt-5-4`) |

Always run `make check` before committing. It is the full CI gate.

## Project structure

```
src/
  main.rs             Thin entrypoint; calls patchloom::run(), maps Result to ExitCode
  lib.rs              Parses CLI with clap, delegates to cmd::dispatch; re-exports modules
  files.rs             File-walking utilities: is_binary, collect_file_paths, build_glob_matcher,
                       matches_glob. Used by search, replace, hygiene, and status commands.
  cli/mod.rs           Defines Cli struct (clap Parser) with GlobalFlags and Command subcommand
  cli/global.rs        GlobalFlags (read-only: --json, --jsonl, --quiet, --cwd, --glob,
                       --files-from) and WriteFlags (--diff, --apply, --check,
                       --ensure-final-newline, --normalize-eol, --trim-trailing-whitespace,
                       --respect-editorconfig). Write flags are only available on write commands.
  cmd/mod.rs           Command enum (clap Subcommand) and dispatch() function
  cmd/search.rs        Literal/regex search across files with context, count, files-with-matches, -i
  cmd/replace.rs       Literal/regex string replacement with diff preview, --nth, -i, atomic write
  cmd/delete.rs        Delete a file (with --apply/--check modes)
  cmd/patch.rs         Preview or apply unified diffs
  cmd/md.rs            Markdown section-aware operations (replace section, insert before/after heading,
                       upsert bullet, table append, dedupe headings, lint)
  cmd/doc.rs           Parser-backed JSON, YAML, TOML operations (get, has, keys, len, set,
                       delete, merge, append, prepend, update, move, ensure, delete-where,
                       select, flatten, diff)
  cmd/hygiene.rs       Final newline, line ending, and trailing whitespace normalization
  cmd/create.rs        Create a new file with content
  cmd/read.rs          Read file contents with optional line range
  cmd/status.rs        Show uncommitted file changes vs git HEAD
  cmd/tx.rs            Transaction engine: execute a multi-operation plan atomically
  selector/mod.rs      Re-exports selector parser and evaluator
  selector/parser.rs   Path selector parser (key, index, wildcard, predicate segments)
  selector/eval.rs     Evaluate parsed selectors against serde_json::Value trees
  agent_rules.md       End-user AGENTS.md template, embedded at compile time by agent-rules command
  exit.rs              Exit code constants: SUCCESS=0, FAILURE=1, CHANGES_DETECTED=2,
                       NO_MATCHES=3, PARSE_ERROR=4, AMBIGUOUS=5, VALIDATION_FAILED=6, ROLLBACK=7
  diff.rs              Unified diff generation using similar::TextDiff; FileDiff and DiffResult types
  ops.rs               Shared operation helpers used by cmd/tx.rs, cmd/doc.rs, cmd/md.rs:
                       replace (validation, replacement text, content replacement),
                       doc (format detection, parsing, navigation, merge, update),
                       md (heading parse, section replace, bullet upsert, table append),
                       patch (parse, apply hunks with fuzz, loader). Each is a pub(crate) submodule.
  write.rs             Atomic file writes via tempfile; WritePolicy applies trim, EOL, final newline
  plan.rs              Transaction plan format: Plan, Operation, FormatStep, ValidationStep;
                       22 operation types including all doc/md/replace/hygiene/file/patch ops
tests/
  integration.rs       Rust integration tests (cargo test --test integration)
  agent/               Python (pytest) agent integration tests verifying AI agents use patchloom
    conftest.py        Fixtures: workspace with AGENTS.md, patchloom shim for invocation capture
    drivers/           Pluggable agent drivers (GrokDriver first, extensible)
    test_basic.py      Search, replace, read scenarios
    test_batch.py      Batch replace, tx multi-file, hygiene scenarios
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

All subcommands receive a `&GlobalFlags` reference. Read-only flags (`--json`, `--jsonl`, `--quiet`, `--cwd`, `--glob` (repeatable), `--files-from`) are global. Write-only flags (`--apply`, `--check`, `--diff`, `--ensure-final-newline`, `--normalize-eol`, `--trim-trailing-whitespace`, `--respect-editorconfig`) are defined in `WriteFlags` and flattened only into write commands. The dispatcher merges them via `GlobalFlags::merge_write()`.

### Error handling

- Use `anyhow::Result` for propagating errors.
- Return exit codes directly using constants from `src/exit.rs` (e.g. `exit::NO_MATCHES`, `exit::CHANGES_DETECTED`).
- Return `Ok(exit::SUCCESS)` for success, `Ok(exit::NO_MATCHES)` for no-match, etc.

### Testing

- Tests go in `#[cfg(test)] mod tests` blocks at the bottom of each file.
- Use `tempfile::TempDir` for test fixtures that need a filesystem.
- Use `GlobalFlags::default()` for test helpers. Override specific fields with struct update syntax: `GlobalFlags { apply: true, ..GlobalFlags::default() }`.
- Test both the internal functions and the public `run()` function to verify exit codes.

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

7. Run `make check`.

## Coding conventions

- Run `cargo fmt` before every commit.
- `cargo clippy --all-targets --all-features -- -D warnings` must produce zero warnings.
- `make check` is the full gate: formatting, build, test, clippy. Nothing merges unless it passes.
- All commits require a `Signed-off-by` line (DCO). Use `git commit -s`.
- Keep `main.rs` thin. No business logic in `main.rs` or `lib.rs`.
- Prefer returning exit codes over panicking. Never use `unwrap()` in non-test code.
- `#![deny(unsafe_code)]` is enforced. No unsafe Rust.
- Use `anyhow::Context` to add context to errors rather than custom `.map_err()` chains.

- When changing how results are populated or filtered (e.g., adding an optimization that skips building result objects), add an integration test that verifies the exit code is still correct for the affected mode. Exit code regressions are invisible to unit tests that only check output format.

## Safety rules

- Never use `git add .` or `git add -A`. Stage only the files you changed.
- Never modify `Cargo.toml` without running `cargo build` afterward to regenerate `Cargo.lock`. Both files must be committed together.
- Always run `make check` before committing.
- Do not push directly to `main`. CI runs on pull requests and pushes to `main`.
