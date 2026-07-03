# Write pipeline inventory (Phase 1 of #1373 / epic #1372)

**Status:** inventory + contract matrix (PR 1.1). No behavior change yet.

**Goal of Phase 1:** one write pipeline owns mode → exit code, backup, and
diff policy. Strategies become *inputs*, not five sibling mode matrices.

This document is the call-site inventory for the collapse. Keep it updated
when adding or removing write entry points.

## Contract (must hold for every path when changes exist)

| Mode | Flags | Exit | Mutation |
|------|-------|-----:|----------|
| Preview (default) | *(none of apply/check)* | **2** (`CHANGES_DETECTED`) | no |
| Check | `--check` | **2** | no |
| Apply | `--apply` | **0** (`SUCCESS`) | yes |
| Confirm + JSON decline | `--confirm` + `--json`/`--jsonl`, non-TTY / EOF | **2** | no |

When there are **no** changes, preview and check should exit **0** (path-specific
edge cases stay in existing tests; this matrix covers the *has changes* case).

Integration lock: `tests/integration/write_path_contract_tests.rs`.

## Entry points (five production paths)

### 1. `execute_via_engine` / `execute_via_engine_inner`

| Item | Location |
|------|----------|
| Definition | `src/cmd/output.rs` |
| Engine core | calls `tx::engine::execute_single` |
| Mode/exit owner | **local** in `execute_via_engine_inner` |

**CLI call sites (representative):**

| Command | File | Notes |
|---------|------|-------|
| `create` | `src/cmd/create.rs` | representative in contract matrix |
| `append` | `src/cmd/append.rs` | |
| `prepend` | `src/cmd/prepend.rs` | thin wrapper over append-style |
| `delete` | uses **no_preview_diffs** variant below | |
| `rename` (text, non-case-only) | `src/cmd/rename.rs` | |
| `doc` writes | `src/cmd/doc.rs` | |
| `md` most writes | `src/cmd/md.rs` | some subcommands use `execute_single` directly |
| `ast` some writes | `src/cmd/ast.rs` | also `execute_operations` for multi-file |

**Variant:** `execute_via_engine_no_preview_diffs` — same function with
`preview_diffs: false` (e.g. `delete`). Not a sixth path; same mode/exit owner.

### 2. `execute_operations`

| Item | Location |
|------|----------|
| Definition | `src/tx/engine.rs` |
| Role | Stage multi-`Operation` plans; returns `ExecutionResult` |
| Mode/exit owner | **caller** (CLI reimplements mode branch after call) |

**CLI call sites:**

| Command | File | Notes |
|---------|------|-------|
| `tidy fix` multi-file | `src/cmd/tidy.rs` → `tidy_fix_output` | representative in contract matrix (`tidy fix …`) |
| `replace` with context anchors | `src/cmd/replace.rs` → `run_context_replace` | secondary |
| `ast` multi-file rename/etc. | `src/cmd/ast.rs` | |

### 3. `execute_precomputed`

| Item | Location |
|------|----------|
| Definition | `src/tx/engine.rs` |
| Role | Commit path for already-computed `(path, original, new)` triples |
| Mode/exit owner | **caller** (`replace_output` in `src/cmd/replace.rs`) |

**CLI call sites:**

| Command | File | Notes |
|---------|------|-------|
| `replace` (default multi-file scan) | `src/cmd/replace.rs` | representative in contract matrix |

### 4. `execute_write` (`write_dispatch`)

| Item | Location |
|------|----------|
| Definition | `src/cmd/write_dispatch.rs` |
| Role | Binary / case-only renames that cannot use UTF-8 tx engine |
| Mode/exit owner | **local** in `execute_write` (parallel to `execute_via_engine_inner`) |

**CLI call sites:**

| Command | File | Notes |
|---------|------|-------|
| `rename` binary | `src/cmd/rename.rs` → `run_binary_rename` | representative (null-byte file) |
| `rename` case-only | same | case-insensitive FS only |

### 5. `execute_single` + **custom CLI mode branch**

| Item | Location |
|------|----------|
| Definition | `src/tx/engine.rs` |
| Role | Single `Operation` → stage; returns `ExecutionResult` |
| Mode/exit owner | **shared helper** when via `execute_via_engine`; **custom** when CLI reimplements branch |

**Custom mode-branch callers (not using `execute_via_engine`):**

| Command | File | Notes |
|---------|------|-------|
| `patch apply` | `src/cmd/patch.rs` | representative in contract matrix |
| `md` dedupe-headings (and similar) | `src/cmd/md.rs` | own branch after `execute_single` |
| library API | `src/api/mod.rs` → `execute_as_edit_result` | maps to `EditResult`, not CLI exits |

`execute_via_engine` also calls `execute_single` internally; that does **not**
count as a separate mode matrix.

## Duplication map (why collapse)

Each of these owns a near-copy of check / apply / confirm+json / preview:

1. `cmd/output.rs` → `execute_via_engine_inner`
2. `cmd/write_dispatch.rs` → `execute_write`
3. `cmd/replace.rs` → `replace_output`
4. `cmd/tidy.rs` → `tidy_fix_output`
5. `cmd/patch.rs` (inline after `execute_single`)
6. Plus other custom branches (md specials, ast multi-file output)

Historical proof of divergence: PRs #1345–#1348 fixed the same preview
exit-0 bug across multiple paths independently.

## Collapse target (PR 1.2–1.4)

```text
WriteRequest {
  cwd, mode, policy, guard,
  source: SingleOp | Ops | Precomputed | BinaryDispatch,
  render: RenderPolicy { preview_diffs, ... },
}
→ WriteReport { has_changes, diffs, exit_code, ... }
```

One function owns mode → exit. Strategies only supply staged changes / apply
closures.

## Related issues

- Epic: #1372
- Phase 1: #1373
- Prior partial unifications: #967, #1004, #1007
- Exit multi-path: #1345–#1348
