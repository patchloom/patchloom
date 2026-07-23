# Patchloom 0.17.0

Clearer machine-readable errors for agents and scripts, a library merge
selector for multi-document YAML hosts, and load messages that keep the OS
detail without repeating themselves. Fourteen commits since 0.16.0.

## Highlights

When a sole path cannot be read (permissions, missing file, and similar),
`--json` now keeps a single complete message such as
`failed to read path: Permission denied (os error 13)` with
`error_kind: invalid_input` or `not_found`. CLI, plan/tx, and MCP no longer
prefix that message a second time ("failed to read X: failed to read X").

`patch check --json` on multi-file diffs sets top-level `error_kind` when
some targets are stale, missing, or invalid, so callers can switch on kind
instead of parsing English status strings.

Library embedders get `api::doc_merge` with a multi-doc selector (same idea
as CLI `doc merge --selector 0`), plus public re-exports of the strict text
loaders used by the CLI. Official MCP Registry publishing runs after crates
and npm so the registry sees a published package.

## New features

- **Library `doc_merge` with selector.** Hosts can merge into a multi-document
  YAML root by document index (for example `"0"` or `"[0]"`) without going
  through the CLI. Text load helpers (`load_text` / `load_text_strict`) are
  re-exported for the same sole-path policy the CLI uses. (#1913)

## Bug fixes

- **Load errors keep path and OS detail once.** Unreadable sole paths set
  `error_kind: invalid_input` and include `Permission denied` (or the OS
  equivalent). Missing files keep `not_found` with `No such file` in the
  message. Explain, batch, tx, patch, and MCP plan loads no longer double
  the `failed to read` prefix. (#1916, #1918, #1925, #1926)
- **`patch check --json` reports `error_kind` on multi-file failures.**
  Stale context maps to `ambiguous`, missing targets to `not_found`, and
  similar cases to stable kinds with a short top-level `error` summary.
  (#1924)
- **`init` noninteractive skip is explicit.** When agent rules are not
  written without `--yes`, JSON and text say so (including `skipped_use_yes`
  where applicable) instead of looking like a quiet full setup. (#1922,
  #1923)
- **MCP Registry publish order.** Registry publish waits for crates.io and
  npm so the listing sees published packages. (#1908)
- **doc.merge schema blurb.** Catalog and MCP prose mention multi-doc
  selectors so agents do not invent a separate merge shape. (#1917)

## Numbers

| Metric | 0.16.0 | 0.17.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,862 | 3,877 |
| Unit-ish (`src/`) | 2,616 | 2,624 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.17"`
- From npm: `npx patchloom@0.17` or pin `patchloom@0.17.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap, or
  Scoop bucket as for 0.16.0

No intentional breaking CLI or plan schema changes. Agents that only
inspected English error text should prefer `error_kind` and the full
`error` string (which now includes OS detail). Library hosts using
`doc_merge` should pass a multi-doc selector when the root is a document
stream.
