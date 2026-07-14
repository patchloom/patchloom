# Patchloom 0.14.0

Distribution and agent-edit polish: install via Scoop, npm (`npx patchloom`), and a
Chocolatey package path; library hosts get backup sessions, fuzzy floors, nested undo
listing, project-wide AST rename, and post-write hooks; multi-document YAML stays
`kubectl apply -f` friendly; batch replace and markdown sibling inserts close everyday
agent gaps. 29 commits since 0.13.0, with about 69 new tests.

## Highlights

You can install and upgrade Patchloom the way Windows and JS developers already expect
(Scoop bucket + npm OIDC publish), while agent hosts and CLI users get tighter fuzzy
replace (floor, token alignment, similar-target hints) and structured multi-doc YAML
edits that keep `---` separators. Batch lines accept common replace flags without a
full JSON plan; markdown can insert a true sibling section after a full body, not only
under the heading line. Undo messaging and rename labels match what agents actually
see on disk.

## New features

- **More install channels.** Official Scoop bucket (`patchloom/scoop-bucket`) updated on
  release, npm package via Trusted Publishing (`npx patchloom` / `npm i -g patchloom`),
  and Chocolatey package generation on the release path. (#1701, #1711, #1714, #1703,
  #1709, #1712)
- **Agent-host library APIs.** `EditResult.backup_session`, `ReplaceOptions.min_fuzzy_score`,
  recursive `list_sessions_under`, `ast_rename_project`, and `PostWriteHooks` on core Apply
  writers so embedders can undo, reject weak fuzzy hits, rename symbols across a tree, and
  run format/validate after writes without inventing their own glue. (#1692, #1693)
- **CLI replace confidence floor.** `--min-fuzzy-score` matches plan/MCP/library so weak
  similarity matches fail closed instead of landing silently. (#1721, #1687)
- **Batch replace flags.** Line-oriented batch accepts `--fuzzy`, `--min-fuzzy-score`
  (including `=value` form), `--word-boundary` / `-w`, `--command-position`,
  `--require-change`, `-i` / `--case-insensitive`, and `--if-exists`, while dash-leading
  positionals (bullet text) stay positionals, not flags. (#1727)
- **Markdown insert-after-section.** New `md insert-after-section` / plan
  `md.insert_after_section` / MCP `md_insert_after_section` places content after the full
  section body (sibling `##` FAQ after Config keeps Settings under Config). Prefer
  `insert-after-heading` when you need intro text under the heading line. (#1727, #1726)
- **Similar targets on CLI no-match JSON.** When replace finds nothing, `--json` can
  surface close alternatives so agents can recover without re-scanning the file. (#1685)
- **Multi-document YAML as a first-class write path.** Streams with multiple `---`
  documents parse as a top-level array; writes keep separators (not a single YAML
  sequence). Address fields with a document index (`0.metadata.name` / `[0].…`). (#1719,
  #1728)

## Bug fixes

- **Multi-doc YAML no longer collapses to a sequence on write.** `doc set` and related
  writers re-serialize multi-document streams with `---` so tools like `kubectl apply -f`
  keep working. Bare keys on an array root now explain the index form instead of only
  saying "parent is not an object." (#1719, #1728)
- **Fuzzy match quality for agents.** Token-like alignment and span reporting for fuzzy
  hits; fail-closed `min_fuzzy_score` validation; embedder smoke coverage for identifier
  typos. (#1694–#1700, #1697–#1700)
- **MCP registry `key` alias.** Plan/MCP registry doc tools accept the legacy `key`
  alias for `selector` consistently with serde and agent-rules. (#1696, #1697)
- **Nested undo listing.** Recursive session discovery under nested backup roots so
  hosts that stash sessions deeper than one level still list them. (#1695, #1697)
- **Post-write backup root.** Backup placement respects the intended project root after
  Apply so undo stays local to the workspace. (#1693)
- **Case-only renames labeled correctly.** Text renames that only change case report
  `(case-only)`, not `(binary)`. (#1723)
- **Compact markdown table-append rows.** Table append accepts compact pipe-row shapes
  agents often emit. (#1716)
- **Doc single-path vs multi-match errors.** Predicate/wildcard selectors on
  `doc set` / `delete` / `move` point at `doc update` or `delete-where` with index
  examples instead of opaque navigation failures. (#1727, #1725)
- **Undo is dry-run until `--apply`.** Docs and agent-rules state the preview contract
  clearly so agents do not assume restore already ran. (#1717)

## Documentation

- Multi-document YAML index selectors in agent-rules, PATCHLOOM.md, and reference.
- npm install via OIDC Trusted Publishing; Scoop and Chocolatey package paths.
- Undo dry-run until `--apply`; markdown insert-after-heading vs insert-after-section
  placement guidance.

## Numbers

| Metric | v0.13.0 | v0.14.0 | Delta |
|--------|---------|---------|-------|
| Unit tests | ~2,402 | 2,425 | +~23 |
| Integration tests | ~1,086 | 1,089 | +~3 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **~3,455** | **3,524** | **+~69** |
| CLI commands | 23 | 23 | -- |
| MCP tools (with `ast`) | 56 | 56 | -- |
| Commits since v0.13.0 | -- | 29 | -- |

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.14"

# npm
npx patchloom --version
# or
npm install -g patchloom

# Scoop (Windows)
scoop bucket add patchloom https://github.com/patchloom/scoop-bucket
scoop install patchloom
# or
scoop update patchloom

# Homebrew (after formula updates)
brew upgrade patchloom
```

**Library hosts:** New APIs are additive. Prefer `min_fuzzy_score` when fuzzy is on;
read `EditResult.backup_session` after Apply; use `list_sessions_under` for nested
backup trees; `ast_rename_project` and `PostWriteHooks` need the same feature set as
existing AST/file writers (`ast` + `files` for pure embedders). Multi-doc YAML writes
expect document-index selectors (`0.key`).

**CLI / scripts:** Optional `--min-fuzzy-score` on `replace` (default: no floor).
`md insert-after-section` is additive beside `insert-after-heading`. Batch replace
flags default off. Under `--json`, multi-doc bare-key errors use `type_error` with an
index hint.

**Plans / MCP:** `min_fuzzy_score` and batch replace flags align with the CLI. New
`md.insert_after_section` / `md_insert_after_section` for sibling sections. Prefer
document indexes on multi-doc YAML. Registry doc tools accept `key` as a `selector`
alias.
