# Patchloom 0.15.0

Clearer machine-readable results for agents and scripts, safer fuzzy replace,
and hardlink-friendly writes. About 47 commits since 0.14.0, with about 91 new
tests.

## Highlights

When something fails or only partly succeeds, `--json` output now includes
stable fields such as `error_kind`, so a caller can decide the next step from
structured data instead of parsing English error text. If a write succeeds but
`--format` fails afterward, the response includes `backup_session` so you can
undo. Paths that were not found show up in `skipped`. Paths that were scanned
but did not match (so nothing was written there) show up in `refused` on
multi-file replace.

On Unix, when a file has hardlinks, Patchloom updates the shared content so
sibling link names keep pointing at the same data after a write or rename.

Fuzzy and context-based replace are stricter by default: if the exact old
string is missing, or context still matches more than one place, Patchloom
refuses the edit instead of guessing.

Public library result types (`EditResult`, plan/tx outputs, and related
structs) are marked `non_exhaustive`, so minor releases can add fields without
a major version bump for Rust embedders.

## New features

- **Library result types are `non_exhaustive`.** Embedders should treat
  `EditResult`, content-edit results, and tx `TxOutput` / `TxChange` as
  extensible in minor releases (prefer Patchloom constructors over hand-built
  struct literals). (#1744)
- **`matched_text` on fuzzy replace.** When fuzzy matching rewrites a span that
  is not identical to the `old` string you passed, JSON and library results
  include the actual matched text so you can check what changed. (#1737)
- **`explain -` reads a plan from stdin.** Same pattern as `tx -` and
  `batch -` (you can still use `explain --stdin`). (#1781)

## Bug fixes

### JSON and exit codes for agents and scripts

- **After Apply, a failing `--format` still reports the write.** The JSON
  includes `error_kind: format_failed` and `backup_session` (when a backup was
  created), so `ok: false` does not mean “no files changed.” Non-strict plan
  format/validate failures report applied changes the same way. (#1778, #1779)
- **Using a directory where a file is required returns `invalid_input`.**
  `read` and `doc set` on a directory no longer look like a missing file
  (`not_found`). (#1779, #1780)
- **`init --json` prints a JSON setup report**, not human-only status lines on
  stdout. (#1780)
- **Search `--max-results` sets `truncated` only when matches were cut off.**
  Count and files-with-matches modes no longer set a misleading truncation
  flag. (#1773, #1775)
- **Partial multi-file replace lists what was refused.** If some paths apply
  and others soft-miss (including weak fuzzy scores), those paths appear under
  `refused[]` so a successful overall status is not treated as “every path
  changed.” (#1774, #1761)
- **Missing input paths appear under `skipped`.** Explicit paths and
  `--files-from` entries that do not exist are listed in JSON for replace,
  search, and tidy (not only on stderr). (#1757, #1781, #1760)
- **No-match responses include both `error` and `error_kind`.** Replace and
  search soft failures put the message in the JSON body for callers that only
  read stdout. (#1755)
- **Plan/tx no-match JSON includes `error_kind` and `replace_hint`.** (#1753)
- **Unknown `undo --session` uses `error_kind: no_matches`.** (#1776)
- **`schema --format prompt` with global `--json` returns a JSON object** that
  wraps the prompt text, instead of raw markdown alone. (#1777)

### Stricter fuzzy and context replace

- **Fuzzy does not invent a match when the exact old string is missing**,
  unless you explicitly allow that path; weak similarity alone no longer
  rewrites the wrong token by default. (#1759, #1762)
- **Before/after context that still matches multiple places fails** instead of
  picking one site silently. (#1765, #1766)
- **`require_change`, `if_exists`, and `min_fuzzy_score` behave consistently**
  on globs and multi-file runs, with `replace_hint` when a better flag or
  pattern would help. (#1745, #1748–#1752, #1747, #1749)
- **MCP no longer blocks fuzzy/context replace with a false exact-only check**
  before the fuzzy path runs. (#1751)
- **Clearer CLI errors for word-boundary + fuzzy and related flag mistakes.**
  (#1754)
- **`--nth` reports out-of-range clearly.** You get a real match count instead
  of a fake “no matches”; multi-file applies the rule on every path; whole-line
  and range reporting stay consistent. (#1768, #1772, #1769)

### Hardlinks and renames

- **Unix hardlinks stay linked through atomic write** when a path has more than
  one link name: content is updated in place so every name sees the new bytes.
  (#1734)
- **Rename (CLI text rename, plan/tx `file.rename`, multi-hop chains, and
  rename-then-edit plans) keeps hardlinks** instead of breaking them with
  rename-over. (#1740–#1743, #1746)

### Documents, verify, and workspace scans

- **Empty `.json` files are treated as `{}` for doc get/set**, which matches
  create-then-fill workflows. (#1771)
- **`--verify` without the `ast` feature errors** instead of ignoring the
  flag. (#1763)
- **`unique_names` verify detects the same symbol name in different files.**
  (#1764)
- **CLI and plan verify no longer run the same checks twice.** (#1767)
- **Tidy does not walk `.git` even when hidden files are included**, which
  avoids binary object noise and slow scans. (#1770)

### Packaging

- **Chocolatey package metadata** adds a package icon URL and uses the docs
  site for the project home while keeping GitHub as the source URL (Community
  Repository guidelines). The first community listing may still be under
  moderation. (#1782)

## Documentation

- Agent-rules and help text cover format failure + backup session, search
  truncation, fuzzy defaults, and related JSON fields.
- Hardlink and rename behavior is locked by regression tests.

## Numbers

| Metric | v0.14.0 | v0.15.0 | Delta |
|--------|---------|---------|-------|
| Unit tests (`src/`) | 2,456 | 2,505 | +49 |
| Integration tests (`tests/`) | 1,101 | 1,143 | +42 |
| **Test attributes total** | **3,557** | **3,648** | **+91** |
| CLI commands | 23 | 23 | -- |
| Commits since v0.14.0 | -- | 47 | -- |

Counts are `#[test]` / `#[tokio::test]` attributes in `src/` and `tests/`
at each tag.

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

**Rust library users:** result structs are `non_exhaustive`. Prefer Patchloom
APIs to build results; hand-written struct literals may need a `..` update when
new fields appear in a minor release.

**Automation and agents:** prefer fields such as `error_kind`,
`backup_session`, `skipped`, `refused`, and `truncated` over free-form error
strings. If you see `format_failed` with a `backup_session`, restore with
`patchloom undo` or `undo --session <id>` when needed.

**Chocolatey:** package version `0.13.0` may still be in community moderation.
Do not assume `choco install patchloom` is listed globally until that version
is approved. For 0.15.0, prefer crates, GitHub Releases, Homebrew, npm, or
Scoop.
