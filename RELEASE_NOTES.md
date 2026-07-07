# Patchloom 0.10.0

Agent reliability and workspace safety. Optional CLI path containment, correct preview exit codes and structured JSON across the board, doc mutation summaries for plans/MCP, and a long series of agent-facing naming and path-resolution fixes. 3 features, dozens of user-visible fixes, and 263 new tests across 102 commits.

## Highlights

Scripts and agents can now treat the working tree as a hard boundary: pass `--contain` on the CLI to reject path escapes for reads, writes, plans, batch files, patches, and `--files-from` lists (MCP already enforced containment). Default preview mode finally returns exit code 2 (`CHANGES_DETECTED`) when an edit would change files, so automation no longer mistakes a dry-run for a no-op. JSON and text error paths are consistent for no-match and ambiguous cases. Doc delete operations report `changed` and `removed` so agents can tell a real delete from an idempotent no-op without re-reading the file.

## New features

- **`--contain` (optional CLI workspace path guarding).** Rejects paths that escape the working directory via `../`, absolute paths outside the workspace, or out-of-tree symlinks. Applies to reads, writes, and meta-input files (transaction plans, batch op lists, patch files, `--files-from`). Absolute paths *under* the workspace are allowed. Default CLI mode stays unrestricted for human scripts; use `--contain` for agent sandboxes. MCP always enforces containment. (#1407, #1410, #1412, #1417, #1418, #1447, #1452)
- **Doc delete mutation summary (MCP and tx/plan JSON).** Successful `doc.delete` / `doc.delete_where` operations surface `changed` and `removed` counts (and per-op mutation details on plan reports) so agents can distinguish a no-op from a real delete without a second read. (#1441, #1437)
- **Schema-driven MCP tool descriptions and post-rewrite library cleanup.** MCP simple tools pull prose and examples from the operation schema registry (with optional MCP-only extras). Library callers of removed shims must migrate (see Breaking changes). (#1387)

## Agent and scripting reliability

- **Preview mode returns exit 2 when changes would be applied.** Default (no `--apply` / `--check` / `--diff`) write paths for replace, create, delete, append, doc, md, ast, rename, tx, batch, and patch now return `CHANGES_DETECTED` (2) instead of success (0) when the operation would mutate files. Scripts that treat exit 0 as "nothing to do" now work correctly. (#1345, #1346, #1347, #1348, #1373/#1377/#1378)
- **Structured JSON on every no-match and error path.** Context replace, patch apply, undo, agent-rules, ast/doc/search/md, and related commands emit proper `{"ok": false, ...}` envelopes under `--json` / `--jsonl`, and print stderr diagnostics in text mode (including when stderr is piped). (#1317–#1322, #1334, #1335, #1338, #1343, #1344, #1360, #1365)
- **Agent-friendly names and stdin conventions.**
  - Replace plans/MCP accept `from` / `to` as aliases for `old` / `new`.
  - `patch apply -` reads a patch from stdin (same as `--stdin`).
  - `md lint` is a CLI alias for `md lint-agents`.
  - Doc MCP params still accept legacy `key` alongside `selector`.
  - AST rename uses `old` / `new` consistently across CLI, plan, and MCP.
  - Tx `format` / `validate` steps accept `"command"` as an alias for `"cmd"`.
  - `schema --tier` is a proper clap enum (valid values in help). (#1423, #1420, #1421, #1426, #1323, #1427)
- **`--cwd` path resolution for meta inputs.** Batch ops files, patch files, explain plans, and related meta paths resolve under `--cwd` instead of only the process working directory. `--files-from` lists honor `--cwd` and `--contain`. (#1444, #1445, #1419, #1418)

## Bug fixes

- **TOML inline tables stay inline under `doc set`.** Setting a field inside `options = { debug = true }` no longer expands the value into a multi-line `[options]` table. (#1328)
- **`doc delete-where` accepts `value=` for scalar arrays.** Predicate form for simple value lists matches what agents expect from nested-path predicates. (#1425)
- **`tidy fix` defaults match `tidy check`.** Fix mode no longer leaves issues that check would report (or vice versa) under default flags. (#1424)
- **Context and fuzzy replace path fixes.** Context anchoring is wired through the library fuzzy fallback; the fallback path no longer ignores context/fuzzy options incorrectly. (#1312, #1316)
- **Silent no-match errors on piped stderr.** Doc, undo, replace, and search now emit diagnostics when stderr is not a TTY (scripts and CI no longer look like empty success). (#1340, #1341, #1343, #1344)
- **PathGuard gaps in patch apply and glob-replace** so contained library/MCP sessions cannot write outside the workspace on those paths. (#1367)
- **Bounded regex compilation and quiet/JSON guards** for ast and related error paths. (#1357)
- **Backup permissions, `.patchloom` exclusion, and editorconfig edge cases.** (#1354, #1352)

## Security

- **CLI containment completed for writes, reads, and meta-inputs** under `--contain` (see New features). Escape errors name the workspace root. Replace fails before scanning escaped path lists. (#1407–#1418, #1414, #1415, #1447, #1452)
- **Library/MCP `PathGuard` gaps closed** for `PatchApply` and glob-replace paths that previously could slip past containment. (#1367)
- **Dependency fix for RUSTSEC-2026-0204** via `crossbeam-epoch` bump. (#1438)

## Breaking changes

- **Library: removed deprecated AST shims.** Call `ast::extract_to_file` instead of `ast::extract { … }`. Call rewrite helpers via `ast::rewrite` instead of re-exports from `ast::symbols`. (#1387 / #1386)
- **CLI scripts that relied on preview exit 0 when changes exist must treat exit 2 as "would change".** Success (0) now means no change in default preview mode for write commands; exit 2 means changes were detected (same semantic as `--check` with differences). (#1345–#1348, #1373)
- **AST rename positional / field contract.** Use `old` and `new` (not legacy positional assumptions). Plans and MCP follow the same names. (#1426, #1428, #1430)

## Internal improvements

- Single write-mode owner (`write_mode` + `stage` / `finalize_*`) so exit codes and apply/check/preview cannot diverge per command. (#1373, #1388–#1391, #1401)
- Plan module split (`plan/`), execute_operation decomposition, symbols split, tidy check/fix modules, schema-aware explain, MCP registry growth with surface inventory tests. (#1369, #1371, #1379–#1381, #1391–#1395)
- Feature-matrix CI: `test-mcp-no-ast` and `test-library-hygiene` in `make check`; custom MCP inventory matches `list_tools` under feature flags. (#1395, #1397, #1364)
- Fuzz matrix completed (additional targets in CI). (#1368)

## Numbers

| Metric | v0.9.0 | v0.10.0 | Delta |
|--------|--------|---------|-------|
| Unit tests | 2,019 | 2,165 | +146 |
| Integration tests | 856 | 973 | +117 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **2,885** | **3,148** | **+263** |
| CLI commands | 23 | 23 | -- |
| MCP tools (with `ast`) | 54 | 54 | -- |
| Commits since v0.9.0 | -- | 102 | -- |

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.10"

# Homebrew (after formula updates)
brew upgrade patchloom
```

**Library embedders:** grep for `ast::extract` and `use patchloom::ast::symbols::{` rewrite imports; switch to `extract_to_file` and `ast::rewrite`.

**Agent / sandbox CLI:** add `--contain` (and usually `--cwd <workspace>`) when invoking patchloom from an agent so plans and file lists cannot escape the project root.

**Automation that greps exit codes:** treat `2` as "preview found changes" for write commands in default mode; do not assume `0` means "operation planned successfully with diffs."
