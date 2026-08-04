# Patchloom 0.27.0

Clearer agent recovery after failed writes, and machine-stable hints when a
doc write selector uses a predicate or wildcard.

## Highlights

Library hosts can read a backup session id from a failed apply path so
fail-restore and undo do not scrape English error text. When `doc set`,
`doc ensure`, or `doc delete` is called with a predicate or wildcard
selector, JSON output can include `suggested_op` (`doc.update` or
`doc.delete_where`) so harnesses can retry the right multi-match op without
parsing messages. That mapping also holds when the predicate is an intermediate
parent path, not only a leaf segment.

## New features

- **`api::backup_session_from_error`.** On a failed write after a backup was
  created (for example a post-write `--format` failure), hosts can peel the
  session id from the error for fail-restore. Success paths already expose
  `EditResult.backup_session`; the Err path is now first-class.
  ([#2129](https://github.com/patchloom/patchloom/pull/2129), [#2131](https://github.com/patchloom/patchloom/pull/2131))

- **`suggested_op` on fail-closed write navigation.** Predicate or wildcard
  selectors on single-path doc writes stay `error_kind: invalid_input` and
  may set `suggested_op` to `doc.update` (set/ensure) or `doc.delete_where`
  (delete). Plan/tx and MCP JSON preserve the field. Move has no multi-match
  sibling and omits the field. Intermediate parents use the same mapping as
  leaves.
  ([#2136](https://github.com/patchloom/patchloom/pull/2136), [#2137](https://github.com/patchloom/patchloom/pull/2137), [#2140](https://github.com/patchloom/patchloom/pull/2140))

## Bug fixes

- **Tx JSON could drop `suggested_op` after re-wrap.** Fail-closed predicate
  navigation still surfaces the hint on CLI and library peels after plan
  error packaging. ([#2137](https://github.com/patchloom/patchloom/pull/2137))

- **Intermediate predicate always suggested `doc.update`.** A delete (or
  move) path such as `items[id=a].val` no longer invents an update retry;
  delete suggests `doc.delete_where`, and move omits `suggested_op`.
  ([#2140](https://github.com/patchloom/patchloom/pull/2140))

## Docs

- Agent and reference guidance for `doc set` vs `doc update`, and when to
  branch on `suggested_op`. Intermediate mapping documented on set/delete.
  ([#2135](https://github.com/patchloom/patchloom/pull/2135), [#2143](https://github.com/patchloom/patchloom/pull/2143))

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.26.0 → 0.27.0 |
| Focus | Fail-restore session peel, write-nav `suggested_op`, docs |
| Tests | 4100+ (rounded badge) |

## Upgrading

- **Library hosts:** on apply Err after a backup, use
  `api::backup_session_from_error(&err)` (or the crate re-export) instead of
  parsing Display for a session id.
- **Agents / harnesses:** on `invalid_input` from doc write navigation,
  read `suggested_op` when present. Do not assume every intermediate
  failure means `doc.update`.
- Install from crates.io, npm, Homebrew, or Scoop after the tag ships.
  Winget and Chocolatey may lag. Fresh Homebrew installs are checked in
  release CI; local cellars still need `brew upgrade` when an older
  formula is already installed.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.26.0...patchloom-v0.27.0
