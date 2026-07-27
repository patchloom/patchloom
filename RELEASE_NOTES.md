# Patchloom 0.21.0

Library hosts can refuse over-wide fuzzy matches after replace, and
docs help agents pick the right edit surface. Nine commits since 0.20.0.

## Highlights

After a fuzzy replace, hosts can call `fuzzy_span_suspicious` on the
matched span and score to refuse edits that replaced far more text
than the `old` string, or near-floor scores with a wide span. Defaults
use Unicode character counts and the same 0.90 floor as
`ReplaceOptions::for_agent`.

Agent-facing docs now lead with a decision table (doc vs md vs AST vs
replace vs shell), comparison pages, and MCP directory copy that
positions Patchloom as structured agent edits rather than a generic
filesystem MCP. Registry `server.json` descriptions stay within the
100-character registry limit.

## New features

- **`fuzzy_span_suspicious` and `FuzzySpanPolicy`.** After
  `replace_in_content` / host replace paths, inspect widest
  `matched_text` and worst (min) `match_score` to refuse over-wide
  fuzzy spans. Default policy: refuse when matched length is greater
  than `max(4 × old, old + 40)` characters, or when score is in
  `[0.90, 0.95)` and matched is more than twice `old`. Empty old with
  non-empty match is always suspicious. Policy is `#[non_exhaustive]`.
  (#1982, #1984, #1985, #1986)
- **Agent routing and comparison docs.** Decision table in agent-rules,
  comparisons (filesystem MCP, yq, ast-grep, Morph), embedder host
  case study for `for_agent` + span refuse, community launch draft.
  (#1999)
- **MCP listing hygiene.** Differentiated `server.json` description
  (≤100 chars), Smithery publish blurb alignment, REPO-SETUP post-release
  directory checklist. (#2000, #2001, #1999)

## Bug fixes

- **Host-path span contract locks.** Tests and embedder-smoke cover
  content replace → `fuzzy_span_suspicious`, Unicode length (not only
  bytes), and absolute vs 4× boundary edges. (#1984, #1985, #1986)
- **Embedder peel table complete.** Reference docs list Binary and
  InvalidEncoding peels with the rest of the host table. (#1979)

## Numbers

| Metric | 0.20.0 | 0.21.0 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,945 | 3,949 |
| Unit-ish (`src/`) | 2,666 | 2,670 |
| Commands | 23 | 23 |

## Upgrading

- From crates: `patchloom = "0.21"`
- From npm: `npx patchloom@0.21` or pin `patchloom@0.21.0`
- CLI binary: install from the GitHub Release installers, Homebrew tap,
  Scoop, or Chocolatey as for 0.20.0
- **Library hosts:** after fuzzy replace succeeds, call
  `fuzzy_span_suspicious(old, matched_text, match_score)` (or
  `fuzzy_span_suspicious_with_policy`) and refuse when true. Prefer
  combining with `ReplaceOptions::for_agent()`. Keep a wildcard arm:
  `FuzzySpanPolicy` and `EditErrorKind` are `#[non_exhaustive]`.
- **Agents:** no CLI flag change; use the agent-rules decision table
  and structured tools (doc/md/ast/batch) before shell `sed`/`jq`.
- **MCP Registry:** description for 0.21.0 publishes the shortened
  differentiated blurb (0.20.0 could not be re-published for metadata
  only).

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.20.0...patchloom-v0.21.0
