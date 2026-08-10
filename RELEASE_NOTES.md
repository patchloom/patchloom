# Patchloom 0.28.0

Empty and whitespace-only paths fail closed with a stable error, and library
hosts can match on `ContainmentError` without breaking when new variants land.

## Highlights

Blank paths no longer resolve as the workspace root. CLI, plan/tx, MCP, and
`PathGuard` reject empty, whitespace-only, and format-character-only paths with
`error_kind: invalid_input` and the message `path must not be empty`. The public
`ContainmentError` enum is `#[non_exhaustive]` so additive variants (including
`EmptyPath`) do not force a major bump every time.

## Breaking changes

- **`ContainmentError` is `#[non_exhaustive]`.** Matching on this public enum
  must include a wildcard arm (`_ => ...`). New variant `EmptyPath` is part of
  this release ([#2152](https://github.com/patchloom/patchloom/pull/2152), [#2153](https://github.com/patchloom/patchloom/pull/2153)).

## Bug fixes

- **Empty path looked like a workspace-root failure.** Paths that were empty,
  spaces-only, or only format characters (ZWSP/BOM) joined to the cwd and
  produced confusing "target is not a file" errors. They now fail early as
  `invalid_input` / `ContainmentError::EmptyPath` across CLI, engine, and MCP
  ([#2150](https://github.com/patchloom/patchloom/pull/2150), [#2152](https://github.com/patchloom/patchloom/pull/2152), [#2155](https://github.com/patchloom/patchloom/pull/2155)).

## Docs

- Windows install guidance: Scoop remains the channel we operate; winget
  (`Patchloom.Patchloom`) tracks GitHub Releases after Microsoft publish;
  Chocolatey often lags. Portable zip layout, PowerShell batch without
  bash heredocs, and MCP `command: patchloom` examples are documented
  ([#2150](https://github.com/patchloom/patchloom/pull/2150), [#2154](https://github.com/patchloom/patchloom/pull/2154)).

- Library embed version snippets stay in sync with `Cargo.toml` on release
  PRs (`x-release-please-version` + integration smoke lock)
  ([#2154](https://github.com/patchloom/patchloom/pull/2154), [#2155](https://github.com/patchloom/patchloom/pull/2155)).

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.27.0 -> 0.28.0 |
| Focus | Blank-path fail-closed, ContainmentError non_exhaustive, install honesty |
| Tests | 4100+ (rounded badge) |

## Upgrading

- **Library hosts matching `ContainmentError`:** add a wildcard arm. Treat
  `EmptyPath` like other invalid paths (do not treat blank as workspace root).
- **Agents / CLI:** blank paths return `error_kind: invalid_input` with
  `path must not be empty`. Prefer a real relative or workspace-absolute path.
- Install from crates.io, npm, Homebrew, or Scoop after the tag ships.
  On Windows, Scoop is recommended; winget may need `winget source update`
  after Microsoft publishes; Chocolatey can lag while moderation runs.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.27.0...patchloom-v0.28.0
