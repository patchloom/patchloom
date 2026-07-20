# Patchloom 0.15.3

Clearer multi-document YAML editing, binary file handling that agents can
trust, and install paths for MCP directories. Twenty-two commits since 0.15.2.

## Highlights

Multi-document YAML (files with `---` separators) is represented as a
top-level array. Bare keys such as `a` no longer silently target the wrong
place: `doc get` / `set` / `has` / `keys` / `merge` and related ops require
an index first (`0.a` or `[0].a`) and return `error_kind: type_error` with a
short hint when you skip it. Merging an object (or any non-array overlay)
onto that array root is refused so the whole stream is not replaced by
accident.

Binary files (NUL in the first probe window) no longer look like pattern
misses or "already tidy." A sole explicit binary path for search, replace,
tidy, read, append, prepend, or md is `invalid_input`. When you list several
paths, search, replace, and tidy put binary co-paths in `refused[]` with
`reason: binary` while still processing text siblings. Multi-path read lists
binaries under `skipped[]`.

MCP distribution moved forward: OIDC publish to the official MCP Registry,
Smithery MCPB pack and REST publish, and repo `glama.json` for the Glama
directory. Agents also get more consistent JSON for write preview (`applied`),
empty `--files-from`, md lint envelopes, and markdown explain text.

## Bug fixes

### Multi-document YAML and doc selectors

- **Bare keys on multi-doc (and top-level array) roots return `type_error`.**
  Address a document first with `0.` / `[0].` for get, set, has, keys, len,
  append, prepend, delete, update, move, and related paths. Error text names
  the index pattern. (#1862, #1863, #1871, #1873, #1874)
- **`doc merge` refuses overlays that would replace a top-level array root.**
  Object, scalar, and other non-array overlays no longer wipe multi-doc
  streams; merge only into object documents (for example under `0.`). (#1872,
  #1875)

### Binary paths and multi-path lists

- **Sole explicit binary is `invalid_input` for search, replace, tidy, read,
  and md.** Not pattern `no_matches`, not "already clean," and not a false
  heading miss on markdown. (#1875, #1876, #1878)
- **Multi-path search, replace, and tidy report binary co-paths in
  `refused[]`** with `reason: binary`. Partial success on text files does not
  mean every listed path was scanned. Directory walks still soft-skip binaries
  without a mass `refused` list. (#1877, #1878)
- **Multi-path read lists binary paths in `skipped[]`** with
  `error_kind: invalid_input` when some paths fail. (#1876)

### Agent and script JSON

- **Preview and multi-op JSON keep `applied` accurate** for pre-write paths
  and batch replace ordering so callers can tell whether bytes landed. (#1856)
- **Empty `--files-from` is `invalid_input` for search and tidy**, not a quiet
  pattern miss or clean tidy. (#1861)
- **`md lint-agents` / MCP `md_lint` use a tidy-style JSON object** (`ok`,
  `issue_count`, `issues`, path) instead of a bare array, matching CLI
  expectations. (#1858, #1860)
- **`explain` describes markdown section bounds and dedupe discard** in plain
  language so plan summaries match real behavior. (#1864, #1865)

### MCP install and publish

- **Official MCP Registry:** repo markers, `server.json`, and OIDC
  `publish-mcp-registry` workflow so listings can publish on release. (#1866)
- **Smithery:** MCPB pack in CI, REST publish (CLI 400 workaround), version
  pin and pack hygiene. (#1867, #1868, #1870)
- **Glama:** root `glama.json` with maintainers for claim / directory prep
  (listing still depends on Glama review and search visibility). (#1869)

## Numbers

| Metric | 0.15.2 | 0.15.3 |
|--------|--------|--------|
| Test attributes (`src/` + `tests/`) | 3,743 | 3,786 |
| Unit-ish (`src/`) | 2,547 | 2,573 |
| Integration-ish (`tests/`) | 1,196 | 1,213 |
| CLI commands | 23 | 23 |
| Commits since 0.15.2 |  | 22 |

Counts are `#[test]` / `#[tokio::test]` attributes at each tag (or HEAD for
0.15.3 pre-tag). README still shows the rounded 3700+ badge until the hundreds
digit moves.

## Upgrading

```bash
# crates.io
cargo install patchloom --locked

# or upgrade an existing install
cargo install patchloom --locked --force
```

Other channels (Homebrew, Scoop, npm, GitHub Releases) update with the usual
release workflow. Chocolatey and winget may lag community moderation.

### For agent and script authors

1. **Multi-document YAML:** use `0.key` / `[0].key` (or another index). Bare
   `key` on a multi-doc root is `type_error`, not a silent wrong document.
2. **`doc merge` on multi-doc:** merge into a document object (`0.` …), not
   the stream root, unless the overlay is a same-shape array you intentionally
   want.
3. **Binary co-paths:** on multi-file search, replace, and tidy, read
   `refused[]` (and read's `skipped[]`) even when overall `ok` is true and some
   text files matched.
4. **Sole binary path:** expect `error_kind: invalid_input` and exit 1, not
   exit 3 / `no_matches`.
5. **Empty `--files-from`:** treat as bad input, not "no matches."
6. **md lint JSON:** parse the object envelope (`issues` array inside), not a
   top-level array, for both CLI and MCP.

No intentional breaking changes to successful happy-path shapes beyond the
refusal and envelope fixes above (callers that assumed bare-key multi-doc or
silent binary skip will see structured errors instead).
