# Patchloom 0.18.0

Clearer sandbox and host errors for agents and embedders, a public
backup-root helper for undo UIs, and several agent-facing CLI JSON and
batch-parse improvements. Eleven commits since 0.17.0.

## Highlights

Library hosts no longer need a private parent walk to find undo data:
`backup::find_backup_roots(path)` returns project roots that own
`.patchloom/backups`, nearest first.

PathGuard and CLI `--contain` rejections are a first-class kind. Library
`edit_error_kind` returns `GuardRejected`, and CLI `--json` sets
`error_kind: "guard_rejected"` (with `applied: false`). Empty paths and
other usage errors stay `invalid_input`. Agents that only checked
`invalid_input` for sandbox escapes should also accept `guard_rejected`.

File create, delete, rename, and append (and AST signature rewrites) peel
the same way for already-exists, directory targets, binary text ops
(`InvalidInput`), missing symbols (`NoMatch`), and guard failures
(`GuardRejected`). On Windows, PathGuard uses dunce canonicalize so UNC
roots do not break containment. Agent JSON keeps a single complete OS
detail string when the outer context already embeds it.

## New features

- **`backup::find_backup_roots(path)`.** Walks ancestors for directories that
  contain `.patchloom/backups` and returns them nearest-first (uncapped,
  unlike depth-limited session listing). For hosts that build undo UIs or
  nested monorepo workflows. (#1934, #1938)
- **Structured `GuardRejected` for PathGuard.** Engine, CLI `--contain`,
  plan `cwd` escapes, and library file/AST writers share
  `EditError::guard_rejected`. CLI JSON uses `error_kind: guard_rejected`.
  (#1935, #1938)
- **Stable file and AST signature error kinds for hosts.** Already-exists,
  directory targets, binary text ops, missing functions, and empty
  signature edits peel without scraping English messages. (#1935, #1936)

## Bug fixes

- **`tidy check --json` sets `error_kind: changes_detected` when issues
  exist.** Exit code 2 is unchanged; agents can switch on kind the same
  way as search `--assert-count`. (#1940)
- **Windows PathGuard under UNC.** dunce canonicalize / simplified paths
  keep contain and backup relative paths valid when roots look like
  `\\?\...`. (#1931, #1932)
- **Agent JSON no longer doubles OS detail.** When the outer context
  already includes the OS message, the envelope keeps one complete
  string. (#1929)
- **Did-you-mean noise on multi-file no-match.** Long invented patterns
  no longer suggest short unrelated tokens (for example `nold`). Real
  typos such as `process_requst` → `process_request` still suggest.
  Score floor raised and length skew filtered. (#1941)
- **Batch replace CLI shape hints.** Pasting CLI
  `replace OLD --new NEW path` into a batch line is rejected with an
  explicit `PATH OLD NEW` message (including plan-shaped `--from` /
  `--to`). `batch --help` documents the shape. (#1942, #1943)

## Tests

- Broader platform path coverage for Windows and WSL (md, patch, tx,
  rename). (#1933, #1937)
- Embedder smoke covers fuzzy typo apply, nested monorepo `undo --list`,
  plan `key` alias, and `--contain` → `guard_rejected`. (#1939)

## Numbers

| Metric | 0.17.0 | 0.18.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,875 | 3,928 |
| Unit-ish (`src/`) | 2,624 | 2,652 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.18"`
- From npm: `npx patchloom@0.18` or pin `patchloom@0.18.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.17.0

**Agent / host note:** treat `--contain` and PathGuard failures as
`error_kind: "guard_rejected"`. Prefer `edit_error_kind` /
`classify_error` over English string matching. Use `find_backup_roots`
instead of a private parent walk over `.patchloom/backups`. Batch
replace lines stay `replace PATH OLD NEW` (not CLI `--new`).
