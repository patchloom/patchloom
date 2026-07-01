# Patchloom 0.9.0

This release adds fuzzy edit matching for library embedders, context-anchored CLI replacements, a new `prepend` command, and fixes a security-relevant path traversal bypass. 3 new features, 6 bug fixes, and 54 new tests across 19 commits.

## Highlights

Replace operations now support fuzzy fallback at the library API level (`replace_in_content`), so embedders like Bline get automatic Jaro-Winkler similarity matching without writing adapter code. The CLI replace command gains `--before-context` and `--after-context` flags for anchor-based match disambiguation. A path traversal bypass in the containment module was identified and patched, hardening the security boundary for library consumers that use `AllowIfContained` or `AllowAdditionalRoots` policies.

## New features

- **Fuzzy fallback for `replace_in_content`.** The in-memory replace API now accepts `fuzzy: true` in `ReplaceOptions`. When exact match fails, patchloom automatically tries Jaro-Winkler similarity matching, then returns suggestions if fuzzy also fails. This eliminates ~15 lines of manual fallback glue that library embedders previously needed. (#1292)
- **`--before-context` / `--after-context` on CLI replace.** Anchor text that disambiguates which match to target when a pattern appears multiple times in a file. Supports fuzzy anchor matching as a fallback. (#1290)
- **`prepend` command.** New CLI command that prepends content to the beginning of an existing file, bringing CLI parity with the MCP `prepend_file` tool. Accepts `--content` or `--stdin` (mutually exclusive). (#1290)

## Bug fixes

- **Path traversal bypass in containment.** `canonicalize_or_ancestor` silently dropped `..` components from non-existent path segments, allowing paths like `workspace/nonexistent/../../outside/secret.txt` to escape the workspace boundary. Fixed by adding lexical normalization of `.` and `..` before the ancestor walk. (#1296)
- **Markdown `replace-section` duplicated headings.** When replacement content started with the same heading being replaced, the heading appeared twice in the output. Now automatically stripped. (#1306)
- **Markdown `replace-section` dropped blank lines.** Replacing a section's content removed the trailing blank line separator before the next heading, producing tightly packed markdown. Now preserves the original spacing. (#1307)
- **YAML `doc set` stripped quote styles.** Setting a value on a double-quoted or single-quoted YAML scalar lost the quotes, producing a plain scalar. Now uses in-place value replacement that preserves the original quoting style. (#1283)
- **Per-extension formatters were dead code.** The `[format.by_extension]` table in `.patchloom.toml` was parsed and validated but never wired through to the format runner. Now correctly discovers modified files and applies extension-specific formatters. (#1285)
- **Patch, doc, and replace exit code fixes.** Creation patches no longer falsely report the target as missing; `doc move` rejects moving a key to its own descendant; replace preview mode returns consistent exit codes. (#1297)

## Library API changes

- **Bline issues #1287, #1288, #1289.** `ast.rename` in tx plans now supports directory paths (recursive rename across all source files); numeric dot-notation selectors (e.g., `env.0.value`) resolve as array indices; operation field descriptions are now included in the `execute_plan` JSON schema for better agent tooling. (#1291)
- **`selector` replaces `key`.** The `key` field on doc operations has been renamed to `selector` for consistency with selector-based commands. Old plans using `key` continue to work via serde aliases. (#1293)

## Internal improvements

- Deduplicated append/prepend implementation via shared `ContentPosition` enum. (#1299)
- Removed all 45 `#[non_exhaustive]` attributes and tightened `pub` to `pub(crate)` on `write.rs` internals. (#1300)
- Extracted the agent-rules generator from the oversized `cmd/mod.rs` into its own module. (#1301)
- Removed backward-compatibility serde aliases and legacy error prefixes. (#1298)
- Unified divergent `parse_line_range` implementations. (#1304)

## Numbers

| Metric | v0.8.0 | v0.9.0 | Delta |
|--------|--------|--------|-------|
| Unit tests | 1,983 | 2,019 | +36 |
| Integration tests | 838 | 856 | +18 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **2,831** | **2,885** | **+54** |
| CLI commands | 22 | 23 | +1 |
| MCP tools | 54 | 54 | -- |
| Commits | -- | 19 | -- |
