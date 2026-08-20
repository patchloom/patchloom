# Patchloom 0.29.0

Library hosts can expand plan `for_each` and list patch destinations without
copying those parsers. Search can list files that do not contain a pattern.

## Highlights

`execute_plan` expands `for_each` when you enable only the `files` feature.
With a PathGuard, plan format and validate commands that contain redirects
or substitutions are refused before they run.

Patch apply honors git `copy from` / `copy to` (new dest, source kept) and
exports dest-list helpers so hosts do not split `diff --git` on spaces.
`search --files-without-match` (`-L`) lists files with no match.

## New features

- **Plan `for_each` on the `files` feature.** Call `api::expand_for_each`
  before walking declared paths, or use `execute_plan`, which expands
  before PathGuard. A glob that matches nothing is `no_matches`. A glob
  that cannot be parsed, and a `filter` other than `has_symbol(NAME)`,
  are `invalid_input`. Do not combine `plan.cwd` with `for_each` ([#2174](https://github.com/patchloom/patchloom/pull/2174)).

- **Lifecycle shell preflight.** `api::lifecycle_cmds` plus
  `api::refuse_lifecycle_shell_metas` reject redirects, pipelines, and
  substitutions. `execute_plan` with a PathGuard does the same before
  commit. MCP still strips format and validate. Commands such as `true`,
  `cargo fmt`, and `rustfmt` with no metas still run ([#2174](https://github.com/patchloom/patchloom/pull/2174)).

- **Patch dest helpers and git copy.** `api::unquote_git_c_string`,
  `parse_diff_file_path`, `parse_diff_git_paths`, and
  `patch_declared_paths` list destinations, including quoted paths with
  spaces and octal C-escapes. Apply copies `copy from` / `copy to`
  dests and keeps the source. An empty new-file dest is written and
  reported as changed. Unsupported git-meta (binary payload, mode-only)
  is `invalid_input` and still appears in the dest list ([#2178](https://github.com/patchloom/patchloom/pull/2178)).

- **`search --files-without-match` (`-L`).** Prints paths that contain
  no match (same idea as grep `-L`). Combining with `--files-with-matches`
  or `--count` is `invalid_input`. When every scanned file contains the
  pattern, CLI `--json` and MCP `search_files` return
  `error_kind: no_matches` and the text
  `no files without matches for 'PATTERN' in SCOPE` ([#2195](https://github.com/patchloom/patchloom/pull/2195), [#2210](https://github.com/patchloom/patchloom/pull/2210)).

## Bug fixes

- **Insert and apply-fragment dropped indent or added a blank line.**
  `--before` / `--after` and apply-fragment copy the anchor line indent.
  A bare fragment lands on its own indented line. A fragment that already
  ends in a newline no longer inserts an extra blank line after
  `--after` ([#2188](https://github.com/patchloom/patchloom/pull/2188), [#2202](https://github.com/patchloom/patchloom/pull/2202), [#2204](https://github.com/patchloom/patchloom/pull/2204), [#2209](https://github.com/patchloom/patchloom/pull/2209)).

- **Patch dest-exists and empty creates reported the wrong status.**
  Copy or rename onto an existing dest is `already_exists` with
  `applied: false`. Empty creates appear in apply JSON. CLI `patch check`
  uses the same dest-clobber message as the library ([#2179](https://github.com/patchloom/patchloom/pull/2179), [#2181](https://github.com/patchloom/patchloom/pull/2181)).

- **`for_each` glob errors looked like plan parse failures.** Invalid
  include or exclude globs, and an unsupported `filter`, return
  `error_kind: invalid_input` (exit 1), not `parse_error` (exit 4) ([#2181](https://github.com/patchloom/patchloom/pull/2181), [#2182](https://github.com/patchloom/patchloom/pull/2182)).

- **Backup sessions collided across processes and listed in the wrong order.**
  Session ids include the process id so parallel applies in the same
  project do not restore the wrong bytes. `undo` list and default-latest
  sort by recency (timestamp, then mtime, then sequence), not by
  filename string order ([#2214](https://github.com/patchloom/patchloom/pull/2214), [#2215](https://github.com/patchloom/patchloom/pull/2215)).

- **`ast validate --json` left `errors[].text` empty on missing tokens.**
  Zero-width tree-sitter errors now report `missing )` or
  `invalid <kind>` ([#2183](https://github.com/patchloom/patchloom/pull/2183)).

- **`ast search --pattern` matched other functions with the same shape.**
  Literal tokens in the pattern must match (a search for
  `fn compute() {}` no longer also hits `fn other()`) ([#2186](https://github.com/patchloom/patchloom/pull/2186)).

- **`agent-rules --surface core` ignored `--mode`.** `--mode cli` and
  `--mode mcp` now change the core pack text the same way they do for
  the full surface ([#2193](https://github.com/patchloom/patchloom/pull/2193)).

- **Batch `creat` had no suggestion.** A typo of `create` now points at
  `file.create` ([#2190](https://github.com/patchloom/patchloom/pull/2190)).

- **`search --unique` told agents to try `--quiet`.** The error now
  points at `replace --unique` ([#2198](https://github.com/patchloom/patchloom/pull/2198)).

- **`doc set` with selector `.` did not mean the document root.** `.`
  is the whole document again ([#2201](https://github.com/patchloom/patchloom/pull/2201)).

- **Prepend `--help` advertised a `'\\n'` escape that is not implemented.**
  Help and append/prepend docs describe the real line-separator behavior ([#2194](https://github.com/patchloom/patchloom/pull/2194), [#2203](https://github.com/patchloom/patchloom/pull/2203)).

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.28.1 -> 0.29.0 |
| Focus | Library for_each and patch dests, search `-L`, insert indent |
| Tests | 4300+ (rounded badge) |

## Upgrading

- **Library hosts (`ast,files`, no `cli`):** call `api::expand_for_each`
  (or `execute_plan`) so glob dests reach PathGuard. Preflight
  format/validate with `lifecycle_cmds` and
  `refuse_lifecycle_shell_metas`, or pass `Some(&guard)` to
  `execute_plan`. Use `patch_declared_paths` / `parse_diff_git_paths`
  for dest denylists instead of splitting `diff --git` on spaces.
- **Agents:** `search -L` / MCP `files_without_match` all-hits is still
  `no_matches`, but the message is `no files without matches for
  'PATTERN' in SCOPE`. Do not treat that as a content miss. Dest-exists
  on patch copy/rename is `already_exists`.
- **CLI insert / apply-fragment:** unindented anchors still copy the
  line indent of the matched line. Fragments that already end in a
  newline do not pick up an extra blank line after `--after`.
- Install from crates.io, npm, Homebrew, or Scoop after the tag ships.
  On Windows, Scoop is recommended; winget may need
  `winget source update` after Microsoft publishes; Chocolatey can lag
  while moderation runs.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.28.1...patchloom-v0.29.0
