# AGENTS.md

## Project overview

Patchloom is a Rust CLI for agent-grade repo operations. It provides seven commands (`search`, `replace`, `patch`, `md`, `doc`, `hygiene`, `tx`) that let AI coding agents perform structured file searches, mechanical replacements, diff-based patching, markdown section editing, JSON/YAML/TOML document manipulation, whitespace normalization, and multi-operation atomic transactions. All write operations support `--check` (dry-run), `--diff` (preview), and `--apply` (mutate) modes.

## Dev commands

| Command | What it does |
|---------|-------------|
| `make fmt` | Run `cargo fmt --all` |
| `make fmt-check` | Check formatting without modifying files |
| `make build` | Run `cargo build` |
| `make test` | Run `cargo test` |
| `make clippy` | Run `cargo clippy --all-targets --all-features -- -D warnings` |
| `make check` | Run all of the above in sequence: `fmt-check`, `build`, `test`, `clippy` |

Always run `make check` before committing. It is the full CI gate.

## Project structure

```
src/
  main.rs             Thin entrypoint; calls patchloom::run(), maps Result to ExitCode
  lib.rs              Parses CLI with clap, delegates to cmd::dispatch; re-exports all modules
  cli/mod.rs           Defines Cli struct (clap Parser) with GlobalFlags and Command subcommand
  cli/global.rs        GlobalFlags struct: --json, --jsonl, --diff, --apply, --check, --cwd,
                       --glob, --atomic, --ensure-final-newline, --normalize-eol,
                       --trim-trailing-whitespace; also defines EolMode enum
  cmd/mod.rs           Command enum (clap Subcommand) and dispatch() function
  cmd/search.rs        Literal/regex search across files with context, count, files-with-matches
  cmd/replace.rs       Literal/regex string replacement with diff preview and atomic write
  cmd/patch.rs         Preview or apply unified diffs
  cmd/md.rs            Markdown section-aware operations (replace section, insert after heading)
  cmd/doc.rs           Parser-backed JSON, YAML, TOML operations (set, delete, merge, append)
  cmd/hygiene.rs       Final newline, line ending, and trailing whitespace normalization
  cmd/create.rs        Create a new file with content
  cmd/tx.rs            Transaction engine: execute a multi-operation plan atomically
  selector/mod.rs      Re-exports selector parser and evaluator
  selector/parser.rs   Path selector parser (key, index, wildcard, predicate segments)
  selector/eval.rs     Evaluate parsed selectors against serde_json::Value trees
  output.rs            Output rendering: Human, Json, Jsonl, Diff modes; SuccessResult/ErrorResult
  error.rs             PatchloomError enum with typed exit codes; implements Display + Error
  exit.rs              Exit code constants: SUCCESS=0, FAILURE=1, CHANGES_DETECTED=2,
                       NO_MATCHES=3, PARSE_ERROR=4, AMBIGUOUS=5, VALIDATION_FAILED=6, ROLLBACK=7
  diff.rs              Unified diff generation using similar::TextDiff; FileDiff and DiffResult types
  write.rs             Atomic file writes via tempfile; WritePolicy applies trim, EOL, final newline
  plan.rs              Transaction plan format: Plan, Operation, ValidationStep; JSON deserialization
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

All subcommands receive a `&GlobalFlags` reference. Output format (`--json`, `--jsonl`, `--diff`), write mode (`--apply`, `--check`), and file transformations (`--ensure-final-newline`, `--normalize-eol`, `--trim-trailing-whitespace`) are controlled here.

### Error handling

- Use `anyhow::Result` for propagating errors.
- Use `PatchloomError` (in `src/error.rs`) when a specific exit code is needed. Each variant maps to a constant in `src/exit.rs`.
- Return `Ok(exit::SUCCESS)` for success, `Ok(exit::NO_MATCHES)` for no-match, etc.

### Testing

- Tests go in `#[cfg(test)] mod tests` blocks at the bottom of each file.
- Use `tempfile::TempDir` for test fixtures that need a filesystem.
- Build a `GlobalFlags` with all fields set to defaults for test helpers. Do not use `Default` on `GlobalFlags` (it does not implement `Default`); construct it manually.
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
}

pub fn run(args: <Name>Args, global: &GlobalFlags) -> anyhow::Result<u8> {
    if let Some(ref cwd) = global.cwd {
        std::env::set_current_dir(cwd)?;
    }

    // implementation

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
Command::<Name>(args) => <name>::run(args, &cli.global),
```

5. Add tests that cover success, failure, and edge-case exit codes.

6. Run `make check`.

## Coding conventions

- Run `cargo fmt` before every commit.
- `cargo clippy --all-targets --all-features -- -D warnings` must produce zero warnings.
- `make check` is the full gate: formatting, build, test, clippy. Nothing merges unless it passes.
- All commits require a `Signed-off-by` line (DCO). Use `git commit -s`.
- Keep `main.rs` thin. No business logic in `main.rs` or `lib.rs`.
- Prefer returning exit codes over panicking. Never use `unwrap()` in non-test code.
- Use `anyhow::Context` to add context to errors rather than custom `.map_err()` chains.

## Safety rules

- Never use `git add .` or `git add -A`. Stage only the files you changed.
- Never modify `Cargo.toml` without running `cargo build` afterward to regenerate `Cargo.lock`. Both files must be committed together.
- Always run `make check` before committing.
- Do not push directly to `main`. CI runs on pull requests and pushes to `main`.
