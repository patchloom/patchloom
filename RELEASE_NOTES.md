# Patchloom 0.11.0

Agent and embedder reliability after the 0.10 containment release. Tighter `--contain` coverage, correct no-match exits for plans, MCP path and plan.cwd fixes, library APIs for signature rewrite and multi-edit, and clearer agent errors for batch and file lists. 1 feature PR, many user-visible fixes, and 71 new tests across 25 commits.

## Highlights

CLI sandboxes no longer leak through unguarded `ast` query commands under `--contain`. Transaction and library plans report missing symbols and headings as exit 3 (`no_matches`) with the concrete detail instead of a generic operation failure. MCP `execute_plan` honors a workspace-contained `plan.cwd` instead of silently ignoring it. Library embedders get `EditResult.removed`, `ast.rewrite_signature` / `api::ast_rewrite_signature`, multi-step content edits, and MCP `ast_rewrite_signature` (55 tools with `ast`). Agents get singular path aliases on search and batch tools, better batch "unknown op" hints, truthful `--files-from` / status messaging, and readable unified-diff headers for absolute paths.

## New features

- **Library embedder surface for Bline-class consumers.** `EditResult` includes `removed` for doc delete / delete-where (including idempotent zeros). Plan op and library API `ast.rewrite_signature` (structured signature fields or full `new_signature`) with PathGuard and ApplyMode. Multi-op content edits (`Replace`, `InsertBefore`, `InsertAfter`, `Append`, `Prepend`) via `apply_content_edits` / `apply_content_edits_to_file`. MCP exposes `ast_rewrite_signature` (default-feature tool count is now 55 with `ast`). CLI for rewrite_signature remains plan/MCP/library-first. (#1461, #1459)
- **Batch `ast.rewrite_signature` line form** documented in agent-rules for path / old / parameters / return_type style usage. (#1462, #1464)

## Agent and scripting reliability

- **Tx / plan no-match is exit 3 with detail.** AST rename and signature rewrite, md heading misses, and doc.update selector misses report `error_kind: no_matches` (exit 3) and keep the function/heading/selector text through intermediate error wrapping. Scripts no longer see exit 9 for "symbol not found." (#1462, #1464)
- **MCP `execute_plan` honors contained `plan.cwd`.** Relative `cwd` under the server workspace re-roots paths; absolute strings and `../` escapes are rejected (no silent strip). Empty/whitespace `plan.cwd` is rejected. Combining `plan.cwd` with `for_each` is rejected with a clear error. (#1466, #1472)
- **LLM-friendly parameter aliases.** MCP `search_files` accepts singular `path` (canonical remains `paths`). `batch_replace` and `batch_tidy` accept singular `file` (canonical remains `files`). When both forms are set, the array form wins. Empty/whitespace singular paths are rejected so they never mean "workspace root." (#1469, #1471, #1472)
- **Batch parse errors that help agents recover.** Unknown ops suggest bare-leaf matches (`create` → `file.create`), close typos (`file.creat`), and redirect CLI-only ops (`read` / `search` / `patch`) to standalone commands or tx. (#1483)
- **`--files-from` messaging and empty lists.** No-match text names `--files-from <list>` (or `-`) instead of a misleading `.`. Empty lists never fall back to walking the workspace. (#1476, #1477)
- **`status` ignores `.patchloom` backups.** After `--apply`, status no longer lists every backup session file as created. `init` ensures `.gitignore` includes `.patchloom/`. (#1478, #1479)
- **Unified-diff headers for absolute paths.** Headers use `a/tmp/...` style without a double slash (`a//tmp/...`). All leading slashes are stripped in the header form only; stored paths stay as provided. (#1481, #1482)
- **AST multi-file mutator guidance.** Tool descriptions warn agents not to issue concurrent writes on the same files (rename, replace, move, extract, split, and related batch tools). (#1469, #1470, #1475)

## Bug fixes

- **`--contain` on `ast list` / `deps` / `map` / `diff`.** Those commands used to join user paths without the containment check, so `../` could still read outside the workspace under `--contain`. (#1456, #1458)
- **Empty and whitespace-only path arguments rejected.** Omitting a path no longer silently scans the whole tree (`ast list ''` and similar). Applies even without `--contain`. (#1460)
- **YAML multiline string splice** propagates JSON-escape failures instead of panicking on an internal invariant. (#1487)

## Security and supply chain

- **Containment hole closed for AST query entry points** under `--contain` (see Bug fixes). (#1456)
- **MCP plan cwd and path aliases cannot escape or widen the workspace** via empty strings or absolute path forms. (#1466, #1472)
- **`cargo deny check` in CI** enforces `deny.toml` licenses/bans/sources alongside existing `cargo audit` and FOSSA. Local: `make deny`. (#1485 / #1486, allowlist prep #1483)
- **rmcp 2.2.0** (MCP protocol stack) with the matching `sse-stream` lockfile bump. (#1484 / #1486)

## Numbers

| Metric | v0.10.0 | v0.11.0 | Delta |
|--------|---------|---------|-------|
| Unit tests | 2,165 | 2,201 | +36 |
| Integration tests | 973 | 1,008 | +35 |
| PTY tests | 10 | 10 | -- |
| **Total tests** | **3,148** | **3,219** | **+71** |
| CLI commands | 23 | 23 | -- |
| MCP tools (with `ast`) | 54 | 55 | +1 |
| Commits since v0.10.0 | -- | 25 | -- |

## Upgrading

```bash
# Cargo
cargo install patchloom --locked
# or pin in Cargo.toml
patchloom = "0.11"

# Homebrew (after formula updates)
brew upgrade patchloom
```

**Agents / MCP:** Prefer `paths` / `files` arrays; singular `path` / `file` remain accepted. Put a relative `cwd` on `execute_plan` only when you need re-root under the server workspace; absolute `cwd` strings are rejected.

**CLI sandboxes:** Keep using `--contain` with `--cwd <workspace>`. After this release, `ast list` and related query commands respect containment the same way writes do.

**Scripts watching tx exit codes:** Treat exit 3 as "no match" for missing symbols/headings/selectors in plans (not exit 9). Exit 2 still means preview would change files.

**Library:** Read `EditResult.removed` after doc deletes. Use `api::ast_rewrite_signature` or plan op `ast.rewrite_signature` for structured signature edits; multi-step content helpers live under the content-edit APIs added in #1461.
