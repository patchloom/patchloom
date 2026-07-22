# Patchloom 0.16.0

Line-oriented inserts that stay on their own line, multi-doc `doc merge`
into a chosen document, and one shared text-load policy so binary,
invalid UTF-8, and unreadable paths report the same way on CLI, MCP, tx,
and the library. Twenty commits since 0.15.3.

## Highlights

Inserts no longer glue onto the anchor by default. `insert_before` and
`insert_after` (CLI, plan/tx, library, and fuzzy paths) place the payload
on its own line, honor CR and CRLF as line boundaries, and keep the file's
existing end-of-line style when they add separators. That matches how
agents usually mean "add a line after this match."

Multi-document YAML gets a real merge path: `doc merge --selector 0`
(plan/MCP `selector`, batch `path selector value`) merges into document 0
instead of only refusing a root merge that would wipe the stream. Library
hosts can peel `EditErrorKind::TypeError` for the multi-doc bare-key case
and call public `files::is_binary_file` for the same 8 KiB NUL probe the
CLI uses.

Text loading is one policy end to end. A sole explicit path that is
binary, not valid UTF-8, or unreadable is `error_kind: invalid_input`
(exit 1), not a pattern miss or a vacuous "already tidy." Multi-path
search, replace, and tidy put those co-paths in `refused[]` with
`reason: binary`, `invalid_utf8`, or `unreadable` while still editing
text siblings. A directory walk where every candidate is unreadable is
`invalid_input` ("could not read N path(s)…"), including
`--assert-count 0` and `ast list`, so zero matches is not a green pass
over files you never opened. Sole binary AST targets and MCP/tx path
replace follow the same rules as CLI replace.

## New features

- **Line-oriented `insert_before` / `insert_after` by default.** Payloads
  land on their own line instead of gluing to the anchor; CR/CRLF count as
  whole-line boundaries; insert separators follow the file's EOL. (#1888,
  #1890, #1892)
- **`doc merge --selector` for multi-document roots.** Merge into an
  object document (for example `0` or `[0]`) from CLI, plan, batch, and
  MCP without replacing the whole stream. (#1886)
- **`EditErrorKind::TypeError` and public `is_binary_file`.** Embedders
  can map multi-doc type errors the same way CLI JSON does, and reuse the
  binary probe without reimplementing it. `EditErrorKind` is
  `#[non_exhaustive]` so new kinds can land in minor releases. (#1889,
  #1891)
- **Shared text I/O loaders (`load_text_strict` / SoftSkip).** Sole-path
  and multi-path readers share one binary / invalid UTF-8 / unreadable
  model across search, replace, tidy, read, md, doc, patch, ast, batch,
  and tx. (#1895, #1897, #1900)

## Bug fixes

### Inserts and renames

- **Whole-line insert no longer glues on CRLF files.** Bare
  `insert_after` on a whole line used to produce `alphabeta\r\n` when only
  `\n` counted as a boundary. (#1890)
- **Insert separators no longer force LF into CRLF (or bare-CR) files.**
  (#1892)
- **Batch/tx `file.rename` reports a rename, not delete+create.** JSON
  uses rename semantics so agents can tell a move from a pair of lifecycle
  ops. (#1887)

### Text, binary, and unreadable paths

- **Sole non-text path is `invalid_input` for search, replace, tidy, read,
  md, patch, and ast** (binary and invalid UTF-8), including sole entries
  from file-backed `--files-from`. Not pattern `no_matches`, not vacuous
  clean tidy, not a false heading miss. (#1881, #1893, #1895, #1897,
  #1898, #1901, #1902)
- **MCP/tx path replace refuses a sole binary** the same way CLI does
  (no silent rewrite of NUL-containing UTF-8 bytes). (#1893)
- **Multi-path `refused[]` reasons include `binary`, `invalid_utf8`, and
  `unreadable`** for search, replace, and tidy. Partial success on text
  siblings does not imply every listed path was scanned. (#1901, #1902)
- **Sole unreadable path (for example mode 000) is `invalid_input`**, not
  `no_matches`, for search and replace. (#1903)
- **All-unreadable directory walks are `invalid_input`**, not a quiet
  pattern miss, including `search --assert-count 0` and `ast list`.
  (#1904, #1905)
- **Patch apply on binary / invalid UTF-8 targets reports
  `invalid_input`** (not a misleading ambiguous/STALE kind). (#1898,
  #1901)
- **Sole non-text fails before soft-skip "skipping …" noise** on stderr
  for replace and search. (#1899)

## Numbers

| Metric | 0.15.3 | 0.16.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,787 | 3,862 |
| Unit-ish (`src/`) | 2,574 | 2,616 |
| Integration-ish (`tests/`) | 1,213 | 1,246 |
| CLI commands | 23 | 23 |
| Commits since 0.15.3 |  | 20 |

Counts are `#[test]` / `#[tokio::test]` attributes under `src/` and
`tests/` at the release tag.

## Upgrading

- **Agents and scripts that treated sole binary, invalid UTF-8, or
  unreadable paths as `no_matches` (exit 3)** should switch on
  `error_kind: invalid_input` (exit 1) when the path cannot be loaded as
  text. Multi-path partial runs still succeed on text files and list
  non-text co-paths under `refused[]`.
- **Inserts:** if you relied on substring glue for `insert_before` /
  `insert_after`, re-check those call sites; the default is whole-line
  placement.
- **Library embedders:** match `EditErrorKind` non-exhaustively; handle
  `TypeError` for multi-doc bare keys. Prefer `files::is_binary_file` over
  a local NUL probe.
- **Multi-doc merge:** use `doc merge --selector 0` (or plan/MCP
  `selector`) to merge into document 0; root merge onto an array stream
  still refuses with `type_error`.
