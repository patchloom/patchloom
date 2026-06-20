# AST Implementation Plan

Persistent plan for implementing all issues above #635 (#646-#655).
Use as a session handoff document. Check off steps as they complete.

## Dependency graph

```
#646 word_boundary (standalone, no deps)
  |
  v
#647 Phase 1: AST core + rename + list + read + validate
  |
  +---> #649 ast search (Phase 2a)
  +---> #650 ast map with PageRank (Phase 2b, needs list)
  +---> #651 ast refs (Phase 2c)
  +---> #652 ast deps (Phase 2d)
  |       |
  |       v
  +---> #653 ast replace (Phase 3a, needs read)
  +---> #654 ast impact (Phase 3b, needs refs)
  +---> #655 ast diff (Phase 3c, needs list)
```

## Step 1: #646 word_boundary (no deps, enables fallback)

**Files to create/modify:**
- `src/api.rs`: Add `word_boundary: bool` to `ReplaceOptions`
- `src/ops.rs`: Implement `\b`-wrapped regex in `replace_content()`
- `src/cmd/replace.rs`: Add `--word-boundary` / `-w` CLI flag
- `src/cmd/mcp.rs`: Add `word_boundary` param to `replace_text` MCP tool
- `src/schema.rs`: Update operation schema if needed
- `tests/integration.rs`: Tests for word boundary replace
- Unit tests in `src/ops.rs` `mod tests`

**Acceptance criteria from issue:**
- [ ] `ReplaceOptions.word_boundary` field (default `false`)
- [ ] Does not match `BenchSetupFile` when searching for `SetupFile`
- [ ] Does match standalone `SetupFile` in `use crate::SetupFile;`
- [ ] Handles regex metacharacters in search string
- [ ] CLI `--word-boundary` flag
- [ ] Backward compatible (default false)

**Verify:** `make check`

---

## Step 2: #647 Phase 1a - AST infrastructure + tree-sitter setup

**Files to create/modify:**
- `Cargo.toml`: Add `ast` feature flag with tree-sitter deps
- `src/ast/mod.rs` (new): Module root, re-exports
- `src/ast/grammar.rs` (new): Language detection (extension -> grammar),
  lazy grammar loading, parse file to tree
- `src/ast/nodes.rs` (new): Language-specific identifier node types table,
  definition query patterns per language
- `src/lib.rs`: Add `pub mod ast;` (cfg-gated on `ast` feature)

**Key design decisions:**
- `src/ast/` is a new module directory (like `src/selector/`)
- Feature-gated: `#[cfg(feature = "ast")] pub mod ast;`
- Grammar loading: one function `parse_file(path, lang_hint) -> Tree`
- Language detection: extension map (`.rs` -> Rust, `.py` -> Python, etc.)
- No caching in Phase 1 (add in Phase 2 with ast map)

**Crate selection:** Use `tree-sitter` crate (v0.24+) with individual
grammar crates (`tree-sitter-rust`, `tree-sitter-python`, etc.).
Start with the 6 most common languages for Phase 1 (Rust, Python,
TypeScript/JavaScript, Go, Java, C/C++). Add remaining grammars
incrementally as tests confirm they work.

**Verify:** `cargo build --features ast` compiles. Unit tests for
language detection and file parsing.

---

## Step 3: #647 Phase 1b - `patchloom rename` (AST-aware)

The existing `src/cmd/rename.rs` is file rename (mv). AST symbol rename
is a new command that needs a different name or the existing rename
needs to be restructured.

**Approach:** The issue says `patchloom rename OLD NEW PATH`. The existing
rename command is `patchloom rename FROM_PATH TO_PATH`. These have
different signatures (2 args + path vs 2 paths). Options:
1. New subcommand: `patchloom symbol-rename` (ugly)
2. Detect by argument pattern (fragile)
3. Rename existing to `patchloom mv` and take `rename` for AST (breaking)
4. Use `patchloom ast rename` as a subcommand under `ast`

**Recommended: Option 4.** Use `patchloom ast rename OLD NEW [PATHS]`.
Keeps existing `rename` (file move) untouched. All AST operations live
under `patchloom ast` subcommand group.

**Files to create/modify:**
- `src/cmd/ast.rs` (new): AST subcommand group with `AstCommand` enum
- `src/cmd/ast/rename.rs` (new): AST rename implementation
- `src/ast/rename.rs` (new): Core `rename_symbol()` function
- `src/cmd/mod.rs`: Add `pub mod ast;` (cfg-gated), add `Ast` variant
- `src/api.rs`: Add `pub fn rename_symbol()` to public API
- `tests/integration.rs`: Rename tests (strings, comments, identifiers)

**Core logic:**
```rust
pub fn rename_symbol(path, old_name, new_name, opts) -> EditResult {
    // 1. Read file
    // 2. Detect language from extension
    // 3. Parse with tree-sitter
    // 4. Walk AST, collect identifier nodes matching old_name
    // 5. Filter to only identifier-type nodes (skip strings, comments)
    // 6. Replace in reverse order (to preserve byte offsets)
    // 7. Return EditResult with diff
    // Fallback: if no grammar or parse fails, use replace_text with word_boundary
}
```

**Tests must verify:**
- Renames `SetupFile` in `struct SetupFile { }` (type_identifier)
- Renames `SetupFile` in `let x: SetupFile = ...` (type_identifier)
- Renames `setup_file` in `fn setup_file()` (identifier)
- Does NOT rename inside `"Loading SetupFile..."` (string)
- Does NOT rename inside `// comment about SetupFile` (comment)
- Does NOT rename inside `/// doc comment` (doc comment)
- Falls back to word-boundary replace for unknown extensions
- Falls back when `ast` feature disabled

**Verify:** `make check`

---

## Step 4: #647 Phase 1c - `patchloom ast list`

**Files to create/modify:**
- `src/cmd/ast/list.rs` (new): CLI subcommand
- `src/ast/symbols.rs` (new): Core symbol extraction logic
- `src/api.rs`: Add `pub fn list_symbols()` to public API
- `tests/integration.rs`: List tests

**Core logic:**
```rust
pub struct SymbolDef {
    pub name: String,
    pub kind: SymbolKind, // Function, Struct, Enum, Trait, Class, Method, ...
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    pub children: Vec<SymbolDef>, // nested (methods inside impl, tests inside mod)
}

pub fn list_symbols(path, opts) -> Vec<SymbolDef>;
```

**Language-specific queries:** Each language needs tree-sitter queries
that capture definitions. Use tag queries similar to Cline's approach:
- Rust: `function_item`, `struct_item`, `enum_item`, `trait_item`,
  `impl_item`, `mod_item`, `const_item`, `static_item`, `type_item`
- Python: `function_definition`, `class_definition`
- JS/TS: `function_declaration`, `class_declaration`, `method_definition`
- Go: `function_declaration`, `method_declaration`, `type_declaration`
- Java: `class_declaration`, `method_declaration`, `interface_declaration`
- C/C++: `function_definition`, `struct_specifier`, `class_specifier`

**Must support:**
- `--kind function,struct` filtering
- `--compact` mode (Cline-style, definition names only)
- `--json` / `--jsonl` output
- Directory recursion with `--glob` filtering

**Verify:** `make check`

---

## Step 5: #647 Phase 1d - `patchloom ast read`

**Files to create/modify:**
- `src/cmd/ast/read.rs` (new): CLI subcommand
- `src/ast/symbols.rs`: Add `read_symbol()` function
- `src/api.rs`: Add `pub fn read_symbol()` to public API
- `tests/integration.rs`: Read tests

**Core logic:** Find symbol by name in AST, extract its full source text
from start_line to end_line. Support `Class::method` syntax for nested
symbols. `--context N` adds N lines before/after.

**Verify:** `make check`

---

## Step 6: #647 Phase 1e - `patchloom ast validate`

**Files to create/modify:**
- `src/cmd/ast/validate.rs` (new): CLI subcommand
- `src/ast/validate.rs` (new): Core validation logic
- `src/api.rs`: Add `pub fn validate_syntax()` to public API
- `tests/integration.rs`: Validate tests (valid file, invalid file, directory)

**Core logic:** Parse file with tree-sitter, check `tree.root_node().has_error()`.
Report error node locations. Exit code 0 = valid, 1 = errors.

**Verify:** `make check`

---

## Step 7: #647 MCP tools for Phase 1

**Files to modify:**
- `src/cmd/mcp.rs`: Add MCP tools for all Phase 1 operations:
  - `rename_symbol` (ast rename)
  - `ast_list_symbols` (ast list)
  - `ast_read_symbol` (ast read)
  - `ast_validate` (ast validate)
- Update `mcp_lists_expected_tools` test with new tool names and count

**All MCP tools gated on both `mcp` + `ast` features.**

**Verify:** `make check`

---

## Step 8: #647 finalize - ancillary files, close issue

- `src/cmd/mod.rs`: Update agent-rules generator with new commands
- `docs/reference/README.md`: Add `ast` command documentation
- `tests/agent/drivers/base.py`: Add `ast` to `_PATCHLOOM_SUBCOMMANDS`
- `make sync-patchloom-md && make update-readme && make check`
- Close #647 after all acceptance criteria verified

---

## Step 9: #649 ast search (Phase 2a)

**Files to create/modify:**
- `src/cmd/ast/search.rs` (new): CLI subcommand
- `src/ast/search.rs` (new): Pattern matching engine
- `src/api.rs`: Add `pub fn ast_search()` to public API
- `src/cmd/mcp.rs`: Add `ast_search` MCP tool
- `tests/integration.rs`: Structural search tests

**Two modes:**
1. Pattern mode: Parse pattern as code, extract AST, match against target
   AST with meta-variable binding ($VAR, $$$MULTI)
2. Query mode: Pass raw S-expression to tree-sitter query API

**Verify:** `make check`

---

## Step 10: #651 ast refs (Phase 2c)

**Files to create/modify:**
- `src/cmd/ast/refs.rs` (new): CLI subcommand
- `src/ast/refs.rs` (new): Reference finding logic
- `src/api.rs`: Add `pub fn find_references()` to public API
- `src/cmd/mcp.rs`: Add `ast_refs` MCP tool
- `tests/integration.rs`: Reference finding tests

**Core logic:** For each file in scope, parse with tree-sitter, find all
identifier nodes matching the symbol name. Distinguish definitions from
references based on node parent type. Report file, line, context, kind.

**Verify:** `make check`

---

## Step 11: #652 ast deps (Phase 2d)

**Files to create/modify:**
- `src/cmd/ast/deps.rs` (new): CLI subcommand
- `src/ast/deps.rs` (new): Import extraction logic
- `src/api.rs`: Add `pub fn analyze_deps()` to public API
- `src/cmd/mcp.rs`: Add `ast_deps` MCP tool
- `tests/integration.rs`: Dependency analysis tests

**Language-specific import extraction queries:**
- Rust: `use_declaration`, `mod_item`, `extern_crate_declaration`
- Python: `import_statement`, `import_from_statement`
- JS/TS: `import_statement`, `call_expression` where fn is `require`
- Go: `import_declaration`
- Java: `import_declaration`
- C/C++: `preproc_include`

**Verify:** `make check`

---

## Step 12: #650 ast map with PageRank (Phase 2b)

**Files to create/modify:**
- `src/cmd/ast/map.rs` (new): CLI subcommand
- `src/ast/map.rs` (new): Repo map generation with ranking
- `src/api.rs`: Add `pub fn repo_map()` to public API
- `src/cmd/mcp.rs`: Add `ast_map` MCP tool
- `tests/integration.rs`: Repo map tests

**Depends on:** ast list (symbols), ast refs (reference graph).

**Core logic:**
1. Extract all definitions from all files (via `list_symbols`)
2. Extract all references (via identifier scan)
3. Build directed graph: edges from reference sites to definitions
4. Compute PageRank scores (implement simple PageRank, no networkx dep)
5. Sort symbols by score
6. Render top symbols within `--max-tokens` budget
7. `--focus` and `--boost` bias the ranking

**PageRank implementation:** Simple iterative PageRank in pure Rust
(~50 lines). No need for a graph library dependency.

**Token counting:** Estimate tokens as `content.len() / 4` (rough
approximation). No tokenizer dependency needed.

**Verify:** `make check`

---

## Step 13: #653 ast replace (Phase 3a)

**Files to create/modify:**
- `src/cmd/ast/replace.rs` (new): CLI subcommand
- `src/ast/replace.rs` (new): Symbol-scoped replace logic
- `src/api.rs`: Add `pub fn ast_replace()` to public API
- `src/cmd/mcp.rs`: Add `ast_replace` MCP tool
- `tests/integration.rs`: Symbol-scoped replace tests

**Core logic:** Locate symbol via AST (reuse `read_symbol` logic), then
run `replace_text` scoped to only the symbol's line range.

**Verify:** `make check`

---

## Step 14: #654 ast impact (Phase 3b)

**Files to create/modify:**
- `src/cmd/ast/impact.rs` (new): CLI subcommand
- `src/ast/impact.rs` (new): Transitive impact analysis
- `src/api.rs`: Add `pub fn analyze_impact()` to public API
- `src/cmd/mcp.rs`: Add `ast_impact` MCP tool
- `tests/integration.rs`: Impact analysis tests

**Core logic:** Build reference graph (reuse refs logic), compute
transitive closure up to `--depth` limit. Render as tree showing
call chains.

**Verify:** `make check`

---

## Step 15: #655 ast diff (Phase 3c)

**Files to create/modify:**
- `src/cmd/ast/diff.rs` (new): CLI subcommand
- `src/ast/diff.rs` (new): Structural diff logic
- `src/api.rs`: Add `pub fn ast_diff()` to public API
- `src/cmd/mcp.rs`: Add `ast_diff` MCP tool
- `tests/integration.rs`: AST diff tests

**Core logic:**
1. Get file contents at two git refs (or working tree vs HEAD)
2. Parse both with tree-sitter
3. Extract definitions from both (via `list_symbols`)
4. Diff the two symbol lists: added, removed, signature changed, body changed
5. Render human-readable or JSON output

**Verify:** `make check`

---

## Step 16: Final verification and cleanup

- Run `make check` one final time
- Run `make sync-patchloom-md && make update-readme`
- Verify all 9 issues (#646-#655) have acceptance criteria met
- Close all issues with commit references
- Push to main via PR

---

## Session handoff template

When starting a new session to continue this plan:

```
Continue implementing the patchloom AST plan at AST-IMPLEMENTATION-PLAN.md.
The plan covers issues #646-#655. Check which steps are completed (marked
with [x]) and continue from the next unchecked step. Run `make check`
after each step. All issues above #635 must be fully implemented and
tested before the plan is complete.
```
