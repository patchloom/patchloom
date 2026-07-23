# Patchloom 0.18.0

Safer embedder undo discovery, structured PathGuard failures, and cleaner
agent JSON. Seven commits since 0.17.0.

## Highlights

Hosts that implement undo no longer need a private parent walk:
`backup::find_backup_roots(path)` returns project roots that own
`.patchloom/backups`, nearest first.

PathGuard and `--contain` rejections are a first-class kind. Library
`edit_error_kind` returns `GuardRejected`, and CLI `--json` sets
`error_kind: "guard_rejected"` (with `applied: false`). Empty paths and
other usage errors stay `invalid_input`. Agents that only checked
`invalid_input` for sandbox escapes should also accept `guard_rejected`.

File create/delete/rename/append and `ast_rewrite_signature` peel the same
way for exists/dir/binary (`InvalidInput`), missing symbols (`NoMatch`),
and guard failures (`GuardRejected`). Windows PathGuard uses dunce
canonicalize so UNC roots do not break containment. Agent JSON stops
doubling the OS detail when context already embeds it.

## New features

- **`backup::find_backup_roots(path)`.** Ancestor walk for backup roots
  (presence of `.patchloom/backups`, nearest first; uncapped, unlike
  `list_sessions_under` depth caps). (#1934)
- **Structured `GuardRejected` for PathGuard.** Engine, CLI `--contain`,
  plan cwd escapes, and library file/AST writers share
  `EditError::guard_rejected`. CLI JSON `error_kind: guard_rejected`.
  (#1935, #1938)
- **File and AST signature error kinds locked for hosts.** Already-exists,
  directory targets, binary text ops, missing functions, and empty signature
  edits peel without English scraping. (#1935, #1936)

## Bug fixes

- **Windows PathGuard UNC.** dunce canonicalize / simplified paths keep
  contain and backup relative paths valid under `\\?\`. (#1931, #1932)
- **Agent JSON no double OS detail.** When outer context already embeds
  the OS message, the agent envelope keeps one complete string. (#1929)
- **Platform path coverage.** Integration tests for Windows and WSL path
  shapes (md, patch, tx, rename) reduce platform-only regressions. (#1933,
  #1937)

## Numbers

| Metric | 0.17.0 | 0.18.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,875 | 3,922 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.18"`
- From npm: `npx patchloom@0.18` or pin `patchloom@0.18.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.17.0

**Agent / host note:** treat `--contain` failures as
`error_kind: "guard_rejected"`. Prefer `edit_error_kind` /
`classify_error` over English string matching. `find_backup_roots` is the
supported replacement for private parent walks over `.patchloom/backups`.
