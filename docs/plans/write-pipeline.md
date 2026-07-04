# Write pipeline (rewrite singularity)

**Status:** Engine stages via `stage(WriteRequest)`; CLI finalizes via
`write_mode` (`classify_write_mode` / `write_exit_code` /
`finalize_execution_result` / `finalize_callback_write`). Call-site
singularity completed after #1388: commands use `run_write` /
`stage_for_write`, not five parallel mode matrices.

**Goal:** one write pipeline owns mode → exit code, backup, and diff policy.
Strategies are *inputs* (`WriteSource`), not sibling exit owners.

## Contract (must hold for every path when changes exist)

| Mode | Flags | Exit | Mutation |
|------|-------|-----:|----------|
| Preview (default) | *(none of apply/check)* | **2** (`CHANGES_DETECTED`) | no |
| Check | `--check` | **2** | no |
| Apply | `--apply` | **0** (`SUCCESS`) | yes |
| Confirm + JSON decline | `--confirm` + `--json`/`--jsonl`, non-TTY / EOF | **2** | no |

When there are **no** changes, preview and check should exit **0**.

Integration lock: `tests/integration/write_path_contract_tests.rs`.

## Canonical flow

```
CLI / API boundary
  → WriteSource::{Operations(Vec<Operation>), Precomputed(Vec<…>)}
  → stage(WriteRequest { source, options: ExecuteOptions { context, guard } })
  → WriteReport (ExecutionResult)
  → finalize_execution_result(...)   // standard phase JSON schema
    OR finalize_report(..., FinalizeCallbacks { ... })  // custom emit only
  → commit/format inside finalize when apply/confirm accepts
```

**Rule:** `match classify_write_mode` must only appear in `src/cmd/write_mode.rs`.
Emit hooks for custom paths are grouped in `FinalizeCallbacks` (no per-command
mode matrices; no `clippy::too_many_arguments` on the entrypoint).

### CLI helpers (`src/cmd/output.rs`)

| Helper | Role |
|--------|------|
| `run_write` / `run_write_op` | stage + finalize (standard schema) |
| `run_write_op_no_preview_diffs` | same, no unified diffs in preview (delete) |
| `stage_for_write` | stage only; caller finalizes or custom-renders |
| `execute_via_engine*` | **compat aliases** of `run_write_op*` |

### Engine (`src/tx/engine.rs`)

| API | Role |
|-----|------|
| `stage` | **canonical** entry |
| `execute_single` / `execute_operations` / `execute_precomputed` | source implementations used by `stage` (also fine for tests) |
| `ExecuteOptions` | `EngineContext` + optional `PathGuard` only (no `GlobalFlags`) |

### Callback path (`src/cmd/write_dispatch.rs`)

Binary / case-only renames cannot use the UTF-8 tx engine. They use
`execute_write` → `finalize_callback_write` (same exit matrix).

## Commands

| Command | Staging | Finalize |
|---------|---------|----------|
| create, append, prepend, delete, doc writes, md most, ast replace | `run_write_op` / via engine alias | `finalize_execution_result` |
| ast rename | `stage_for_write(Operations)` | `finalize_execution_result` |
| replace (scan + context) | `stage_for_write` | `finalize_report` via `replace_output` hooks |
| tidy fix | `stage_for_write(Operations)` | `finalize_report` via `tidy_fix_output` hooks |
| patch apply | `stage_for_write(Operations)` | `finalize_report` hooks |
| md dedupe-headings | `stage_for_write` | `finalize_report` (side-channel headings first; no 2nd JSON body) |
| rename binary/case-only | n/a | `execute_write` / `finalize_callback_write` |

## Adding a write command

1. Prefer building an `Operation` and calling `run_write_op`.
2. If multi-file scan with precomputed content, use `WriteSource::Precomputed` + `stage_for_write`.
3. If JSON schema cannot use the phase constructor, custom-render **only** after `stage_for_write`, and still call `classify_write_mode` / `write_exit_code` (or `finalize_execution_result`).
4. Never call `atomic_write` from a command for normal text writes.
5. Add/extend a row in `write_path_contract_tests` when introducing a new public write surface.
