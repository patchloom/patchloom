# Patchloom 0.20.0

Clearer non-text load results and a shared library replace preset for
agent hosts, with CLI and MCP paths that report the same kinds. Ten
commits since 0.19.0.

## Highlights

Reading a binary or invalid UTF-8 file on a sole-path write or read no
longer looks like a generic failure or a soft "no matches" miss.
Library hosts get `EditErrorKind::Binary` and `InvalidEncoding`, CLI
`--json` sets `error_kind` to `binary` or `invalid_encoding`, and bool
helpers (`api::is_binary`, `api::is_invalid_encoding`,
`api::is_load_text_strict_fail`) make recovery straightforward.

Rust hosts that run a primary replace and a fuzzy fallback can share
one policy via `ReplaceOptions::for_agent()`: unique multi-match,
require a real change, fuzzy with a 0.90 floor, and refuse when exact
`old` is absent unless the host opts into `allow_absent_old`.

Unique multi-match also peels as `AmbiguousTarget` with
`api::is_ambiguous`. Force create can replace a non-text prior file
instead of getting stuck on the old content load.

## New features

- **`EditErrorKind::Binary` and `InvalidEncoding`.** Sole-path NUL
  binary and invalid UTF-8 loads are distinct from `InvalidInput` and
  pattern `no_matches`. Appended after existing variants so published
  discriminants stay stable. (#1969)
- **Bool peels and load helper.** `api::is_binary`,
  `api::is_invalid_encoding`, and `api::is_load_text_strict_fail`
  (true for binary, encoding, or related invalid_input load refuses).
  (#1969, #1973)
- **`ReplaceOptions::for_agent` and `AGENT_MIN_FUZZY_SCORE` (0.90).**
  Shared preset for primary and fallback replace paths in embedders.
  `allow_absent_old` stays false by default. (#1971)
- **`api::is_ambiguous`.** Unique multi-match peels cleanly for hosts
  that need a dedicated check. (#1967)
- **Force create over non-text prior.** Soft-loads a binary or invalid
  UTF-8 existing file when force is set so overwrite can proceed.
  (#1969)

## Bug fixes

- **CLI remappers keep Binary and InvalidEncoding.** Replace, batch,
  tx, explain, md, AST, and related paths no longer collapse those
  loads into `operation_failed` or generic invalid input. (#1970,
  #1972, #1974)
- **Shared `load_text_strict` path for sole files.** One strict text
  load layer for agent-facing sole targets; md and AST emit the same
  kinds as replace and read. (#1972, #1974)
- **Windows validate flake timeout.** Validate-step timeout budget
  raised for slow CI runners so format/validate plans do not flake.
  (#1972)
- **Embedder smoke locks host peels.** Pre-release smoke checks sole
  binary and invalid UTF-8 JSON kinds alongside already_exists,
  not_found, and guard_rejected. (#1975)

## Tests

- Integration coverage for md and AST sole-path binary and
  invalid_encoding peels. (#1975)
- Test names and comments aligned with `binary` / `invalid_encoding`
  kinds (no longer labeled as only `invalid_input`). (#1976)

## Numbers

| Metric | 0.19.0 | 0.20.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,932 | 3,945 |
| Unit-ish (`src/`) | 2,656 | 2,666 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.20"`
- From npm: `npx patchloom@0.20` or pin `patchloom@0.20.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.19.0
- **Library hosts:** match `EditErrorKind::Binary` and
  `InvalidEncoding` (or `api::is_binary` / `is_invalid_encoding` /
  `is_load_text_strict_fail`) before treating a load failure as
  generic `InvalidInput` or retrying as a pattern miss. Prefer
  `ReplaceOptions::for_agent()` for agent primary and fallback
  replace; set `allow_absent_old: true` only when approximate
  recovery is intentional. Keep a wildcard arm: `EditErrorKind` is
  `#[non_exhaustive]`.
- **Agents:** sole binary and bad UTF-8 files report
  `error_kind: binary` or `invalid_encoding` (exit 1). Do not treat
  those as `no_matches`. Multi-match under unique policy peels as
  `ambiguous` / `is_ambiguous`.
