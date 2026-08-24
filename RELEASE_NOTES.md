# Patchloom 0.30.0

`patch apply` now accepts Codex Begin Patch envelopes and Aider
SEARCH/REPLACE documents, not only unified diffs. Doc selectors can
filter with numeric compares and field negation.

## Highlights

`patch apply`, MCP `apply_patch`, and plan `patch.apply` detect
`*** Begin Patch` and `<<<<<<< SEARCH` on their own. An Update hunk or
SEARCH block must match exactly once unless you pass `--replace-all`
(SEARCH/REPLACE only).

Selectors accept `!=`, `>`, `>=`, `<`, `<=`, and `[!key]`. Use
`servers[port>8000]` to pick numeric ports. A non-numeric compare
returns `error_kind: invalid_input`.

## New features

- **Begin Patch and SEARCH/REPLACE apply.** Codex `*** Begin Patch`
  (Add, Update, Delete, Move) and Aider SEARCH/REPLACE / DiffFenced
  documents apply through CLI `patch apply`, MCP `apply_patch`, and
  plan `patch.apply`. Update and SEARCH matches must be unique.
  `--replace-all` / `replace_all` is valid only on SEARCH/REPLACE.
  Mixing Begin Patch or SEARCH/REPLACE with a unified diff is
  `invalid_input`. Library hosts can call `apply_begin_patch`,
  `apply_search_replace_document`, and the dest-list helpers
  (`looks_like_begin_patch`, `begin_patch_declared_paths`,
  `looks_like_search_replace`, `search_replace_declared_paths`) ([#2222](https://github.com/patchloom/patchloom/pull/2222), [#2224](https://github.com/patchloom/patchloom/pull/2224)).

- **Selector comparison and negation.** Array and object-map filters
  now include `!=`, numeric `>` / `>=` / `<` / `<=`, and `[!key]`
  (absent, `false`, or `null`). Equality still allows `=` or `>`
  inside the value (`items[url=a>b]`). The operand after a numeric
  operator must be a number (`[port>abc]` is `invalid_input`). A
  present string field compared with `>` is also `invalid_input`, not
  a lexicographic compare. A missing field does not match. Regex
  predicates are not supported.

  ```
  patchloom doc get inventory.yaml 'servers[port>8000].name'
  ```

  The same selector works on `doc update`, plan `tx`, MCP, and batch ([#2233](https://github.com/patchloom/patchloom/pull/2233)).

- **`if_exists` on `doc.set` and `file.delete`.** When `if_exists` is
  true, a missing file (and, for `doc.set`, a missing selector) is a
  soft success with no write. The default stays `false` (fail-hard:
  `not_found` for a missing file). Replace already had this flag.
  Plan/tx, MCP, and batch `--if-exists` honor it. Standalone
  `doc set --if-exists` is not a CLI flag; use a plan, MCP, or batch
  line ([#2234](https://github.com/patchloom/patchloom/pull/2234), [#2235](https://github.com/patchloom/patchloom/pull/2235)).

- **Rewrite consumer imports after `ast.move` / `extract_to_file`.**
  Set `update_imports` with `old_module_path` and `new_module_path` to
  rewrite `use` / import statements of the moved or extracted symbols.
  The fields default off. Turning the flag on without both module
  paths is `invalid_input` ([#2232](https://github.com/patchloom/patchloom/pull/2232)).

## Bug fixes

- **Unrelated `doc.set` expanded YAML anchors.** Editing a sibling
  field no longer explodes `&name` anchors, `*name` aliases, or
  `<<: *name` merge keys. Local overrides of merge-inherited keys add
  an explicit key beside `<<`. Edits that must rewrite a pure alias
  value as a full mapping still expand that alias ([#2228](https://github.com/patchloom/patchloom/pull/2228)).

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.29.0 -> 0.30.0 |
| Focus | Patch grammars, selector predicates, if_exists |
| Tests | 4400+ (rounded badge) |

## Upgrading

- **Agents:** Prefer `patch apply` for Begin Patch and SEARCH/REPLACE
  instead of converting those documents to unified diffs. Unique match
  is the default; `--replace-all` on a unified diff or Begin Patch is
  `invalid_input`. Numeric selector compares that are not numbers
  return `invalid_input` (exit 1), not a silent miss. Soft-skip a
  missing `doc.set` / `file.delete` target with plan/MCP/batch
  `if_exists` (default remains fail-hard).
- **Library hosts:** Detect grammars with `looks_like_begin_patch` /
  `looks_like_search_replace` and list dests before apply. Do not
  copy a Begin Patch or SEARCH/REPLACE parser. `apply_patch` already
  routes those documents.
- **AST move/extract:** import rewrite is opt-in. Pass both module
  paths when `update_imports` is true.
- Install from crates.io, npm, Homebrew, or Scoop after the tag ships.
  On Windows, Scoop is recommended; winget may need
  `winget source update` after Microsoft publishes; Chocolatey can lag
  while moderation runs.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.29.0...patchloom-v0.30.0
