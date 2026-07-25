# Patchloom 0.19.0

Stable error kinds and bool helpers for library hosts (especially create,
rename, and missing-path recovery), plus clearer agent recipes for
line insert and force overwrite. Twelve commits since 0.18.0.

## Highlights

Create and rename destination conflicts are a first-class library kind.
`edit_error_kind` returns `AlreadyExists` (CLI `error_kind: already_exists`)
instead of collapsing into generic `InvalidInput`. Hosts can call
`api::is_already_exists` or `api::error_kind_str` without scraping English.

Missing paths, patch merge conflicts, and check/assert soft fails peel as
`NotFound`, `Conflicts`, and `ChangesDetected`. Matching bool peels cover
the rest of the fine-grained kinds hosts already map in agent JSON:
`is_type_error`, `is_format_failed`, `is_guard_rejected`, `is_invalid_input`,
and `is_no_match`.

New `EditErrorKind` variants stay append-only so published discriminants
do not shift under cargo-semver-checks. Agent-rules and PATCHLOOM document
the peel set and copy-paste insert-after recipes.

## New features

- **`EditErrorKind::AlreadyExists` and `api::is_already_exists`.** Create
  and rename dest-exists peels separately from binary, empty path, and
  directory targets (`InvalidInput`). (#1947, #1950)
- **`api::error_kind_str`.** Returns the same kind strings as CLI JSON
  (`already_exists`, `not_found`, `guard_rejected`, and the rest) for host
  envelopes that mirror agent output. (#1948, #1950)
- **Sibling peels: `NotFound`, `Conflicts`, `ChangesDetected`.** Missing
  path I/O, patch conflict markers, and exit-2 soft failures no longer look
  like generic `OperationFailed`. Bool helpers: `is_not_found`,
  `is_conflicts`, `is_changes_detected`. (#1950, #1958)
- **Full public bool peel set.** Also `is_type_error`, `is_format_failed`,
  `is_guard_rejected`, `is_invalid_input`, and `is_no_match` (JSON kind for
  soft zero matches remains `no_matches`). (#1959)

## Bug fixes

- **Append-only `EditErrorKind` growth.** New variants are added after the
  last published variant so cargo-semver-checks does not see discriminant
  shifts on a minor release. (#1955)
- **Agent docs matched library peels.** PATCHLOOM / agent-rules no longer
  say create dest-exists peels to `InvalidInput` after `AlreadyExists`
  landed. (#1956)
- **Create and rename force hints.** Dest-exists messages point at
  `--force` / force so agents recover without guessing. (#1952)
- **Insert-after recipes for agents.** Copy-paste CLI shapes for
  line-oriented insert (omit `--new`; insert is not replace). (#1954)

## Tests

- Library and API coverage for AlreadyExists, NotFound peels, and
  `edit_error_kind` maps from exit typed errors. (#1950, #1958, #1959)
- Broader Windows and WSL CI: full WSL lib + integration tests, rustfmt
  for format steps, and PowerShell smoke for path/CRLF/contain peels.
  (#1957)

## Numbers

| Metric | 0.18.0 | 0.19.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,928 | 3,932 |
| Unit-ish (`src/`) | 2,652 | 2,656 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.19"`
- From npm: `npx patchloom@0.19` or pin `patchloom@0.19.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.18.0
- **Library hosts:** match on `EditErrorKind::AlreadyExists` (or
  `api::is_already_exists`) for create/rename dest-exists instead of
  scraping `"already exists"` or treating every `InvalidInput` as
  overwrite recovery. Keep a wildcard arm: `EditErrorKind` is
  `#[non_exhaustive]`.
- **Agents:** dest-exists and guard failures remain distinct
  `error_kind` values (`already_exists`, `guard_rejected`); force
  overwrite still requires an explicit force flag.
