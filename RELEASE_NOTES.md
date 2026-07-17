# Patchloom 0.15.1

More reliable agent JSON for write results, safer batch content, and clearer
search and patch check signals. Sixteen commits since 0.15.0, with about 67 new
tests.

## Highlights

After `--apply`, successful writes report `applied: true` in `--json`, and
no-ops (for example `doc ensure` when the value is already present) report
`applied: false` instead of looking like a real edit. Delete and rename include
`backup_session` so you can undo without listing sessions first.

Batch line format is safer for agents: unquoted JSON objects keep their quotes,
and `\n` / `\t` / similar escapes expand in `file.create` / `append` /
`prepend` content so multi-line scaffolds fit on one batch line.

`search` `match_count` now counts every occurrence on a line (same idea as
replace). `patch check` uses `would_change` and exit 2 when a patch would
mutate, matching apply preview instead of looking "clean" with exit 0.

## Bug fixes

### JSON fields agents and scripts can use

- **Writes report `applied` correctly after `--apply`.** Create, append,
  rename, doc, md, and related paths set `applied: true` when the write lands.
  Preview and `--check` still omit it or leave applied false. (#1788)
- **No-op applies do not look like successful edits.** Identity `doc ensure` /
  `doc set`, empty append/prepend, identical md section bodies, and similar
  paths set `applied: false` (and related `files_changed` reporting for AST
  rename). (#1816)
- **Delete and rename JSON include `backup_session`.** After `--apply`, the
  session id is in the success payload so agents can call
  `patchloom undo --session <id>` without a separate list step. (#1817)
- **`backup_session` on more successful apply paths**, and multi-file
  `--jsonl` reports refused/skipped/summary consistently with full JSON.
  (#1815)
- **Soft no-match and fuzzy refuse set `ok: false` on plan/tx and MCP** where
  CLI already did, so hosts do not treat a soft miss as full success. (#1814,
  #1791)
- **Multi-path replace lists zero-match paths in `refused[]`.** Partial apply
  stays visible under overall success. (#1814, #1792)
- **md `dedupe-headings --json` returns an object** with `removed`, `applied`,
  and `backup_session` (not a bare array). (#1816)

### Batch content and optional replace

- **Unquoted JSON in batch `file.create` keeps inner quotes.**
  `file.create f.json {"x":1}` writes valid JSON instead of `{x:1}`. Brace and
  bracket tokens stay balanced during tokenization. (#1820)
- **Escape sequences expand in batch file content.** In
  `file.create` / `append` / `prepend`, `\n`, `\t`, `\r`, `\\`, and `\"` expand
  so multi-line content can live on one batch line. (#1821)
- **`replace … --if-exists` on a missing path soft-skips in batch and tx
  plans.** Optional files no longer abort the whole batch with `not_found`.
  Without `--if-exists`, missing paths still hard-fail for atomic safety.
  (#1823, #1793)

### Search, patch, binary, and AST

- **Search `match_count` counts every non-overlapping hit on a line.** A line
  with three `foo` tokens reports 3, matching replace. Invert-match stays
  line-oriented. (#1819)
- **`patch check` reports `would_change` and exit 2** when the patch would
  mutate files (same idea as apply preview). Already-matching content reports
  unchanged; stale and missing paths still fail closed. (#1818)
- **Sole explicit binary targets fail with `invalid_input`** for replace and
  tidy (not silent "already clean" / no_matches). Multi-path walks still
  soft-skip binaries. (#1790)
- **Append and prepend reject binary (NUL) files** with `invalid_input` and
  leave the file unchanged. (#1789)
- **`ast validate` fails closed** on unsupported languages or empty targets
  (`no_matches`, exit 3) instead of empty success. (#1787)
- **Create and rename reject paths whose parent is a file** (not a directory)
  with `invalid_input`, without creating a backup for an unwritten path.
  (#1785)

## Documentation

- Agent-rules and reference docs cover batch JSON content, escape expansion,
  `if_exists` on missing paths, patch check exit codes, and related JSON
  fields. (#1822, #1823, and related)

## Numbers

| Metric | v0.15.0 | v0.15.1 | Delta |
|--------|---------|---------|-------|
| Unit tests (`src/`) | 2,506 | 2,536 | +30 |
| Integration tests (`tests/`) | 1,143 | 1,180 | +37 |
| **Test attributes total** | **3,649** | **3,716** | **+67** |
| CLI commands | 23 | 23 | -- |
| Commits since v0.15.0 | -- | 16 | -- |

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

**Automation and agents:** Prefer `applied`, `backup_session`, `error_kind`,
`refused`, and `skipped` over free-form status text. Treat `applied: false`
with `ok: true` as "no bytes changed," not failure. For optional files in a
batch, pass `--if-exists` on replace; omit it when a missing path should abort
the transaction.

**Batch authors:** Unquoted JSON objects in `file.create` are safe. Use `\n`
(and friends) for multi-line content on one line, or use a `tx` plan when you
need full plan features.

**Patch automation:** `patch check` exit 2 means the patch would change files;
exit 0 means it would not. Do not treat exit 0 as "already applied" for
stale/mismatch cases (those still fail closed).
