# Patchloom 0.22.0

Constrained freeform fragment apply for Morph-class snippets, a smaller
MCP tool pack for compact agents, and stronger library refuse defaults
for over-wide fuzzy replaces. Seventeen commits since 0.21.0.

## Highlights

Agents can place freeform fragments with Morph-style lazy markers when
they still know a unique anchor (`--after`, `--before`, or `--old`).
Markers are stripped; there is no cloud model merge and no anchor-less
apply. Hosts that want fewer MCP tools at handshake set
`PATCHLOOM_MCP_SURFACE=core` (10 tools). `ReplaceOptions::for_agent`
now turns on automatic refuse for over-wide fuzzy matches, and plan/tx
multi-path JSON reports the widest matched span with a min score so
hosts can refuse before write.

## New features

- **`apply-fragment` / plan `apply.fragment` / MCP `apply_fragment`.**
  Apply a freeform fragment at a required unique anchor. Morph-style
  `// ... existing code ...` marker lines are stripped from the
  fragment. Exactly one of `--after`, `--before`, or `--old` is
  required. Unique match is the default; `--allow-non-unique` opts out.
  Surfaces: CLI, plan/tx, MCP. Prefer structured `doc`/`md`/`ast`/
  `replace` when the edit is exact; use this when the agent has a
  snippet plus a known nearby anchor. (#2021, #2022, #2026, #2027)

  ```bash
  patchloom apply-fragment src/lib.rs \
    --after 'fn foo() {' \
    --fragment '// ... existing code ...
  let x = 1;' --apply
  ```

- **`PATCHLOOM_MCP_SURFACE=core|full`.** Unset or `full` registers the
  full inventory (default). `core` registers 10 tools only (`read_file`,
  `search_files`, `replace_text`, `batch_replace`, `doc_get`, `doc_set`,
  `doc_query`, `md_replace_section`, `execute_plan`, `server_info`) with
  handshake instructions that name only those tools. Invalid values fail
  closed at server start. (#2015, #2016, #2017)

- **`for_agent` auto-refuse and content-edit honesty.**
  `ReplaceOptions::for_agent()` sets `refuse_suspicious_fuzzy: true`.
  Over-wide fuzzy matches become `EditErrorKind::FuzzySpanSuspicious`
  (`error_kind: "fuzzy_span_suspicious"`). Multi-replace results include
  per-op `op_honesty` (`ContentEditHonesty`) with matched text and
  score. (#2010)

- **Plan/tx multi-path widest `matched_text` + file span refuse.**
  Multi-path plan/tx top-level `matched_text` is the widest span (not
  first non-null); score is the worst (min). Hosts that refuse on span
  should still pair via `changes[]` when paths differ.
  `apply_content_edits_to_file_with_span_policy` refuses before write
  when any fuzzy op is suspicious under `FuzzySpanPolicy`. (#2012)

- **Embedder host checklist.** Minimal ordered checklist for library
  hosts: dual-path peels, multi-op honesty, fuzzy span refuse. (#2013)

## Bug fixes

- **apply-fragment marker safety and exit codes.** Marker stripping and
  fail-closed exit codes for missing/ambiguous anchors are locked with
  unit and multi-surface tests. (#2022, #2026)

- **Ambiguous-match hints name apply-fragment.** When a pattern matches
  multiple times, messages mention apply-fragment `--allow-non-unique`
  as well as replace `--nth`. (#2026)

- **Scorecard packaging hygiene.** MCPB pack no longer shells out to
  npm; release wait drops curl|python npm polling. Code-scanning alerts
  #48–#51 cleared. (#2023, #2024, #2025)

## Numbers

| Metric | 0.21.0 | 0.22.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,948 | 3,998 |
| Unit-ish (`src/`) | 2,669 | 2,706 |
| Commands | 23 | 24 |

## Upgrading

- From crates: `patchloom = "0.22"`
- From npm: `npx patchloom@0.22` or pin `patchloom@0.22.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.21.0
- **CLI / agents:** new command `apply-fragment` (also plan op
  `apply.fragment`, MCP `apply_fragment`). Decision table and Morph
  matrix: agent-rules / `PATCHLOOM.md`, `docs/plans/morph-gap-matrix.md`.
- **MCP hosts:** optional `PATCHLOOM_MCP_SURFACE=core` for a smaller
  tool list; default remains full inventory.
- **Library hosts using `for_agent`:** over-wide fuzzy replaces now
  return `FuzzySpanSuspicious` by default. Opt out with
  `refuse_suspicious_fuzzy: false` on a custom `ReplaceOptions` if you
  still call `fuzzy_span_suspicious` yourself. Prefer per-op
  `op_honesty` / `changes[]` for multi-edit refuse, not only rollup
  `matched_text`. Keep a wildcard arm: `EditErrorKind` and
  `FuzzySpanPolicy` are `#[non_exhaustive]`.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.21.0...patchloom-v0.22.0
