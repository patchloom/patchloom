# Patchloom Reference

This is the reference for Patchloom's meaningful commands, actions, operations, and notable command modes.

- Start with [Quickstart](../getting-started/quickstart.md) if you want a first success.
- Read [Core Concepts](../getting-started/concepts.md) for shared semantics like write modes, exit codes, and transaction behavior.
- Use this file when you need to choose the right feature or mode for a job, or when a pull request adds meaningful CLI surface and the docs coverage test expects it here.

## Global behaviors

Patchloom has a small set of global features that shape how other commands behave.

### Write modes

Patchloom write commands default to preview mode. The canonical semantics live in [Core Concepts](../getting-started/concepts.md#write-modes). The sections below focus on when to choose each mode.

<!-- ref:write-flag:diff -->
### `--diff`

- **What it does:** Prints the unified diff for a write command without mutating files.
- **Use when:** You want a human review step before applying a change, or you want to inspect the exact patch Patchloom would write.
- **Prefer instead:** Use `--check` for CI pass or fail behavior, or `--apply` to actually write files.

<!-- ref:write-flag:apply -->
### `--apply`

- **What it does:** Writes the requested change to disk.
- **Use when:** You have already previewed the change, or you trust the command and want the mutation to happen now.
- **Prefer instead:** Use `--diff` when reviewing, or `--check` when you only need a clean or dirty signal.

<!-- ref:write-flag:check -->
### `--check`

- **What it does:** Calculates whether a write command would change files and returns exit code 2 when changes are pending.
- **Use when:** You are wiring Patchloom into CI, pre-commit validation, or agent workflows that should fail on drift.
- **Prefer instead:** Use `--diff` when you need the actual patch text, or `--apply` when you want the mutation.

<!-- ref:write-flag:confirm -->
### `--confirm`

- **What it does:** Shows the diff preview, then prompts `Apply? [Y/n]` on stderr. If confirmed, applies the change; if declined, exits without writing.
- **Use when:** You want a single-command preview-then-apply workflow instead of running the command twice.
- **Prefer instead:** Use `--apply` when scripting (no interactive prompt), or `--diff` when you only want the preview.

`--diff`, `--apply`, `--check`, and `--confirm` are mutually exclusive. Passing more than one is rejected with an error. When none is specified, `--diff` is the default. When `--confirm` is used and stdin is not a TTY, the command shows the diff without prompting (same as `--diff`).

### Write policy flags

These flags shape how written content is normalized before it reaches disk.

<!-- ref:write-flag:ensure-final-newline -->
### `--ensure-final-newline`

- **What it does:** Ensures non-empty written files end with `\n`.
- **Use when:** You want simple newline hygiene on every touched file without running a separate cleanup command.
- **Prefer instead:** Use `tidy fix` when the goal is repo cleanup, not just normalization of files already being edited.

<!-- ref:write-flag:normalize-eol -->
### `--normalize-eol`

- **What it does:** Normalizes written line endings to `keep`, `lf`, `crlf`, or `cr`.
- **Use when:** A repo or downstream tool expects a specific line ending convention.
- **Prefer instead:** Use `--respect-editorconfig` when the repo already declares the desired convention there.

<!-- ref:write-flag:trim-trailing-whitespace -->
### `--trim-trailing-whitespace`

- **What it does:** Removes trailing spaces and tabs from touched lines before writing.
- **Use when:** You want text cleanup to happen automatically as part of another write command.
- **Prefer instead:** Use `tidy fix` when the goal is to sweep existing files for whitespace problems.

<!-- ref:write-flag:respect-editorconfig -->
### `--respect-editorconfig`

- **What it does:** Reads `.editorconfig` when present and applies matching write policy.
- **Use when:** The repo already encodes formatting policy in `.editorconfig` and Patchloom should follow it automatically.
- **Prefer instead:** Use explicit write flags, or `tx` `write_policy`, when the command should be self-contained and not depend on repo metadata.

<!-- ref:write-flag:collapse-blanks -->
### `--collapse-blanks`

- **What it does:** Collapses consecutive blank lines into a single blank line after writing. Useful after line deletion to prevent double-blank gaps.
- **Use when:** You are deleting lines (e.g. with `replace --whole-line --new ''`) and want to clean up the resulting blank line runs.
- **Prefer instead:** Omit when consecutive blank lines are intentional (e.g. section separators in code).

<!-- ref:write-flag:format -->
### `--format`

- **What it does:** Runs a shell command after every successful `--apply` write. Intended for formatters (e.g. `prettier --write .`, `cargo fmt`).
- **Use when:** The repo has an autoformatter and you want Patchloom to invoke it after each mutation so files stay formatted.
- **Prefer instead:** Omit when the formatter is already run separately, or when using `--diff`/`--check` modes (the command only fires on `--apply`).
- **Failure behavior:** Non-zero exit or timeout exits **1** with `error_kind: "format_failed"` under `--json`/`--jsonl`. The write may already be on disk; JSON includes `backup_session` when a session was created, plus `applied: true` (canonical; #1831), `write_applied: true` (deprecated alias), `files_changed`, and `files[].path` for written paths (#1795). Use `undo` or re-run the formatter.

<!-- ref:write-flag:format-timeout -->
### `--format-timeout`

- **What it does:** Sets the maximum time in seconds the `--format` command is allowed to run before being killed. Defaults to 30 seconds.
- **Use when:** The formatter is slow (e.g. large monorepo) and the default 30 second timeout is insufficient.
- **Prefer instead:** Keep the default unless the formatter demonstrably needs more time.
- **Failure behavior:** Exceeding the timeout kills the formatter process tree and exits **1** with `error_kind: "format_failed"` (same envelope as a failing `--format` command).

<!-- ref:write-flag:no-format -->
### `--no-format`

- **What it does:** Disables post-write formatting even if configured in `.patchloom.toml` (via `[format] auto = true` or `[defaults] format`).
- **Use when:** You want to skip formatting for a single invocation without changing the project config.
- **Prefer instead:** Omit when you want the configured formatter to run normally.

### Output and scope flags

These flags affect how Patchloom reports results or chooses which files to touch.

<!-- ref:global-flag:json -->
### `--json`

- **What it does:** Emits one machine readable JSON document for the command result.
- **Use when:** Another tool, script, or agent needs structured output instead of human oriented text.
- **Prefer instead:** Use `--jsonl` when you want one JSON object per result line for streaming style consumers.

<!-- ref:global-flag:jsonl -->
### `--jsonl`

- **What it does:** Emits one compact JSON value per result line instead of one aggregate document.
- **Use when:** A command naturally yields multiple result records, or you want compact machine-readable output from single-result commands like `create`, `delete`, `rename`, `status`, `tx`, `explain`, or `undo`.
- **Prefer instead:** Use `--json` when you want one aggregate document for the whole command.

<!-- ref:global-flag:quiet -->
### `--quiet`

- **What it does:** Suppresses non-JSON human readable output.
- **Use when:** Only the exit code or the file mutation matters and extra stdout noise would get in the way.
- **Prefer instead:** Use `--json` when another tool still needs structured output.

<!-- ref:global-flag:cwd -->
### `--cwd`

- **What it does:** Sets the working directory used to resolve relative paths for operation targets **and** meta-input files: `tx` plan files, `batch` ops files, `patch` patch files, `explain` plan files, and `--files-from` list files (absolute meta paths are unchanged).
- **Use when:** You are invoking Patchloom from outside the target repo, or you want scripts to behave predictably regardless of the caller's current directory. Example: `patchloom --cwd /repo batch ops.txt` finds `/repo/ops.txt`.
- **Prefer instead:** Use a plan level `cwd` in `tx` when the directory choice should travel with the plan itself, but keep it inside the invocation root. Relative plan `cwd` values resolve from the caller's working directory (`--cwd` or the process cwd), not from the plan file location.
- **Not a sandbox:** Without `--contain`, paths may escape via `../` or absolute paths. MCP always enforces containment; use `--contain` for the same on CLI.

<!-- ref:global-flag:contain -->
### `--contain`

- **What it does:** Rejects file paths that escape the working directory (via `../`, absolute paths that resolve **outside** the workspace, or symlinks that resolve outside the workspace). Uses `PathGuard` with **`AllowIfContained`**: absolute paths that canonicalize under `--cwd` (or the process cwd) are allowed so agents may pass absolutized paths. Mirrors library-style containment for **reads and writes**: explicit paths on `search` / `read` / `replace` / `create` / `delete` / `rename` / `append` / `prepend` / `patch` / `md` / `doc` / `tidy` / `ast` (list, read, rename, validate, search, refs, deps, map, impact, diff, replace) / `status` / `tx` / `batch` (including binary/case-only rename). Also applies to **meta-input files**: `tx`/`explain` plan files, `batch` ops files, `patch` patch files, and `--files-from` list files (so a list path cannot open `/etc/passwd` or `../outside.txt` under `--contain`). MCP remains stricter (`Reject` all absolute path *strings*).
- **Use when:** An agent or automation should not be able to read or write outside `--cwd` (or the process cwd). Pair with `--cwd` for a workspace root.
- **Default:** Off. CLI remains unrestricted for human scripts (same trust model as `make` / `sh`).
- **Prefer instead:** Use the MCP server when the agent already has MCP tools; containment is always on there (and still rejects absolute path strings at the MCP layer).

<!-- ref:global-flag:glob -->
### `--glob`

- **What it does:** Restricts candidate files by one or more glob patterns. Patterns match either the basename or the path relative to the input root, so `sub/*.txt` matches files under a searched `sub/` directory.
- **Use when:** A command should only see a narrow file type or subtree, even if the input path is broader.
- **Prefer instead:** Use `--files-from` when another tool has already determined the exact file list.

<!-- ref:global-flag:exclude -->
### `--exclude`

- **What it does:** Excludes paths matching the given glob patterns (applied after .gitignore and any custom ignore files). May be repeated. Complements `--glob`.
- **Use when:** You want to layer additional excludes (e.g. `target/**` or build artifacts) on top of custom ignore files / `.gitignore` for search, replace, or tidy.
- **Parity:** Matches `SearchOptions.exclude_patterns` and the library `collect_file_paths_with_ignores` precedence.

<!-- ref:global-flag:ignore-file -->
### `--ignore-file`

- **What it does:** Specifies additional gitignore-style ignore files (e.g. `.agentignore`, `.cursorignore`) to respect during file collection. May be repeated.
- **Use when:** LLM agents or projects use tool-specific ignore files and want CLI / tx / MCP search (and replace/tidy) to honor the same layered ignores as the pure-library API.
- **Parity:** Matches `SearchOptions.custom_ignore_filenames`.

<!-- ref:global-flag:files-from -->
### `--files-from`

- **What it does:** Reads the target file list from a file, or from stdin when passed `-`. A relative list path is resolved under `--cwd` when that flag is set (absolute list paths are unchanged). Paths **inside** the list are still resolved against `--cwd` / the process cwd as usual, and under `--contain` each listed path must stay in the workspace.
- **Exact paths only:** Each line is one file path. Directory entries are skipped (not walked). Blank lines are ignored. Lines whose first non-whitespace character is `#` are comments and are ignored (#1811; gitignore-style). Paths that literally start with `#` are not supported.
- **No walk fallback:** An empty list (or only blanks) searches/replaces **zero** files. Patchloom does **not** fall back to walking `.`. Search/replace "no matches" messages name `--files-from …` rather than bare `.`.
- **Use when:** Another tool already selected the exact paths and Patchloom should operate only on that set.
- **Prefer instead:** Use `--glob` for pattern based scoping, or direct path arguments when the target set is already small and obvious.

<!-- ref:global-flag:color -->
### `--color`

- **What it does:** Controls when ANSI color codes appear in output. `auto` (default) enables color when stdout is a terminal and the `NO_COLOR` environment variable is not set. `always` forces color even when piped. `never` disables color unconditionally.
- **Use when:** You need to override the default terminal detection, for example forcing color into a pager or disabling it in a terminal that renders escape codes literally.
- **Prefer instead:** Set the `NO_COLOR` environment variable when you want a global, tool-agnostic way to disable color across all CLI tools.

<!-- ref:global-flag:format-config -->
### `format_config` (internal)

- **What it does:** Carries the per-extension formatter configuration loaded from `.patchloom.toml` (`[format.by_extension]` table) so that post-write formatting can run the correct formatter for each file type.
- **Use when:** You configure `[format] auto = true` and `[format.by_extension]` in `.patchloom.toml`. The field is populated automatically; it is not set via CLI flags.

<!-- ref:global-flag:verbose -->
### `--verbose`

- **What it does:** Prints diagnostic messages to stderr prefixed with `[patchloom]`. Shows which operations are running, search parameters, selector path evaluation steps, and MCP tool call timing. Can also be enabled by setting the `PATCHLOOM_LOG` environment variable to any value.
- **Use when:** A command produces unexpected results and you need to see what Patchloom is doing internally without reading source code.
- **Prefer instead:** Use `--json` when you need machine-readable output for downstream tools.

### Exit codes

Use [Core Concepts](../getting-started/concepts.md#exit-codes) as the canonical exit code table. When integrating Patchloom into CI or agent workflows, branch on exit codes instead of parsing human readable output.

## Commands

These are the main entry points. If you are deciding between commands, start here.

<!-- ref:command:search -->
## `search`

- **What it does:** Searches text files with literal or regex matching, optional context, counts, and file only results. Binary and invalid UTF-8 files are skipped.
- **Use when:** You need to locate candidate edits, audit repo state, or narrow inputs before changing files. For AI agents, native search/grep tools are typically faster for simple pattern matching.
- **Prefer instead:** Use `replace` for actual text mutation, or `doc`, `md`, or `patch` when you already know the structured change you want.
- **Failure behavior:** Pattern miss on existing roots exits `3` with `error_kind: "no_matches"`. When every explicit path root (or non-stdin `--files-from` entry) is missing, exit `1` with `error_kind: "not_found"`. Empty pattern, incompatible flags, and invalid regex patterns use `invalid_input`.
- **Related:** `--glob`, `--files-from`, `replace`

<!-- ref:command:replace -->
## `replace`

- **What it does:** Performs mechanical string replacement across one or many text files, with literal or regex matching. Binary and invalid UTF-8 files are skipped.
- **Use when:** You are doing a rename, version bump, boilerplate rewrite, or another string level change where plain text semantics are enough. For AI agents doing single-file replacements, native search_replace tools are typically faster; use patchloom `replace` inside `tx` plans when batching multiple file edits.
- **Prefer instead:** Use `doc` for structured data, `md` for heading aware markdown, or `patch` when you already have a unified diff.
- **Failure behavior:** Soft pattern miss exits `3` with `error_kind: "no_matches"`; `--unique` multi-match exits `5` with `ambiguous`. All-explicit-path-missing (or all-missing `--files-from` list) exits `1` with `not_found`. Empty `--files-from` (empty list file or empty stdin) exits `1` with `invalid_input` (not pattern miss; #1796). Validation failures and invalid regex patterns use `invalid_input`.
- **Multi-path honesty:** Explicit multi-file lists report zero-match paths under `refused[]` with `reason: no_matches` while applying matches on other paths (#1792). Missing explicit paths soft-skip under `skipped[]` on CLI (partial apply); MCP `batch_replace` hard-fails missing paths and rolls back (#1793). Under `--json`/`--jsonl`, missing-path stderr lines are suppressed when paths are already in `skipped[]` (#1797).
- **Related:** `search`, `tx`

<!-- ref:command:patch -->
## `patch`

- **What it does:** Checks or applies a unified diff.
- **Use when:** The change already exists as a patch, or you want stale context detection instead of search and replace semantics.
- **Paths:** A relative patch file path is resolved under `--cwd`. Paths *inside* the unified diff are also resolved against `--cwd`.
- **Prefer instead:** Use `replace`, `doc`, or `md` when you want to describe the mutation directly instead of carrying a diff artifact.
- **Related:** `patch check`, `patch apply`, `patch merge`, `tx patch.apply`

<!-- ref:command:md -->
## `md`

- **What it does:** Performs heading aware markdown edits for sections, bullets, tables, and AGENTS linting.
- **Use when:** Documentation needs semantic markdown edits that should not depend on raw byte offsets.
- **Prefer instead:** Use `replace` for simple line level edits, or `patch` for exact diff application.
- **Related:** `md` actions, `tx` markdown operations

<!-- ref:command:doc -->
## `doc`

- **What it does:** Performs parser backed JSON, YAML, and TOML queries and mutations.
- **Use when:** Config or metadata changes should operate on keys and arrays instead of brittle text matching.
- **Prefer instead:** Use `replace` for plain text, `md` for markdown, or `patch` for existing diffs.
- **Related:** `doc` actions, `tx` document operations

<!-- ref:command:tidy -->
## `tidy`

- **What it does:** Checks or fixes trailing whitespace, line endings, and final newlines in text files. Binary and invalid UTF-8 files are skipped.
- **Use when:** You need repo text normalization, or a CI guard for basic text tidiness.
- **Failure behavior:** `tidy fix` with both `--dedent` and `--indent` exits `1` with `error_kind: "invalid_input"` under `--json`/`--jsonl`. When every explicit path root is missing, exit `1` with `error_kind: "not_found"` (not vacuous clean success). Pending tidy issues under `tidy check` exit `2` (`CHANGES_DETECTED`).
- **Prefer instead:** Use write policy flags when the cleanup should only apply to files already being touched by another command.
- **Related:** `tidy check`, `tidy fix`, `tx tidy.fix`

<!-- ref:command:append -->
## `append`

- **What it does:** Appends content to the end of an existing file. If the file does not end with a newline, one is inserted before the appended content. Exactly one of `--content` or `--stdin` is required. Fails if the file does not exist (unlike `create`).
- **Use when:** Adding tests, changelog entries, rules, or any content to the end of a file without reading the entire file to find a unique anchor.
- **Failure behavior:** Missing file exits `1` with `error_kind: "not_found"`; bad flags or directory targets use `invalid_input` (JSON envelopes).
- **Prefer instead:** Use `replace` when the insertion point is not the end of the file.
- **Related:** `create`, `prepend`, `tx file.append`

<!-- ref:command:prepend -->
## `prepend`

- **What it does:** Prepends content to the beginning of an existing file. Exactly one of `--content` or `--stdin` is required. Fails if the file does not exist or if the target is a directory.
- **Use when:** Adding headers, copyright notices, shebang lines, or any content to the beginning of a file without reading the entire file to find a unique anchor.
- **Failure behavior:** Same as `append`: `not_found` / `invalid_input` under `--json`/`--jsonl`.
- **Prefer instead:** Use `replace` when the insertion point is not the beginning of the file.
- **Related:** `append`, `create`, `tx file.prepend`

<!-- ref:command:create -->
## `create`

- **What it does:** Creates a file from literal content or stdin. Exactly one of `--content` or `--stdin` is required. Passing both is rejected with `--content and --stdin cannot be combined`, and passing neither is rejected with `either --content or --stdin must be provided`. Directory targets are rejected in all modes. When combined with `--confirm` and `--json` or `--jsonl`, the structured output includes `applied: true|false` so callers can tell whether the prompt was accepted.
- **Use when:** Generating a new tracked file is the whole task, or one step in a larger transaction. For AI agents creating a single file, native file creation tools are typically faster; use `file.create` inside `tx` plans when bundling with other edits.
- **Failure behavior:** Existing file without `--force` exits `1` with `error_kind: "already_exists"`; bad flags/non-file targets use `invalid_input`.
- **Prefer instead:** Use `doc`, `md`, or `replace` when the file already exists and only needs edits.
- **Related:** `delete`, `tx file.create`

<!-- ref:command:delete -->
## `delete`

- **What it does:** Removes a file. Directory targets are rejected in all modes. When combined with `--confirm` and `--json` or `--jsonl`, the structured output includes `applied: true|false` so callers can tell whether the prompt was accepted.
- **Use when:** A file should disappear outright and no other atomic edits are needed. For AI agents deleting a single file, native delete tools are typically faster; use `file.delete` inside `tx` plans when bundling with other edits.
- **Failure behavior:** Missing file exits `1` with `error_kind: "not_found"`; directory targets use `invalid_input`.
- **Prefer instead:** Use `tx file.delete` when the removal must be bundled atomically with other changes.
- **Related:** `create`, `tx file.delete`

<!-- ref:command:rename -->
## `rename`

- **What it does:** Moves (renames) a file from one path to another. Source and destination must both be file paths, not directories. When combined with `--confirm` and `--json` or `--jsonl`, the structured output includes `applied: true|false` so callers can tell whether the prompt was accepted.
- **Use when:** A file needs to be relocated and no other atomic edits are needed. Use `file.rename` inside `tx` plans when bundling with other edits.
- **Failure behavior:** Missing source exits `1` with `error_kind: "not_found"`; destination exists without `--force` uses `already_exists`; non-file paths use `invalid_input`.
- **Prefer instead:** Use `tx file.rename` when the rename must be bundled atomically with other changes.
- **Related:** `create`, `delete`, `tx file.rename`

<!-- ref:command:tx -->
## `tx`

- **What it does:** Runs multiple operations atomically, then optional format and validate steps.
- **Use when:** Editing 3 or more files in one task. Batches N operations into 1 tool call, eliminating agent round-trips. Also provides atomicity, rollback, and format/validate lifecycle. For AI agents, this is the primary speed advantage: one call instead of N.
- **JSON status:** Preview (default), `--diff`, and `--check` with pending changes report `status: "changes_detected"` and exit `2`. Applied success reports `status: "success"` and exit `0`. Do not treat `ok: true` alone as "applied."
- **Prefer instead:** Use standalone commands when one direct operation is enough.
- **Related:** [examples](https://github.com/patchloom/patchloom/tree/main/examples), `tx` fields, `tx` operations

<!-- ref:command:batch -->
## `batch`

- **What it does:** Executes multiple operations from a simple line-oriented format. Each line is one operation with positional arguments (e.g., `doc.set config.json version "2.0.0"`). Internally builds a tx plan and delegates to the tx engine.
- **Use when:** Editing multiple files and the JSON tx plan format is too verbose. The line format covers 28 operations (doc.set, doc.delete, doc.merge, doc.ensure, doc.append, doc.prepend, doc.update, doc.move, doc.delete_where, replace with optional flags, file.append, file.prepend, file.create, file.delete, file.rename, md.upsert_bullet, md.table_append, md.replace_section, md.insert_after_heading, md.insert_after_section, md.insert_before_heading, md.move_section, md.dedupe_headings, md.lint_agents, tidy.fix, ast.rename, ast.replace, ast.rewrite_signature) with minimal syntax. For AI agents, this is faster to generate than a full JSON plan.
- **Paths:** A relative ops file path is resolved under `--cwd` (same as `tx` plan files). Paths *inside* ops lines are also resolved against `--cwd`.
- **Quoting:** Double-quoted tokens allow only `\"` and `\\`. Sequences like `\n` are **literal** (not newlines). Prefer `tx` / MCP JSON for multi-line content, or put real newlines outside one-line quoted strings.
- **Failure behavior:** Line parse failures (unknown op, bad arity, bad quotes) exit `4` (`PARSE_ERROR`) with `error_kind: "parse_error"` under `--json`/`--jsonl`. Too many operations (over the hard cap) exits `1` with `invalid_input`. Runtime op failures use the shared tx exit codes. Preview with changes uses the same `status: "changes_detected"` / exit `2` contract as `tx`.
- **Prefer instead:** Use `tx` when you need format/validate lifecycle steps, strict mode, multi-line content, or operations not supported by the line format (patch.apply, replace with regex/nth, search, read).
- **Related:** `tx`

<!-- ref:command:read -->
## `read`

- **What it does:** Prints the contents of one or more files, optionally restricted to a line range. Multiple files get `==> path <==` separators in text mode, a JSON array in `--json` mode, and one object per line in `--jsonl` mode. If at least one requested file is read successfully, the command still exits successfully and reports errors only for the missing files.
- **Use when:** An agent needs to inspect one or several files before deciding on an edit. For AI agents, native read_file tools are typically faster for single-file reads.
- **Failure behavior:** Invalid `--lines` exits `1` with `error_kind: "invalid_input"`. When every path fails, exit `1` with `error_kind: "not_found"`. Partial success still exits `1` after emitting successful reads.
- **Prefer instead:** Use `search` when you need pattern matching, or `doc get` when the file is structured and you want a single value.

## Library API

Patchloom can be used as a Rust library (disable default `cli` feature for smaller dep). See `patchloom::api` (search_directory with context/globs/max_results, replace_text, read, etc) and `execute_plan` for tx. Full details and examples in crate docs and README "Embedding as a library". Recent expansions (#779 etc) and hygiene (#784) improved coverage.

- **Related:** `search`, `doc get`

<!-- ref:command:status -->
## `status`

- **What it does:** Shows which files have uncommitted changes compared to git HEAD. This command is git-backed, so it must run inside a git repository.
- **Internal paths:** Entries under `.patchloom/` (backup sessions from `--apply`) are omitted so status reflects user project files, not Patchloom's undo store.
- **Use when:** An agent needs a quick summary of the working tree before committing, staging, or choosing which files to process. For AI agents, native git status or terminal commands are typically equivalent.
- **Failure behavior:** Outside a git repository (or if `git status` fails) exits `1` with `error_kind: "invalid_input"` under `--json`/`--jsonl`, and prints a `git init` hint in text mode.
- **Prefer instead:** Use `git status` directly when you need full git porcelain output or staging details (including untracked `.patchloom/` if you care about it).
- **Related:** `search`, `read`, `undo`

<!-- ref:command:undo -->
## `undo`

- **What it does:** Previews or restores files from a backup created by a previous write `--apply`. Before any `--apply` write, patchloom saves originals under `.patchloom/backups/<timestamp>/`. **Default is dry-run** (same singularity as other write commands): bare `patchloom undo` prints what would be restored, exits `2` (`CHANGES_DETECTED`), and does not change files. Pass `--apply` to restore (exit `0`). There is no `--latest`; without `--session`, the most recent backup is used. Dry-run JSON includes `status: "changes_detected"` and a `hint` field reminding agents to pass `--apply`.
- **Use when:** An `--apply` operation produced an undesirable result and you want to revert. Especially useful when the working tree was not committed before applying changes.
- **Notable flags:**
  - `--list` shows available backup sessions. `--json` emits the full session list as one array, while `--jsonl` emits one session object per line.
  - `--session <timestamp>` targets a specific session (defaults to most recent).
  - `--apply` actually restores files (required for a real restore; omitted = preview only).
- **Failure behavior:** No backup sessions (`--list` empty, or restore with no sessions) exits `3` (`NO_MATCHES`). With `--json`/`--jsonl`, the error envelope includes `error_kind: "no_matches"` so agents can branch without scraping stderr.
- **Agent trap:** Do not treat exit `2` from bare `undo` as a completed restore. Re-run with `--apply`.
- **Prefer instead:** Use `git checkout` or `git stash` when working in a committed git repo.
- **Related:** `tx`, `replace`, `tidy`

<!-- ref:command:explain -->
## `explain`

- **What it does:** Parses a tx plan (JSON, YAML, or TOML) and prints a numbered, human-readable summary of each operation. Supports `--json` and `--jsonl` for structured output, plus `--stdin` for piped input. If both a path and `--stdin` are provided, stdin takes precedence and the path is ignored.
- **Use when:** A user or agent wants to review what a tx plan will do before running `tx --apply`. Converts machine-readable plan format into plain English descriptions.
- **Prefer instead:** Use `tx` directly (without `--apply`) to see the actual diff preview. Use `explain` when you want a quick overview without touching any files.
- **Related:** `tx`, `batch`

<!-- ref:command:schema -->
## `schema`

- **What it does:** Exports the complete registry of patchloom operations with JSON Schemas, tier-filtered subsets, and LLM-ready system prompt fragments. Each operation is annotated with a minimum capability tier (weak, medium, strong).
- **Use when:** You are building an AI agent that uses patchloom programmatically and need machine-readable operation schemas, or you want to generate a system prompt tailored to a specific model tier.
- **Notable flags:**
  - `--format json|prompt` (default: `json`): `json` outputs operation schemas as JSON, `prompt` outputs markdown suitable for LLM system prompts.
  - `--tier weak|medium|strong`: Filter operations by minimum capability tier. Help lists these as the only allowed values (clap enum; no `small`/`large` aliases).
  - `--examples`: Include usage examples in JSON output (omitted by default).
- **Prefer instead:** Nothing; this is the only programmatic way to discover available operations and their schemas.
- **Related:** `agent-rules`, `mcp-server`

<!-- ref:command:agent-rules -->
## `agent-rules`

- **What it does:** Prints an end-user AGENTS.md that teaches AI agents how to use patchloom. Includes command reference, exit codes, write modes, transaction plan format, and usage examples.
- **Use when:** You are setting up a project where agents should use patchloom for file operations and need an AGENTS.md or SKILL.md that describes patchloom's interface.
- **Notable flags:**
  - `--mode cli|mcp|all` (default: `all`): `cli` omits MCP section, `mcp` omits CLI shell examples, `all` includes everything.
  - `--platform linux|windows|all` (default: `all`): `linux` uses heredocs and single-quote syntax, `windows` uses file arguments and double-quote escaping, `all` shows both.
- **Prefer instead:** Nothing; this is the only way to generate the end-user agent documentation.
- **Related:** `completions`, `mcp-server`

<!-- ref:command:init -->
## `init`

- **What it does:** Sets up patchloom in the current project: creates `AGENTS.md` if needed, otherwise appends the rules to an existing agent instructions file, prints shell completion instructions, detects MCP configuration opportunities, and ensures `.gitignore` ignores `.patchloom/` (undo backups). When `.vscode/` or `.cursor/` already exists, it prints ready-to-copy `.vscode/mcp.json` or `.cursor/mcp.json` snippets.
- **Use when:** You just installed patchloom and want a single command to configure a project instead of running `agent-rules`, `completions`, and MCP setup separately.
- **Notable flags:**
  - `-y, --yes`: Skip confirmation prompts and auto-accept all actions (agent rules + shell completions install).
  - With global `--json` / `--jsonl`: agent-rules create/append is auto-accepted without `-y` so agent bootstrap does not report `ok: true` with `agent_rules: skipped` (#1833). Shell completion install still requires `-y` / interactive confirm.
- **Prefer instead:** `agent-rules` if you only need the rules text, or `completions` if you only need shell completions.
- **Related:** `agent-rules`, `completions`, `mcp-server`, `status`, `undo`

<!-- ref:command:mcp-server -->
## `mcp-server`

- **What it does:** Starts an MCP (Model Context Protocol) server, exposing patchloom operations as structured tool calls. Supports stdio (default) and Streamable HTTP transport (with `--http`). Included by default in all builds.
- **Use when:** An MCP-capable AI agent can call patchloom tools directly via structured tool calls instead of constructing shell commands. Use `--http` for remote agents.
- **Notable flags:**
  - `--log <path>`: Log tool calls to a JSONL file (also settable via `PATCHLOOM_MCP_LOG` env var).
  - `--http`: Use Streamable HTTP transport instead of stdio.
  - `--host <addr>` (default: `127.0.0.1`): Bind address (requires `--http`).
  - `--port <port>` (default: `8080`): Bind port (requires `--http`).
  - `--tls-cert <path>` / `--tls-key <path>`: TLS certificate and key PEM files for HTTPS (requires `--http`; both must be provided together).
- **Failure behavior:** Invalid bind address or TLS config fails startup with `error_kind: "invalid_input"` when surfaced through typed error paths.
- **Prefer instead:** Use the CLI directly when the agent does not support MCP, or when patchloom is invoked from scripts and CI.
- **Related:** `batch`, `tx`

<!-- ref:command:completions -->
## `completions`

- **What it does:** Generates shell completion scripts for bash, zsh, fish, or elvish.
- **Use when:** You are installing Patchloom into an interactive shell and want faster command discovery.
- **Prefer instead:** Nothing, if Patchloom is only used from scripts or ephemeral CI runners.
- **Related:** [installation guide](../getting-started/installation.md)

<!-- ref:command:ast -->
## `ast`

- **What it does:** AST-aware operations on source code (20 languages). Subcommands: `list` (extract symbol definitions), `read` (read a symbol by name), `rename` (rename identifiers, skipping strings/comments), `validate` (syntax validation), `search` (structural queries), `refs` (find references), `deps` (extract imports), `map` (ranked repo map via PageRank), `diff` (structural diff vs git refs), `impact` (transitive impact analysis), `replace` (scoped text replacement within a symbol).
- **Use when:** You need to list, read, rename, validate, search, or analyze symbols with structural awareness (skip strings, comments, and documentation). Especially useful for rename operations where the old name appears inside strings that should not be changed, and for impact analysis before refactoring.
- **Prefer instead:** Use `replace --word-boundary` for quick identifier renames when AST precision is not required. Use a language server (LSP) when cross-file type-aware rename is needed.
- **Related:** [`replace`](#replace), [`search`](#search)

## Command modes

These are meaningful command-specific modes that change how a top-level command behaves, even though they are not separate subcommands.

<!-- ref:search-mode:files-with-matches -->
### `search --files-with-matches`

- **What it does:** Emits only file paths that contain at least one match.
- **Use when:** You need a path list to feed into another tool or command instead of the matching lines themselves.
- **Prefer instead:** Use `search --count` when per-file match totals matter, or plain `search` when the matching lines matter.

<!-- ref:search-mode:count -->
### `search --count`

- **What it does:** Emits match counts per file instead of full matching lines.
- **Use when:** You are auditing prevalence, comparing files, or gating on how many matches remain.
- **Prefer instead:** Use plain `search` when you need the matching text, or `search --files-with-matches` when only file membership matters.

<!-- ref:search-mode:invert-match -->
### `search --invert-match`

- **What it does:** Shows lines that do not match the pattern.
- **Use when:** You are looking for non-conforming lines or excluding content that matches a known pattern.
- **Prefer instead:** Use plain `search` when you want the matching lines themselves.

<!-- ref:search-mode:multiline -->
### `search --multiline`

- **What it does:** Lets regex matches span multiple lines by making `.` match newlines.
- **Use when:** The pattern you care about is inherently block-shaped, such as a function body or multi-line stanza.
- **Prefer instead:** Use plain `search` for line-oriented patterns because it is simpler and easier to reason about.

<!-- ref:search-mode:before-context -->
### `search --before-context`

- **What it does:** Shows N lines before each match but none after (unless combined with `-A`).
- **Use when:** You need to see what precedes a match (function signature before a body, imports before usage) without cluttering output with lines after.
- **Prefer instead:** Use `--context` (`-C`) when symmetric context is fine, or combine `-B` and `-A` for independent before/after counts.

<!-- ref:search-mode:after-context -->
### `search --after-context`

- **What it does:** Shows N lines after each match but none before (unless combined with `-B`).
- **Use when:** You need to see what follows a match (function body after signature, error handling after a call) without lines before.
- **Prefer instead:** Use `--context` (`-C`) when symmetric context is fine, or combine `-B` and `-A` for independent before/after counts.

<!-- ref:search-mode:case-insensitive -->
### `search --case-insensitive`

- **What it does:** Matches regardless of case.
- **Use when:** The target text may appear in inconsistent capitalization across files.
- **Prefer instead:** Use case-sensitive search when exact spelling matters and false positives would be noisy.

<!-- ref:search-mode:assert-count -->
### `search --assert-count`

- **What it does:** Succeeds (exit 0) only if the total match count equals the given number. Exits 2 otherwise. Under `--json`/`--jsonl`, mismatch sets `ok: false`, `status: "changes_detected"`, and `error_kind: "changes_detected"` (same kind as plan/tx `search` assert_count and MCP `search_files`).
- **Use when:** An agent or CI pipeline needs to verify an invariant (e.g. "exactly 18 markers exist") in one call instead of searching and then comparing the count manually.
- **Prefer instead:** Use plain `search --count` when you want to see counts without a pass/fail assertion.

<!-- ref:search-mode:max-results -->
### `search --max-results`

- **What it does:** Caps the detailed `matches` array under `--json` (and line-oriented output) while `match_count` stays the full total. Also caps the `files` list in `--count` and `--files-with-matches` modes (`file_count` stays full). When a list is capped, JSON sets `truncated: true` (omitted when complete). Tx plan search results use the same `truncated` field for content matches.
- **Use when:** Agents need a bounded sample of hits or file paths without discarding the true total for budgeting or pagination (#1798).
- **Prefer instead:** Omit `--max-results` when you need every match line or the full file inventory.

<!-- ref:replace-mode:regex -->
### `replace --regex`

- **What it does:** Treats the pattern as a regex instead of a literal string.
- **Use when:** The change is pattern-based, or capture groups should shape the replacement.
- **Prefer instead:** Use literal replace for fixed text because it is simpler and less error-prone.

<!-- ref:replace-mode:if-exists -->
### `replace --if-exists`

- **What it does:** Returns success when no content matches are found, and soft-skips missing explicit paths (CLI `skipped[]`; plan/batch path ops succeed with zero matches instead of hard `not_found`).
- **Use when:** The replacement is intentionally idempotent and should not fail if the file or pattern is already gone or never present (for example optional config files in a batch).
- **Prefer instead:** Use default replace behavior when a missing match or missing path should be treated as drift or an error.

<!-- ref:replace-mode:nth -->
### `replace --nth`

- **What it does:** Replaces only the Nth occurrence of the target.
- **Use when:** Replacing every occurrence would be too broad and the exact positional match matters.
- **Prefer instead:** Use plain replace when every occurrence should change, or regex when the target can be narrowed semantically.

<!-- ref:replace-mode:insert-before -->
### `replace --insert-before`

- **What it does:** Inserts text before each match instead of replacing it. The matched text is preserved.
- **Use when:** You need to add a line or annotation above an existing anchor without repeating the anchor in the replacement text.
- **Prefer instead:** Use `--new` when the matched text should actually change, not just receive a prefix.

<!-- ref:replace-mode:insert-after -->
### `replace --insert-after`

- **What it does:** Inserts text after each match instead of replacing it. The matched text is preserved.
- **Use when:** You need to append content after an existing anchor, such as adding a comment or tag after a specific line.
- **Prefer instead:** Use `--new` when the matched text should actually change, not just receive a suffix.

<!-- ref:replace-mode:multiline -->
### `replace --multiline`

- **What it does:** Lets regex replacement span multiple lines by making `.` match newlines.
- **Use when:** The target pattern is a multi-line block rather than a single line.
- **Prefer instead:** Use line-oriented replace when the match should stay local and easy to inspect.

<!-- ref:replace-mode:case-insensitive -->
### `replace --case-insensitive`

- **What it does:** Matches regardless of case during replacement.
- **Use when:** The target text appears with inconsistent capitalization and should still be updated uniformly.
- **Prefer instead:** Use case-sensitive replace when exact spelling is part of the safety boundary.

<!-- ref:replace-mode:word-boundary -->
### `replace --word-boundary`

- **What it does:** Wraps the search pattern with `\b` (word boundary) anchors so it only matches as a standalone word. Prevents `SetupFile` from matching inside `BenchSetupFile`. The pattern is auto-escaped for regex metacharacters before anchoring.
- **Use when:** Renaming identifiers where the old name is a substring of other identifiers (e.g. `SetupFile` vs `BenchSetupFile`, `Task` vs `TaskResult`).
- **Prefer instead:** Use AST-aware rename (#647) when you need to skip matches inside strings and comments. Word boundary only prevents partial-word matches, not string/comment matches.

<!-- ref:replace-mode:whole-line -->
### `replace --whole-line`

- **What it does:** Replaces (or deletes) entire lines that contain a match, instead of replacing only the matched span. When combined with `--new ''`, removes matching lines entirely.
- **Use when:** You need to delete lines matching a pattern (dead code, lint suppressions, debug statements) or replace full lines based on a partial match.
- **Prefer instead:** Use regular replace when only the matched text should change while the rest of the line stays intact.

<!-- ref:replace-mode:range -->
### `replace --range`

- **What it does:** Restricts `--whole-line` matching to a line range (e.g. `--range 10:50`). Lines outside the range are not considered for matching. Requires `--whole-line`.
- **Use when:** The pattern matches lines you want to keep in other parts of the file (e.g. removing dead code from implementation but not from tests).
- **Prefer instead:** Omit when the pattern is specific enough to avoid false positives.

<!-- ref:replace-mode:unique -->
### `replace --unique`

- **What it does:** Fails with exit code 5 (AMBIGUOUS) if the pattern matches more than once in any single file. Enforces unambiguous, single-target edits. The check is per-file: matching once in file A and once in file B is allowed.
- **Use when:** You need a guardrail that the replacement targets exactly one location per file (CI scripts, automated pipelines, agent-driven edits where accidental bulk replacement is dangerous).
- **Prefer instead:** Use `--nth` to target a specific occurrence when you know which one you want, or omit when replacing all occurrences is the intent.

<!-- ref:replace-mode:before-context -->
### `replace --before-context`

- **What it does:** Provides context line(s) that must appear before the target for anchor-based disambiguation. When the pattern matches multiple times, the match nearest to this context is selected. Routes through the tx engine fallback chain, which supports fuzzy anchor matching when the exact text is not found.
- **Use when:** The pattern matches multiple times in a file and you need to target one specific occurrence by its surrounding code. Requires explicit file paths (not directory scan).
- **Prefer instead:** Use `--nth` when you know the ordinal position. Use `--unique` when you want to enforce single-match without specifying context.

<!-- ref:replace-mode:after-context -->
### `replace --after-context`

- **What it does:** Provides context line(s) that must appear after the target for anchor-based disambiguation. Same semantics as `--before-context` but anchors on what follows the match instead of what precedes it. Both can be combined for even more precise targeting.
- **Use when:** The pattern matches multiple times and the distinguishing context comes after the match, not before.
- **Prefer instead:** Use `--before-context` when the preceding lines are more distinctive.

<!-- ref:replace-mode:require-change -->
### `replace --require-change` / library `ReplaceOptions.require_change`

- **What it does:** Zero matches become an error (CLI exit 3 / structured `EditErrorKind::NoMatch`) instead of soft success. Softened by `--if-exists` / `if_exists`.
- **Use when:** Agent hosts that treat a missed target as a tool error (fail closed). CLI already fails on no-match by default; the flag is explicit for plan/MCP/library parity.
- **Identity:** When the pattern matches but `new` equals `old`, the match counts and `require_change` is satisfied (CLI exit 0 with an "identical (no file changes)" note). That is not a zero-match failure. With `--json`, the response includes `"identity": true`.
- **JSON `error_kind`:** Soft no-match and `--unique` multi-match failures include `error_kind: "no_matches"` (exit 3) or `error_kind: "ambiguous"` (exit 5), matching tx plan JSON. Success responses omit the field.
- **Prefer instead:** Leave the default when soft no-match is intentional. When both `require_change` and `if_exists` are set, `if_exists` wins.

<!-- ref:replace-mode:command-position -->
### `replace --command-position` / library `ReplaceOptions.command_position`

- **What it does:** Replaces only tokens in shell **command position** (start of line, after `&&` `|` `;` / newlines, after wrappers like `sudo`, `timeout 30`, `nice -n 10`, `setsid`, `unshare`/`nsenter`/`taskset`/`prlimit`/`numactl`/`chrt`/`setpriv`, `runuser -u USER`, `busybox`, `chpst -u app`, `softlimit -m N`, `flock /lock`, `chroot /jail`, `envdir /env`, `setlock /lock`, `xargs`, `eval`, `source`, `env KEY=val`). Does not rewrite arguments (`uv pip`) or longer words (`pipenv`). Literal only. Also available on plan/MCP replace and `batch_replace`.
- **Use when:** Migrating install tooling in shell scripts or agent-generated commands without breaking package names that embed the same substring.
- **Prefer instead:** Ordinary replace or `word_boundary` for identifiers. Cannot combine with `regex`, `case_insensitive`, `word_boundary`, `fuzzy`, `nth`, multiline/whole-line, context anchors, or insert-before/after.

<!-- ref:replace-mode:fuzzy -->
### `replace --fuzzy` / library `ReplaceOptions.fuzzy` / plan `fuzzy`

- **What it does:** When the exact pattern has zero matches, try similarity/anchor fallback (same chain as before/after context). Plan ops and MCP `replace_text` accept `fuzzy: true`. Pure fuzzy (no context) works on disk library, single-path tx, **glob** plan ops, and CLI (including directory roots expanded like ordinary replace).
- **Use when:** Agent edits may have whitespace or small typos but should still land with honest `match_mode` / `match_score` / `matched_text` in library results and CLI/MCP JSON (#1669, #1736). Multi-file CLI replace, plan/tx, and content_edits all roll up worst-case confidence (`fuzzy` > `anchored` > `exact`) so mixed batches never under-report fuzzy. Aggregate `match_score` is the **minimum** fuzzy score across paths/ops (lowest confidence), not the first fuzzy hit.
- **Default safety (#1758):** When exact `old` is **absent**, Similarity/fuzzy **refuses to write** by default (even above `min_fuzzy_score`) and reports the best candidate. Set `--allow-absent-old` / `allow_absent_old` only for deliberate approximate recovery. Anchored matches (explicit context) still apply.
- **Prefer instead:** Exact replace when the target string is known; `ast rename` for code identifiers.

<!-- ref:replace-mode:min-fuzzy-score -->
### `replace --min-fuzzy-score` / library `ReplaceOptions.min_fuzzy_score` / plan `min_fuzzy_score`

- **What it does:** When a fuzzy match is found, reject it if its similarity score is below this floor (`0.0..=1.0`). Exact and anchored matches are unaffected. Available on CLI (`--min-fuzzy-score`), plan/MCP (`min_fuzzy_score`), and `ReplaceOptions` (#1687).
- **Use when:** Agent hosts want fuzzy recovery for small typos but must refuse weak similarity hits (typical floor: `0.80`).
- **Does not mean:** `score >= min_fuzzy_score` alone authorizes a write when exact `old` is absent; that still requires `allow_absent_old` (#1758).
- **Prefer instead:** Exact replace when the target string is known.

<!-- ref:replace-mode:allow-absent-old -->
### `replace --allow-absent-old` / library `ReplaceOptions.allow_absent_old` / plan `allow_absent_old`

- **What it does:** Opt in to historical fuzzy behavior: when exact `old` is not in the file, apply the best Similarity candidate above `min_fuzzy_score` (if any). Default is **false** (fail closed; no write).
- **Use when:** You intentionally want approximate recovery and will verify `matched_text`.
- **Prefer instead:** Leave unset for agent hosts; use exact strings or AST renames.

<!-- ref:create-mode:stdin -->
### `create --stdin`

- **What it does:** Reads the new file content from stdin instead of `--content`.
- **Use when:** Another tool is generating the content, or shell composition is cleaner than embedding the full text in one argument.
- **Prefer instead:** Use `create --content` for short inline content that should stay visible in the command itself.

<!-- ref:create-mode:force -->
### `create --force`

- **What it does:** Overwrites an existing file instead of failing.
- **Use when:** File recreation is intentional and should replace previous contents deterministically.
- **Prefer instead:** Use default create behavior when accidental overwrite would be dangerous.

<!-- ref:patch-mode:file -->
### `patch FILE`

- **What it does:** Reads the unified diff from a file path (positional argument).
- **Use when:** The patch already exists as a saved artifact that should be reviewed, reused, or passed around directly.
- **Prefer instead:** Use `patch --stdin` when another tool is piping the patch text dynamically.

<!-- ref:patch-mode:stdin -->
### `patch --stdin`

- **What it does:** Reads the unified diff from stdin instead of a file argument.
- **Use when:** Another tool is generating or piping the patch text directly.
- **Prefer instead:** Use `patch FILE` when the diff should be stored as a tangible artifact.

<!-- ref:doc-mode:predicate -->
### `doc --predicate`

- **What it does:** Supplies the key-value predicate used by `doc delete-where`. Object arrays use a field key (e.g. `name=react`). Scalar arrays match the element with `.=a`, `_=a`, or `value=a` (`value` is accepted as an agent-friendly alias for element match).
- **Use when:** Array cleanup should target matching objects or scalar values instead of deleting by fixed index or selector path alone.
- **Prefer instead:** Use `doc delete` when one direct selector path can remove the target without predicate filtering.

<!-- ref:doc-mode:stdin -->
### `doc --stdin`

- **What it does:** Reads merge payload content from stdin for `doc merge`.
- **Use when:** The object being merged is generated by another tool or is awkward to express inline.
- **Prefer instead:** Use `doc merge --value` for short, self-contained object literals.

<!-- ref:md-mode:stdin -->
### `md --stdin`

- **What it does:** Reads replacement or inserted markdown content from stdin for the section-editing commands.
- **Use when:** The markdown payload is generated, large, or easier to stream than to quote inline.
- **Prefer instead:** Use `--content` when the inserted text is small and should stay visible in the command.

<!-- ref:tx-mode:plan-stdin -->
### `tx -`

- **What it does:** Reads the transaction plan from stdin instead of a plan file. Defaults to JSON; use `--plan-format` for YAML or TOML.
- **Use when:** The plan is generated on the fly or piped from another tool.
- **Prefer instead:** Use `tx FILE` when the plan should be stored, reviewed, or reused.

<!-- ref:tx-mode:plan-yaml -->
### `tx --plan-format yaml`

- **What it does:** Tells `tx` to parse the plan as YAML (or TOML) instead of JSON. Auto-detected from file extension for plan files; required when piping YAML from stdin.
- **Use when:** The plan is easier to write or generate in YAML/TOML, or when JSON verbosity is friction for inline agent-generated plans.
- **Prefer instead:** Use JSON plans when interoperability or strict schema validation matters more than writability.

## `doc` actions

Use these when the top level `doc` command is right, but you need a specific structured operation.

**Comment preservation:** All `doc` write operations preserve inline comments, section comments, and formatting in YAML and TOML files. The parser edits the concrete syntax tree (CST) directly, so only the changed values are rewritten while surrounding comments and whitespace stay intact. This includes operations that change array length (`append`, `prepend`, `delete-where`), which use text-level splicing to preserve comments on the affected file.

**Multi-document YAML:** Streams with more than one `---` document are modeled as a JSON array (one element per document). Use a document index in the selector (`0.metadata.name`, `[1].spec.ports[0].port`). A bare top-level key on a multi-doc file fails with an actionable type error that points at the index form. Successful writes re-serialize with `---` separators (not as a single YAML sequence), so `kubectl apply -f` style multi-doc files stay valid.

**JSON write summary:** Every doc write success payload under `--json` / `--jsonl` includes `changed` (bool). `doc delete` and `doc delete-where` also include `removed` (usize). Agents should not treat exit 0 alone as "something was deleted"; check `removed` / `changed` for idempotent no-ops. The same fields appear on MCP `doc_delete` / `doc_delete_where` and on `execute_plan` / `tx` reports (plus a `mutations` array with per-op `path`, `op`, `changed`, `removed` for multi-step plans).

<!-- ref:doc-action:get -->
### `doc get`

- **What it does:** Reads the value at a selector path from a JSON, YAML, or TOML file.
- **Use when:** You need one precise value without mutating the document.
- **Prefer instead:** Use `doc flatten` when you are exploring an unfamiliar file and need a broader map of its contents.

<!-- ref:doc-action:has -->
### `doc has`

- **What it does:** Checks whether a selector path exists.
- **Use when:** A script or workflow needs a presence check before choosing a later action.
- **Prefer instead:** Use `doc ensure` when the real goal is to create the value if it is missing.

<!-- ref:doc-action:keys -->
### `doc keys`

- **What it does:** Lists the keys of an object at a selector path.
- **Use when:** You want to inspect the shape of a structured object before choosing an edit.
- **Prefer instead:** Use `doc get` when you already know the exact selector path you want.

<!-- ref:doc-action:len -->
### `doc len`

- **What it does:** Counts items in an array or object.
- **Use when:** You need a quick cardinality check in scripts, CI, or exploratory work.
- **Prefer instead:** Use `doc select` or `doc get` when the actual values matter more than the count.

<!-- ref:doc-action:set -->
### `doc set`

- **What it does:** Sets or creates a value at a selector path.
- **Use when:** One exact selector path should be updated deterministically.
- **Prefer instead:** Use `doc merge` for multi field updates, or `doc ensure` when existing values should be preserved.
- **Leading slash:** A single leading `/` is stripped (JSON Pointer habit). `/feature_flag` sets key `feature_flag`, not a key named `/feature_flag` (#1794). Prefer bare keys in agent prompts.

<!-- ref:doc-action:delete -->
### `doc delete`

- **What it does:** Removes the value at a selector path.
- **Use when:** A selector path or node is obsolete and should disappear cleanly.
- **Idempotency:** When the selector matches nothing, the command exits 0 and does not rewrite the file.
- **JSON summary:** With `--json` / `--jsonl`, success payloads include `changed` (bool) and `removed` (`1` when a value was deleted, `0` on no-match). Exit 0 with `"removed": 0` is expected for idempotent cleanup of a missing key.
- **Prefer instead:** Use `doc delete-where` when the target is a subset of array items instead of one direct selector path.

<!-- ref:doc-action:delete-where -->
### `doc delete-where`

- **What it does:** Deletes array items that match a predicate (`--predicate key=value`). For scalar arrays, use `.=x`, `_=x`, or `value=x`. This is a separate filter from selector predicates used by `doc update`.
- **Use when:** You need to remove selected objects or scalar values from a list without rebuilding the whole array by hand.
- **Idempotency:** When no elements match the predicate, the command exits 0 and does not rewrite the file (same as `doc delete` on a missing key). Use `doc update` when a missing match should be an error.
- **JSON summary:** With `--json` / `--jsonl`, success payloads include `changed` (bool) and `removed` (usize). Exit 0 with `"removed": 0` and `"changed": false` means the predicate matched nothing (idempotent cleanup). A non-zero `removed` means that many array items were deleted.
- **Prefer instead:** Use `doc delete` when one direct selector path can remove the target.

<!-- ref:doc-action:merge -->
### `doc merge`

- **What it does:** Deep merges an object payload into an existing document.
- **Use when:** Several related fields should be added or updated together.
- **Prefer instead:** Use `doc set` when one exact path should change and merge semantics are unnecessary.

<!-- ref:doc-action:append -->
### `doc append`

- **What it does:** Appends a value to an array.
- **Use when:** New items should appear at the end of the list.
- **Prefer instead:** Use `doc prepend` when order or precedence means the new item should come first.

<!-- ref:doc-action:prepend -->
### `doc prepend`

- **What it does:** Inserts a value at the front of an array.
- **Use when:** The new item should win by order, or defaults should be introduced at the front of the list.
- **Prefer instead:** Use `doc append` when simple chronological growth is enough.

<!-- ref:doc-action:select -->
### `doc select`

- **What it does:** Reads only the values that match a selector path or predicate.
- **Use when:** You need a filtered read view of a larger structure.
- **Prefer instead:** Use `doc update` or `doc delete-where` when the end goal is mutation rather than inspection.

<!-- ref:doc-action:update -->
### `doc update`

- **What it does:** Sets the same new value at every location matching a selector. Wildcards (`items[*].enabled`) and selector predicates (`items[name=foo].v`) filter inside the selector string. There is no separate `--where` or `--predicate` flag (unlike `doc delete-where`).
- **Use when:** A broad but uniform change should apply across many selected elements.
- **Prefer instead:** Use `doc set` when the change only targets one path.

<!-- ref:doc-action:move -->
### `doc move`

- **What it does:** Moves or renames a selector path.
- **Use when:** Schema cleanup or path migration should preserve the value while changing the selector path.
- **Prefer instead:** Use `doc set` plus `doc delete` only when the move semantics are not a clean fit.

<!-- ref:doc-action:ensure -->
### `doc ensure`

- **What it does:** Creates a value only if it is currently missing.
- **Use when:** You need idempotent config bootstrapping and must not overwrite existing values.
- **Prefer instead:** Use `doc set` when the desired value should win even if the selector path already exists.

<!-- ref:doc-action:flatten -->
### `doc flatten`

- **What it does:** Lists leaf selector paths and their values.
- **Use when:** You are discovering the shape of an unfamiliar structured file.
- **Prefer instead:** Use `doc get` for one targeted read, or `doc keys` when only the object shape matters.

<!-- ref:doc-action:diff -->
### `doc diff`

- **What it does:** Compares two structured files by their semantic content.
- **Use when:** You care about structural value changes more than raw formatting differences.
- **Prefer instead:** Use `patch` or ordinary diff tooling when the exact textual patch matters.

## `md` actions

Use these when markdown structure matters more than raw text matching.

<!-- ref:md-action:replace-section -->
### `md replace-section`

- **What it does:** Replaces the body of a heading section.
- **Use when:** A section should be treated as authoritative content that can be rewritten in one step.
- **Prefer instead:** Use `md insert-after-heading` when existing section content should stay and you only need to add more text.

<!-- ref:md-action:insert-after-heading -->
### `md insert-after-heading`

- **What it does:** Inserts content **immediately after the heading line** (before any existing body such as tables or paragraphs). It does **not** insert after the full section body.
- **Use when:** You want to add a note, intro, or status line under a heading while keeping the rest of the section body after the insert (for example intro text before an existing table).
- **Prefer instead:** Use `md insert-after-section` when adding a **sibling** `##` section after this section's body. Use `md replace-section` when the whole section should be regenerated.

<!-- ref:md-action:insert-after-section -->
### `md insert-after-section`

- **What it does:** Inserts content after the **full section body** (after the last line of the section, before the next same-or-higher heading). Sibling placement for new sections.
- **Use when:** You want to add a new `## FAQ` (or similar) after `## Config` including Config's existing body.
- **Prefer instead:** Use `md insert-after-heading` only for content under the heading line, not for a new sibling section.

<!-- ref:md-action:insert-before-heading -->
### `md insert-before-heading`

- **What it does:** Inserts content immediately before a heading line.
- **Use when:** You want to add a preface or a new section boundary before an existing heading.
- **Prefer instead:** Use `md insert-after-section` when the addition belongs after the previous section's body.

<!-- ref:md-action:upsert-bullet -->
### `md upsert-bullet`

- **What it does:** Ensures a bullet exists under a heading, without duplicating it.
- **Use when:** Rules, checklists, or recurring notes should be added idempotently.
- **Prefer instead:** Use `md replace-section` when the entire list should be rewritten.

<!-- ref:md-action:dedupe-headings -->
### `md dedupe-headings`

- **What it does:** Removes duplicate headings.
- **Use when:** Generated markdown or hand edited docs have accumulated repeated sections that should collapse to one.
- **Prefer instead:** Use `md lint-agents` when the goal is diagnosis rather than mutation.
- **JSON:** Object with `ok`, `path`, `removed` (heading strings), `applied` (false for preview/check), and `backup_session` after a real apply. JSONL still emits one JSON string per removed heading.

<!-- ref:md-action:lint-agents -->
### `md lint-agents`

- **What it does:** Checks AGENTS style markdown for common problems.
- **Use when:** You want a CI style guard for agent instruction files before they drift into invalid or confusing structure.
- **Prefer instead:** Use `md dedupe-headings` when you already know the file should be auto corrected.

<!-- ref:md-action:table-append -->
### `md table-append`

- **What it does:** Appends a row to the markdown table under a heading.
- **Use when:** A docs table should grow without manually rebuilding its existing rows.
- **Prefer instead:** Use `md replace-section` when the whole table should be regenerated from source data.

<!-- ref:md-action:move-section -->
### `md move-section`

- **What it does:** Moves a heading section to a new position, either within the same file (reorder) or to a different file. The section (heading plus body) is extracted from the source and inserted at the target location. Both files are updated atomically.
- **Use when:** Reorganizing documentation structure by moving sections between files or reordering sections within a file.
- **Prefer instead:** Use `md replace-section` for rewriting content in place, or manual cut and paste when the move involves non-contiguous content.

## `patch` actions

Use these when the change already exists as a unified diff.

<!-- ref:patch-action:check -->
### `patch check`

- **What it does:** Dry-run a unified diff without writing. Per-file status is `would_change` (exit 2) when the patch applies and content would change, `unchanged` when the result equals the current file, or `stale`/`missing`/`error` (exit 5) when the patch cannot apply.
- **Use when:** CI or agents need the same “would change” signal as `patch apply` preview before committing to `--apply`.
- **Prefer instead:** Use `patch apply` when the patch should be written, or `replace` and `doc` when you do not actually need to carry a diff file.
- **JSON:** Includes `applied: false`. Do not treat historical status `clean` as “nothing to do”; that name meant “applies without fuzz” and confused agents.

<!-- ref:patch-action:apply -->
### `patch apply`

- **What it does:** Applies a unified diff. Use `--on-stale merge` to retry with three-way merge when context is stale.
- **Use when:** The desired change is already available as patch text and should be replayed directly.
- **Prefer instead:** Use `replace`, `md`, or `doc` when you would rather describe the desired mutation at a higher level.

<!-- ref:patch-action:merge -->
### `patch merge`

- **What it does:** Three-way merges a unified diff. Conflicts emit `<<<<<<< patchloom (ours)` / `=======` / `>>>>>>> patch (theirs)` markers.
- **Use when:** Patch context is stale but you still want partial replay instead of regenerating the diff.
- **Flags:** `--check` reports `clean`/`merged`/`conflict` per file. Conflicts block `--apply` unless `--allow-conflicts`. Exit **8** (`CONFLICTS`) when conflicts remain.

## `tidy` actions

Use these when newline and whitespace correctness is the main concern.

<!-- ref:tidy-action:check -->
### `tidy check`

- **What it does:** Reports missing final newlines, mixed line endings, and trailing whitespace in text files. Binary and invalid UTF-8 files are skipped.
- **Use when:** You want a non mutating tidy audit for CI or local review.
- **Prefer instead:** Use `tidy fix` when the goal is to normalize the files immediately.

<!-- ref:tidy-action:fix -->
### `tidy fix`

- **What it does:** Applies newline and whitespace normalization to text files. Binary and invalid UTF-8 files are skipped. With no write-policy flags (and without `--respect-editorconfig`), it enables final-newline and trailing-whitespace fixes so it matches the issues bare `tidy check` always reports. Pass explicit flags (or EditorConfig) to narrow the fix set.
- **Use when:** Existing files already need cleanup and the cleanup itself is the task.
- **Prefer instead:** Use write policy flags on another write command when normalization should only apply to files already being touched by that command.

## `tx` reference

`tx` is the place where Patchloom's features compose. Use [Core Concepts](../getting-started/concepts.md) for the canonical explanation of rollback and exit codes, and [examples](https://github.com/patchloom/patchloom/tree/main/examples) for plan templates.

### Plan fields

<!-- ref:tx-field:version -->
### `version`

- **What it does:** Declares the plan schema version. Patchloom rejects plans whose version does not match the version it supports.
- **Use when:** Every plan must include this field. It ensures forward-compatibility safety so an old patchloom build does not silently misinterpret a plan written for a newer schema.
- **Required:** Yes. Plans without a version field are rejected.

<!-- ref:tx-field:cwd -->
### `cwd`

- **What it does:** Sets the base directory used to resolve relative paths inside the plan.
- **Use when:** You need plan operations and lifecycle steps to run from a specific subdirectory under the invocation root.
- **Important:** Relative values resolve from the invocation working directory (`--cwd` or the process cwd), not from the plan file's directory. In MCP mode, `plan.cwd` must be a **relative** path under the server workspace root; it is honored for op path re-rooting. Absolute path strings, `../` escapes, and combining `cwd` with `for_each` are rejected as invalid params (not silently stripped). If the resolved path does not exist or is not a directory, the plan is rejected with PARSE_ERROR (exit 4).
- **Prefer instead:** Use the CLI `--cwd` flag when the directory choice is a caller concern rather than part of the plan itself.

<!-- ref:tx-field:write_policy -->
### `write_policy`

- **What it does:** Applies newline, EOL, and whitespace normalization across all pending writes in the plan.
- **Use when:** Every write in the transaction should share the same normalization policy.
- **Fields:** Supports `ensure_final_newline` (bool), `normalize_eol` (`keep`, `lf`, `crlf`, or `cr`), `trim_trailing_whitespace` (bool), and `collapse_blanks` (bool).
- **Failure behavior:** Invalid `normalize_eol` values exit **1** with `error_kind: "invalid_input"` (not `operation_failed` / 9).
- **Precedence:** Patchloom starts from the invocation's per-file write policy, including CLI flags and any `--respect-editorconfig` values, then overrides only the keys set here.
- **Prefer instead:** Use CLI write flags when one invocation needs defaults, but the plan itself should stay generic.

<!-- ref:tx-field:strict -->
### `strict`

- **What it does:** Rolls back file writes when a format or validation step fails. Defaults to `true` when omitted from the plan.
- **Use when:** Partial writes are unacceptable and post-write failure should behave like a full transaction failure (the default for agent workflows).
- **Prefer instead:** Set `"strict": false` in the plan, `[tx] strict = false` in `.patchloom.toml`, or `patchloom tx plan.json --apply --no-strict` when writes may stay on disk even if later validation reports a problem.

<!-- ref:tx-field:operations -->
### `operations`

- **What it does:** Lists the ordered mutations that make up the transaction.
- **Alias:** `ops` is accepted on deserialize (common agent shorthand). Serialized plans still emit `operations`.
- **Use when:** One logical change spans several steps or several mutation types.
- **Prefer instead:** Use a standalone command when one direct operation is enough.

<!-- ref:tx-field:format -->
### `format`

- **What it does:** Runs shell commands after writes are staged to disk but before validation.
- **Use when:** Generated or edited files should be normalized by tools like `cargo fmt`, `prettier`, or `black` as part of the same workflow.
- **Step fields:** Each entry accepts `cmd` (required shell command) and `timeout` (seconds, default `60`).
- **Failure behavior:** Any non-zero exit or timeout fails the transaction. Error output reports the failing step number, exit status, the lifecycle working directory (`cwd`), and a truncated snippet of the command's stderr when available. With `strict: true`, Patchloom rolls back the staged writes.
- **Prefer instead:** Run formatting outside `tx` when it does not need to participate in the transaction's success criteria.

<!-- ref:tx-field:validate -->
### `validate`

- **What it does:** Runs shell commands that decide whether the transaction should be reported as valid.
- **Use when:** Build, test, or policy checks are part of the definition of success for the change.
- **Step fields:** Each entry accepts `cmd` (required shell command), `required` (bool, default `false`), and `timeout` (seconds, default `60`).
- **Failure behavior:** `required: true` makes the step gate transaction success. `required: false` still reports the validation problem to stderr. Error output reports the failing step number, exit status, the lifecycle working directory (`cwd`), and a truncated snippet of the command's stderr when available.
- **Prefer instead:** Use standalone verification outside `tx` when the mutation and the validation lifecycle should stay separate.

<!-- ref:tx-field:verify -->
### `verify`

- **What it does:** Runs pre/post-operation symbol verification checks to ensure structural safety.
- **Use when:** A refactoring plan must preserve the number of functions, test methods, or other AST symbols.
- **Field value:** Array of check objects. Each is either `{"kind": "function", "attr": "test"}` (symbol count) or `{"check": "unique_names"}` (named check).
- **CLI equivalent:** `--verify="kind=function,attr=test"` (repeatable).
- **Failure behavior:** When a check fails, the transaction rolls back and exits with `VALIDATION_FAILED` (6).
- **Prefer instead:** Omit when the plan only touches configuration files or non-code content.

<!-- ref:tx-field:for_each -->
### `for_each`

- **What it does:** Glob-driven batch expansion. When present, the plan's `operations` are treated as templates and expanded once per matching file. Template variables (`{path}`, `{item}` as an alias for `{path}`, `{dir}`, `{stem}`, `{ext}`, `{name}`) are substituted in all operation fields. A path field that is still a lone `{placeholder}` after expansion is rejected as `invalid_input` (not a later opaque `not_found`).
- **Escape mechanism:** Double the braces to produce a literal brace in the output. `{{path}}` becomes `{path}` (not substituted), `{{stem}}` becomes `{stem}`, etc. Use this when operation values must contain literal brace-wrapped text that should not be treated as template variables.
- **Use when:** The same structural transform (extract tests, add headers, reorder symbols) must be applied to many files matching a glob pattern.
- **Field value:** Object with `glob` (required), `exclude` (optional array of glob patterns), and `filter` (optional, e.g. `has_symbol(tests)`).
- **MCP:** Do not set `for_each` together with `plan.cwd` (rejected). Use workspace-relative `{path}` templates without `cwd`.
- **Failure behavior:** If the glob matches zero files, the plan produces zero operations (success with no changes). If any expanded operation fails, the entire batch rolls back atomically.

### Transaction operations

The operations below are the building blocks inside `operations`.

<!-- ref:tx-op:replace -->
### `replace`

- **What it does:** Runs text replacement inside a transaction.
- **Use when:** A text rewrite needs to share atomic rollback, formatting, or validation with other operations.
- **Requires:** Exactly one of `to`, `insert_before`, or `insert_after`, matching top level `replace`.
- **Regex insert semantics:** In regex mode, `insert_before` and `insert_after` preserve the matched text, they do not insert the raw pattern string.
- **Optional fields:** `case_insensitive` (bool, default false), `multiline` (bool, default false), and `if_exists` (bool, default false) match the top level `replace --case-insensitive`, `--multiline`, and `--if-exists` flags. Library-aligned plan fields: `require_change` (bool, default false; hard-fails the op on zero matches when `if_exists` is false) and `command_position` (bool, default false; shell invocable rewrite).
- **Related:** top level `replace`

<!-- ref:tx-op:doc.set -->
### `doc.set`

- **What it does:** Runs a targeted structured set inside a transaction.
- **Use when:** A precise config update must be bundled atomically with other repo changes.
- **Field naming:** Use `selector` for the path expression in `doc.set`, `doc.delete`, `doc.append`, `doc.prepend`, `doc.update`, `doc.ensure`, and `doc.delete_where`.
- **Related:** top level `doc set`

<!-- ref:tx-op:doc.delete -->
### `doc.delete`

- **What it does:** Removes a structured value inside a transaction.
- **Use when:** Schema cleanup should happen as one step in a larger atomic change.
- **Related:** top level `doc delete`

<!-- ref:tx-op:doc.merge -->
### `doc.merge`

- **What it does:** Deep merges structured content inside a transaction.
- **Use when:** Several related structured fields should change together as part of one plan.
- **Related:** top level `doc merge`

<!-- ref:tx-op:doc.append -->
### `doc.append`

- **What it does:** Appends to an array inside a transaction.
- **Use when:** List growth must stay atomic with other edits in the same plan.
- **Related:** top level `doc append`

<!-- ref:tx-op:doc.prepend -->
### `doc.prepend`

- **What it does:** Prepends to an array inside a transaction.
- **Use when:** Ordered config precedence should change as part of a larger atomic mutation.
- **Related:** top level `doc prepend`

<!-- ref:tx-op:doc.update -->
### `doc.update`

- **What it does:** Updates all matching structured nodes inside a transaction. Matching is via the `selector` field (wildcards and selector predicates), not a separate predicate field.
- **Use when:** A broad structured rewrite should be coupled to other edits and validations.
- **Failure behavior:** When the selector matches nothing, the plan exits 3 (`no_matches`); unlike idempotent `doc.delete`, a miss is an error.
- **Related:** top level `doc update`

<!-- ref:tx-op:doc.move -->
### `doc.move`

- **What it does:** Moves or renames a structured selector path inside a transaction.
- **Use when:** Schema migration must stay atomic with related code or docs edits.
- **Related:** top level `doc move`

<!-- ref:tx-op:doc.ensure -->
### `doc.ensure`

- **What it does:** Adds a structured value only if it is missing, inside a transaction.
- **Use when:** Idempotent bootstrapping should happen together with other plan steps.
- **Related:** top level `doc ensure`

<!-- ref:tx-op:doc.delete_where -->
### `doc.delete_where`

- **What it does:** Deletes array items matching a predicate inside a transaction.
- **Use when:** Targeted list cleanup must be coordinated with other transactional edits.
- **Related:** top level `doc delete-where`

<!-- ref:tx-op:md.replace_section -->
### `md.replace_section`

- **What it does:** Replaces a markdown section inside a transaction.
- **Use when:** Docs regeneration should be part of a larger all or nothing repo change.
- **Failure behavior:** Missing heading exits 3 (`no_matches`) with the heading name in the error.
- **Related:** top level `md replace-section`

<!-- ref:tx-op:md.insert_after_heading -->
### `md.insert_after_heading`

- **What it does:** Inserts markdown content immediately after a heading line (before existing body) inside a transaction.
- **Use when:** A release note or docs annotation under a heading must be added atomically with code or config changes.
- **Related:** top level `md insert-after-heading`; sibling sections: `md.insert_after_section`

<!-- ref:tx-op:md.insert_after_section -->
### `md.insert_after_section`

- **What it does:** Inserts markdown content after the full section body (sibling placement) inside a transaction.
- **Use when:** Adding a new section after an existing section's content as part of a multi-op plan.
- **Related:** top level `md insert-after-section`

<!-- ref:tx-op:md.insert_before_heading -->
### `md.insert_before_heading`

- **What it does:** Inserts markdown content before a heading line inside a transaction.
- **Use when:** Docs structure must change as one step in a broader plan.
- **Related:** top level `md insert-before-heading`

<!-- ref:tx-op:md.upsert_bullet -->
### `md.upsert_bullet`

- **What it does:** Ensures a markdown bullet exists inside a transaction.
- **Use when:** Idempotent docs or checklist updates should stay coupled to other edits.
- **Related:** top level `md upsert-bullet`

<!-- ref:tx-op:md.table_append -->
### `md.table_append`

- **What it does:** Appends a markdown table row inside a transaction.
- **Use when:** Documentation tables should be updated together with the code or metadata they describe.
- **Related:** top level `md table-append`

<!-- ref:tx-op:md.move_section -->
### `md.move_section`

- **What it does:** Moves a markdown section to a new position, optionally to a different file.
- **Use when:** Section reordering or cross-file moves should be atomic with the rest of the plan.
- **Related:** top level `md move-section`

<!-- ref:tx-op:md.dedupe_headings -->
### `md.dedupe_headings`

- **What it does:** Removes duplicate markdown headings inside a transaction.
- **Use when:** Cleanup of generated docs should stay atomic with the rest of the plan.
- **Related:** top level `md dedupe-headings`

<!-- ref:tx-op:md.lint_agents -->
### `md.lint_agents`

- **What it does:** Lints an AGENTS.md file for common problems (duplicate headings, dangerous commands outside code fences, missing final newline) inside a transaction.
- **Use when:** Agent rules validation should be part of a larger plan, e.g., lint before and after markdown edits to confirm no new issues.
- **Related:** top level `md lint-agents`, MCP `md_lint`

<!-- ref:tx-op:tidy.fix -->
### `tidy.fix`

- **What it does:** Applies tidy normalization inside a transaction.
- **Use when:** Text cleanup should be part of the same atomic success criteria as other edits.
- **Related:** top level `tidy fix`

<!-- ref:tx-op:file.append -->
### `file.append`

- **What it does:** Appends content to the end of an existing file inside a transaction. Inserts a newline separator if the file does not end with one.
- **Use when:** Adding content to a file must be atomic with other operations in the same plan. Fails if the file does not exist.
- **Related:** top level `append`

<!-- ref:tx-op:file.prepend -->
### `file.prepend`

- **What it does:** Prepends content to the beginning of an existing file inside a transaction.
- **Use when:** Adding a header, license, or shebang line must be atomic with other operations in the same plan. Fails if the file does not exist.
- **Related:** `file.append`, top level `append`

<!-- ref:tx-op:file.create -->
### `file.create`

- **What it does:** Creates a file inside a transaction.
- **Use when:** New files must appear only if the full plan succeeds.
- **Related:** top level `create`

<!-- ref:tx-op:file.delete -->
### `file.delete`

- **What it does:** Deletes a file inside a transaction.
- **Use when:** File removal should roll back if later format or validation steps fail.
- **Related:** top level `delete`

<!-- ref:tx-op:file.rename -->
### `file.rename`

- **What it does:** Renames (moves) a file inside a transaction.
- **Use when:** File renames should roll back if later format or validation steps fail. More efficient than `read` + `file.create` + `file.delete` as a single operation.
- **Related:** top level `rename`

<!-- ref:tx-op:search -->
### `search`

- **What it does:** Searches a file for a pattern inside a transaction and includes match results in the JSON output without writing anything.
- **Use when:** An agent needs to locate patterns before replacing them in the same plan, enabling locate-then-edit in a single call.
- **Optional fields:** `literal`, `regex`, `case_insensitive`, `multiline`, `invert_match`, `context`/`before_context`/`after_context`, `globs`, `exclude_patterns`, `custom_ignore_filenames` (for agent/tool ignore layering), `max_results`, `assert_count`. These provide full parity with the top-level `search` command and library `SearchOptions`.
- **Related:** top level `search`

<!-- ref:tx-op:read -->
### `read`

- **What it does:** Reads a file inside a transaction and includes its content in the JSON output without writing anything. The JSON read result carries the same line metadata as top level `read` (`start_line`, `end_line`, `total_lines`), and when no line range is requested it preserves the raw file content exactly.
- **Use when:** An agent needs to inspect file content before or after other operations in the same plan, enabling "understand then edit" in a single call.
- **Related:** top level `read`

<!-- ref:tx-op:patch.apply -->
### `patch.apply`

- **What it does:** Applies a unified diff inside a transaction. Supports `on_stale: "merge"` for three-way merge when the on-disk file diverged from the patch base, and `allow_conflicts: true` to write conflict markers instead of failing during staging.
- **Use when:** Patch replay needs to compose with earlier in-plan edits and share the same rollback or validation behavior.
- **Failure behavior:** Merge conflicts without `allow_conflicts` exit **8** (`CONFLICTS`) with `error_kind: "conflicts"` under `--json` (not generic `operation_failed` / 9). Stale context without merge exits **5** (`ambiguous`).
- **Related:** top level `patch apply`, `patch merge`

<!-- ref:tx-op:ast.rename -->
### `ast.rename`

- **What it does:** Renames all occurrences of an identifier within a file using tree-sitter AST awareness, skipping strings, comments, and documentation. References inside the renamed symbol and callers are updated atomically. Fields are `path`, `old`, and `new` (same names as `replace` / `ast.replace`). CLI: `ast rename <path> --old <OLD> --new <NEW>`.
- **Use when:** You need a precise identifier rename that respects language semantics (e.g., renaming `old_fn` to `new_fn` without touching the string `"old_fn"` in a log message).
- **Failure behavior:** Missing identifier exits 3 (`no_matches`) with the old name in the error (plan and CLI).
- **Related:** `replace` (text-level), `ast replace`

<!-- ref:tx-op:ast.replace -->
### `ast.replace`

- **What it does:** Performs a scoped text replacement within a single symbol's body. Only the text inside the named symbol is searched and modified; the rest of the file is untouched.
- **Use when:** You need to change a value, string, or expression inside a specific function or struct without affecting identically-named text in other symbols.
- **Failure behavior:** Missing symbol exits **3** (`no_matches`) with `error_kind: "no_matches"`.
- **Related:** `replace` (file-level), `ast.rename` (identifier rename)

<!-- ref:tx-op:ast.rewrite_signature -->
### `ast.rewrite_signature`

- **What it does:** Rewrites a function signature using tree-sitter. Structured fields `visibility`, `parameters`, and `return_type` map to [`FunctionSigEdit`](https://docs.rs/patchloom); optional `new_signature` replaces the whole signature span. Field `old` (alias `name`) is the function name. Library: `api::ast_rewrite_signature`. MCP: `ast_rewrite_signature`.
- **Body gap:** High-level paths accept a logical `new_signature` without trailing whitespace and preserve the original gap before `{` (or insert a conventional space if the original was already glued). Trait/extern forms ending in `;` do not get a spurious space. See #1503 / `splice_function_signature`.
- **Use when:** Changing parameter lists, visibility, or return types without a brittle line scan (LLM agent hosts and embedders).
- **Failure behavior:** Missing function name exits 3 (`no_matches`) with the function name in the error; JSON plans report `error_kind: "no_matches"`.
- **Related:** `ast.replace`, `ast.rename`

<!-- ref:tx-op:ast.insert -->
### `ast.insert`

- **What it does:** Inserts new source code at a position relative to an existing symbol (before, after, inside-start, inside-end). Handles indentation matching and blank-line separation.
- **Use when:** You need to add a new function, field, or statement adjacent to or inside an existing symbol without manually computing line numbers.
- **Failure behavior:** Missing container/adjacent symbol exits **3** (`no_matches`) with `error_kind: "no_matches"`.
- **Related:** `ast.wrap`, `ast.group`

<!-- ref:tx-op:ast.wrap -->
### `ast.wrap`

- **What it does:** Wraps an existing symbol with a prefix and suffix, re-indenting the original body. Commonly used for wrapping a function in an `impl` block, a `mod` block, or an `if` guard.
- **Use when:** You need to add structural nesting around an existing symbol (e.g., wrapping free functions in an `impl`, adding `#[cfg(test)]` module wrappers).
- **Failure behavior:** Missing symbol or empty symbols list exits **3** / **1** (`no_matches` / `invalid_input`). Bad line-range numbers use `invalid_input`.
- **Related:** `ast.insert`, `ast.group`

<!-- ref:tx-op:ast.imports -->
### `ast.imports`

- **What it does:** Adds or removes import statements from a file. Supports `add` and `remove` actions with deduplication. Language-aware: handles `use` (Rust), `import` (Python/JS/TS/Go/Java), `#include` (C/C++).
- **Use when:** You need to manage imports programmatically after moving symbols, adding new dependencies, or cleaning up unused imports.
- **Related:** `ast.move`, `ast.extract_to_file`

<!-- ref:tx-op:ast.reorder -->
### `ast.reorder`

- **What it does:** Reorders top-level symbols (or symbols inside a scope) according to a strategy: `alphabetical`, `reverse`, `kind-first` (types before functions), or a custom ordered list of names. Preserves attached doc comments and attributes.
- **Use when:** You want to enforce a consistent declaration order (e.g., alphabetical functions, types-first convention) or manually arrange symbols to match a specification.
- **Failure behavior:** Missing container symbol exits **3** (`no_matches`). Malformed custom order items exit **1** with `invalid_input`.
- **Related:** `ast.group`, `ast.move`

<!-- ref:tx-op:ast.group -->
### `ast.group`

- **What it does:** Moves one or more symbols into a new or existing module block within the same file. Supports a preamble (e.g., `use super::*;`) and configurable placement (first-symbol position, end of file, or after a specific symbol).
- **Use when:** You want to organize related symbols into a `mod tests { ... }` block or group utility functions into a sub-module without extracting to a separate file.
- **Failure behavior:** Missing symbols exit **3** (`no_matches`) with `error_kind: "no_matches"`.
- **Related:** `ast.extract_to_file`, `ast.move`, `ast.reorder`

<!-- ref:tx-op:ast.move -->
### `ast.move`

- **What it does:** Moves symbols from one file to another, removing them from the source and inserting at a specified position in the target. Supports creating the target file with an optional prepend. Preserves attached doc comments and attributes.
- **Use when:** You need to relocate functions, structs, or constants between files during a refactoring (e.g., moving helpers from `lib.rs` to `utils.rs`).
- **Failure behavior:** Missing source or target anchor symbols exit **3** (`no_matches`) with `error_kind: "no_matches"`.
- **Related:** `ast.extract_to_file`, `ast.group`, `ast.imports`

<!-- ref:tx-op:ast.extract_to_file -->
### `ast.extract_to_file`

- **What it does:** Extracts a single symbol from a source file into a new target file. For module blocks, it can unwrap the module wrapper and un-indent the body. Leaves an optional replacement text (e.g., `mod tests;`) in the source. Supports a prepend for the target (e.g., `use super::*;`).
- **Use when:** You want to extract a test module, a large struct, or a helper block into its own file while leaving a `mod` declaration behind.
- **Failure behavior:** Missing symbol exits **3** (`no_matches`) with `error_kind: "no_matches"`. Existing target without force exits **1** with `error_kind: "already_exists"`.
- **Related:** `ast.split`, `ast.move`, `ast.imports`

<!-- ref:tx-op:ast.split -->
### `ast.split`

- **What it does:** Splits a file into multiple target files by distributing symbols. Each target specifies which symbols it receives and an optional prepend. Symbols not assigned to any target stay in the source (controlled by `keep_in_source`). Supports `source_suffix` and `source_prefix` for adding `mod` declarations. Enforces exhaustive accounting by default.
- **Use when:** A file has grown too large and you want to distribute its symbols across several new files in one atomic operation, with `mod` re-exports generated automatically.
- **Failure behavior:** Duplicate or unaccounted symbols exit **1** with `error_kind: "invalid_input"`.
- **Related:** `ast.extract_to_file`, `ast.move`, `ast.group`

## Library API

- **What it does:** Use patchloom as a Rust library (`default-features = false`, enable `ast`/`mcp` as needed). High level entry points in `patchloom::api` (search, replace_text, etc), plus `execute_plan`, `make_plan`, `PathGuard` for containment, and full plan types for tx. All public types are `Send + Sync`.
- **Use when:** Embedding in LLM coding agents, custom tools, or tests without CLI spawn overhead. See `cargo doc --no-default-features --features ast --open`.
- **Notable:** `search_directory(root, pattern, opts)` for parallel content search with globs/context (library equivalent of CLI search). Error paths and guards documented in api.rs.
- **Related:** README "As a library", `src/api.rs`, `src/lib.rs` docs, examples/README.md entry for search_directory.

### Embedder surfaces (LLM agent hosts)

| Need | API |
|------|-----|
| Fail-closed text replace | `ReplaceOptions.require_change` + `edit_error_kind` / `classify_error` |
| Non-`anyhow` error kinds | `classify_error(&dyn Error)` / `classify_error_ref` (#1659); `EditErrorKind::FormatFailed` for post-write hooks |
| Shell token rename | `ReplaceOptions.command_position` / `ContentEdit::Replace` (#1666) |
| Scoped symbol replace (literal/regex) | `ast_replace_in_symbol` + `AstReplaceInSymbolOptions.regex` (#1658) |
| Project symbol discovery + multi-file rename | `find_files_with_symbol` then `ast_rename_batch` (#1664); one-shot `ast_rename_project` (#1689) |
| Match honesty (fuzzy confidence) | `EditResult` / `ContentEditsResult` `match_mode` / `match_score` (#1662); CLI/MCP JSON (#1669); plan/tx `TxChange` + aggregate mode/score/`match_count` from engine meta (#1674) |
| Reject weak fuzzy matches | CLI `--min-fuzzy-score` / `ReplaceOptions.min_fuzzy_score` / plan `min_fuzzy_score` / MCP `min_fuzzy_score` (#1687); range `0.0..=1.0` |
| Apply session id for surgical undo | `EditResult.backup_session` after Apply (#1686); pair with `restore_path_from_session` |
| Nested monorepo backup listing | `backup::list_sessions_under` + `ListSessionsOptions` (#1688) |
| In-memory multi-op with real diff headers | `apply_content_edits_with_label` (#1665) |
| Surgical undo one path | `backup::restore_path_from_session(root, ts, path)` (#1660) |
| Post-Apply format/lint + optional revert | `run_post_write_validation` / `PostWriteHooks` (#1663); also `ReplaceOptions.post_write`, `WritePolicyOptions.post_write`, `AstRenameBatchOptions.post_write` (#1690). Revert uses the file parent as backup root even when hooks cwd differs |
| Signature rewrite complete in one write | `ast_rewrite_signature` body-gap invariant (#1661) |

### Shell command-position for embedders

When rewriting package managers or CLI tools in scripts (`pip` → `uv`, `wget` → `curl`), set `command_position: true` so only invocable tokens change:

```text
# before
sudo pip install foo
uv pip list
pipenv run test

# after (command_position)
sudo uv install foo
uv pip list          # argument pip kept
pipenv run test      # longer token kept
```

Cannot combine with `regex`, `whole_line`, `multiline`, `nth`, insert-before/after, `fuzzy`, or context anchors (typed `InvalidInput`). Prefer this over `word_boundary` for shell files.
