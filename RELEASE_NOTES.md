# Patchloom 0.15.0

Agent JSON you can branch on, hardlinks that stay linked, and fuzzy replace that
fails closed when the old text is gone. About 47 commits since 0.14.0, with
about 91 new tests.

## Highlights

Hosts and CLI agents get structured failure metadata across write, search,
replace, undo, and schema: `error_kind`, `backup_session` after post-write
format failure, `skipped` for missing paths, `refused` for soft multi-path
misses, and accurate `truncated` on search caps. Unix hardlinks no longer break
when Patchloom rewrites or renames a multi-linked file. Fuzzy and context
replace refuse weak or ambiguous edits instead of writing the wrong span.
Public edit/tx result structs are `non_exhaustive` so minor releases can add
fields without a major bump for library embedders.

## New features

- **Library results marked `non_exhaustive`.** `EditResult`, content-edit
  results, and tx `TxOutput` / `TxChange` can gain honesty fields in minor
  releases without forcing embedders through a major. (#1744)
- **`matched_text` on fuzzy replace.** JSON and library results report the
  actual matched span when it differs from the requested `old` string, so
  agents can verify what changed. (#1737)
- **Explain from stdin like `tx`.** `patchloom explain -` reads a plan from
  stdin (same as `tx -` / `batch -` / `explain --stdin`). (#1781)

## Bug fixes

### Agent JSON and exit honesty

- **Post-write `--format` failure includes `backup_session`.** When Apply
  writes files then the format command fails, CLI `--json` exposes the undo
  session id and a restore hint so agents do not treat `ok: false` as “nothing
  wrote.” Non-strict tx format/validate failures report applied changes and
  backup the same way. (#1778, #1779)
- **Directory targets are `invalid_input`, not `not_found`.** `read` and
  `doc set` on a directory set `error_kind: invalid_input` so agents do not
  retry as create-missing. (#1779, #1780)
- **`init --json` returns a structured setup report** instead of human status
  text on stdout. (#1780)
- **Search `--max-results` truncation is accurate.** Content search sets
  `truncated` when the match list is capped; count / files-with-matches modes
  do not claim truncation incorrectly. (#1773, #1775)
- **Soft multi-op no-match surfaces in `refused[]`.** Multi-file or multi-op
  replace that applies some paths and soft-misses others lists refused paths
  (including fuzzy floor rejects) so partial apply is not mistaken for full
  coverage. (#1774, #1761)
- **Missing paths appear in JSON `skipped`.** Explicit CLI paths and
  `--files-from` entries that do not exist are listed under `skipped` for
  replace, search, and tidy (not only stderr under `--quiet`). (#1757, #1781,
  #1760)
- **No-match JSON always carries `error` with `error_kind`.** Replace and
  search soft failures put the diagnostic in the JSON envelope so agents that
  only read stdout still see the message. (#1755)
- **Tx no-match JSON includes `error_kind` and `replace_hint`.** Plan/tx
  soft-miss payloads stay machine-readable. (#1753)
- **Unknown `undo --session` is `no_matches`.** Agents can branch on kind
  instead of scraping English. (#1776)
- **`schema --format prompt` under global `--json` wraps markdown** in a JSON
  envelope instead of dumping raw text. (#1777)

### Fuzzy, context, and replace fail-closed

- **Fuzzy refuses when the exact old string is absent** unless you opt into
  inventing a match; weak similarity no longer rewrites the wrong token by
  default. (#1759, #1762)
- **Context anchors fail closed when they do not disambiguate.** Before/after
  context that still matches multiple sites no longer picks one silently.
  (#1765, #1766)
- **`require_change`, `if_exists`, and `min_fuzzy_score` interact correctly**
  on glob and multi-file paths: floor rejects, soft skips, and forced
  failure modes stay consistent, with `replace_hint` when useful. (#1745,
  #1748–#1752, #1747, #1749)
- **MCP skips exact pre-validation when fuzzy or context is set**, so agents
  are not blocked by a false exact miss before the fuzzy path runs. (#1751)
- **Word-boundary + fuzzy and CLI replace error hints** report what agents
  need to fix the call (flags, context, or pattern). (#1754)
- **`--nth` honesty.** Out-of-range `--nth` fails with a live match count
  instead of a false no-match; multi-file applies the rule to every path;
  whole-line + range/`--lines` reporting stays consistent. (#1768, #1772,
  #1769)

### Hardlinks and renames

- **Atomic write preserves Unix hardlinks** when `nlink > 1`, updating the
  shared inode so sibling link paths see the new content. (#1734)
- **Text rename, plan/tx `file.rename`, multi-hop rename chains, and
  rename-then-edit plans** keep hardlinks intact instead of breaking them
  with rename-over. Force rename and related paths cover the same contract.
  (#1740–#1743, #1746)

### Document, verify, and workspace hygiene

- **Empty JSON files parse as `{}` for doc read/set**, matching agent
  expectations for create-then-fill workflows. (#1771)
- **`--verify` without the `ast` feature fails closed** instead of silently
  doing nothing. (#1763)
- **`unique_names` verify catches cross-file symbol collisions.** (#1764)
- **CLI and plan verify checks no longer double-run** the same checks. (#1767)
- **Tidy with include-hidden never walks `.git`**, avoiding binary object
  noise and slow scans. (#1770)

### Packaging

- **Chocolatey package metadata** adds a CDN package icon and separates
  project home (docs) from source URL for Community Repository guidelines.
  First listing remains under moderation until approved; later versions
  publish after that. (#1782)

## Documentation

- Agent-rules and help text for fail-closed fuzzy defaults, format_failed +
  backup_session, search truncation, and related JSON fields.
- Hardlink and rename contracts covered in tests and release host notes.

## Numbers

| Metric | v0.14.0 | v0.15.0 | Delta |
|--------|---------|---------|-------|
| Unit tests (`src/`) | 2,456 | 2,505 | +49 |
| Integration tests (`tests/`) | 1,101 | 1,143 | +42 |
| **Test attributes total** | **3,557** | **3,648** | **+91** |
| CLI commands | 23 | 23 | -- |
| Commits since v0.14.0 | -- | 47 | -- |

Counts are `#[test]` / `#[tokio::test]` attributes in `src/` and `tests/`
at each tag (same method as internal hygiene scans).

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.15"

# npm
npx patchloom --version

# Homebrew
brew upgrade patchloom

# Scoop
scoop update patchloom
```

**Library embedders:** public result structs are now `non_exhaustive`. Continue
to construct them only via Patchloom APIs (or update struct literals if you
built them by hand). Expect new optional fields in minor releases without a
major version bump.

**Agents:** prefer branching on `error_kind`, `backup_session`, `skipped`,
`refused`, and `truncated` rather than English error text. After
`format_failed` with a backup session, use `patchloom undo` (or
`undo --session <id>`) if you need to restore.

**Chocolatey:** community package `0.13.0` may still be in moderation; do not
assume `choco install patchloom` is globally listed until that listing is
approved. Other channels (crates, GitHub Releases, Homebrew, npm, Scoop) are
the primary install paths for 0.15.0.
