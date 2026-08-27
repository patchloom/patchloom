# Patchloom 0.31.0

`doc set` can turn a pure YAML alias (`service_a: *shared`) into a
merge key plus local fields, instead of inlining the whole object.
Create and rename refuse a destination whose parent is a file.

## Highlights

YAML files that reuse an object with `key: *anchor` keep the
shared mapping when you add or override fields. The line becomes
`<<: *anchor` plus your local keys. `&anchor` definitions stay
intact.

Create and rename check each parent on the destination path. A
parent that is a regular file (or a dangling / file symlink)
returns `error_kind: invalid_input` and the message
`parent path is not a directory`. A symlink that points at a
directory, including macOS `/tmp`, is allowed.

## New features

- **YAML alias to merge on interior edit.** `doc set` (CLI, plan
  `tx`, MCP, and the library) on a mapping that is only
  `service_a: *shared` writes a merge plus local keys when the new
  object is a superset of the resolved mapping:

  ```yaml
  shared: &shared
    timeout: 30
  service_a: *shared
  ```

  After `doc set config.yaml service_a.retries --value 3 --apply`:

  ```yaml
  service_a:
    <<: *shared
    retries: 3
  ```

  Sequence items (`- *shared`) still expand or stay not-applied.
  A non-plain key skips the splice and may dump that file through
  the CST. Tab-indented YAML is invalid and is rejected by the
  parser ([#2243](https://github.com/patchloom/patchloom/pull/2243)).

## Bug fixes

- **Two `cfg: *shared` lines dumped the whole file.** A unique-line
  splice treated the second mapping alias as a miss and rewrote
  both sites. Occurrence indexes are now file-wide for
  `key: *alias`, the same way they already were for `- *alias` ([#2247](https://github.com/patchloom/patchloom/pull/2247)).

- **Sequence alias `set` looked applied and did nothing.** After
  yaml-edit 0.3, `Sequence::set` on `- *alias` can return success
  without changing the node. Patchloom now reports that edit as
  not applied instead of writing an unchanged CST dump ([#2252](https://github.com/patchloom/patchloom/pull/2252), [#2255](https://github.com/patchloom/patchloom/pull/2255)).

- **Create or rename through a file parent failed late with a
  bare IO error.** The parent check now runs before staging.
  Destinations that are already directories are refused before a
  sibling write in the same apply. `--json` failures on these
  paths, plus init hard-fail and undo dest-kind dry-run, set
  `error_kind` ([#2258](https://github.com/patchloom/patchloom/pull/2258), [#2259](https://github.com/patchloom/patchloom/pull/2259)).

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.30.0 -> 0.31.0 |
| Focus | YAML alias-to-merge, dest-parent classify |
| Tests | 4600+ (4640 unit + integration + PTY) |

## Upgrading

- **Agents:** Prefer `doc set` on `service_a: *shared` when you
  want local keys beside the shared mapping. Do not convert the
  alias to a full inline object first. A parent that is a file is
  `invalid_input`, not a late IO failure.
- **Library hosts:** `ApplySearchReplaceOptions` gained
  `file_hint: Option<PathBuf>` (use `None` unless you remap dests
  the way `apply_patch` does). `SessionListing` gained
  `warnings: Vec<String>` for missing or corrupt backup manifests.
  `Command::McpServer` gained `allow_unauthenticated`. The
  internal `ops::md::table_append_for_tx` helper is gone; call
  `table_append_in`.
- Install from crates.io, npm, Homebrew, or Scoop after the tag
  ships. On Windows, Scoop is recommended; winget may need
  `winget source update` after Microsoft publishes; Chocolatey can
  lag while moderation runs.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.30.0...patchloom-v0.31.0
