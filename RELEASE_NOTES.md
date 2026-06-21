# Patchloom v0.3.0

This is the biggest patchloom release yet. It adds a full AST layer powered by tree-sitter, a new `file.append` command, post-write formatting hooks, smarter error recovery for failed edits, and a major internal cleanup.

## AST-aware operations (new `ast` feature)

Patchloom can now parse source code. The new `ast` command uses tree-sitter grammars for 20 languages (Rust, Go, Python, TypeScript, JavaScript, Java, C, C++, C#, Ruby, Kotlin, Swift, Scala, PHP, Lua, Zig, Elixir, Haskell, Bash, HCL) and provides 11 subcommands:

| Subcommand | What it does |
|------------|-------------|
| `ast list` | List symbol definitions in a file or directory |
| `ast read` | Read a specific symbol by name |
| `ast rename` | Rename identifiers, skipping strings and comments |
| `ast validate` | Check syntax of source files |
| `ast search` | Structural search using tree-sitter queries |
| `ast refs` | Find all references to a symbol across files |
| `ast deps` | Extract import/dependency statements |
| `ast map` | Generate a ranked repository map (PageRank) |
| `ast replace` | Replace text only within a specific symbol's body |
| `ast impact` | Transitive impact analysis of changing a symbol |
| `ast diff` | Structural diff between two file versions |

All subcommands support `--json` and `--jsonl` output. The `ast` feature is enabled by default; build with `--no-default-features` for a smaller binary without tree-sitter.

AST operations are also available as MCP tools (`ast_list`, `ast_read`, `ast_rename`, `ast_validate`) and in transaction plans.

## Post-write formatting

All write commands (`replace`, `create`, `append`, `md`, `doc`, `tidy`, `patch`) now accept a `--format` flag that runs a shell command after a successful `--apply`:

```bash
patchloom replace --from "old_fn" --to "new_fn" src/main.rs \
  --apply --format "cargo fmt --all"
```

The formatter runs after the file is written but before the command exits, so you get a clean result in one step. A `--format-timeout` flag (default 30s) prevents hanging formatters. Also works through `--confirm` interactive mode and in transaction plans via the `format` lifecycle step.

## file.append

New `append` command adds content to the end of an existing file:

```bash
patchloom append src/lib.rs --content "pub mod new_module;"
echo "new entry" | patchloom append CHANGELOG.md --stdin
```

Supports all standard modes (`--diff`, `--check`, `--apply`, `--confirm`), `--format` post-write hooks, and the `--json`/`--jsonl` output flags. Also available as the `file_append` MCP tool.

## Smarter error recovery for edits

The fallback module, previously built but unwired, is now integrated at three levels:

- **Similarity hints.** When a literal replace finds no matches, patchloom suggests similar strings using Jaro-Winkler scoring: `no matches for 'proccess_data' in main.rs (did you mean: process_data?)`.
- **Structural validation.** The MCP `replace_text` tool pre-validates edits to JSON, YAML, and TOML files, warning about broken syntax or unbalanced brackets before the replacement is applied.
- **Context-anchored matching.** The `replace_text` MCP tool and transaction plans now accept optional `before_context` and `after_context` parameters. When exact matching fails, patchloom uses surrounding lines as anchors to locate the intended edit target, recovering from whitespace drift or minor formatting changes.

## Word-boundary matching

New `--word-boundary` flag on `replace` prevents partial-word matches. `patchloom replace --from "File" --to "Document" --word-boundary` replaces `SetupFile` but not `BenchSetupFile`. Available on the CLI, in transaction plans, and via the MCP `replace_text` tool.

## Library API for downstream consumers

The `fallback` and `ast` modules now expose their core building blocks as public API, letting library consumers (e.g., bline) use patchloom's edit recovery and tree-sitter infrastructure directly.

**Fallback module** (`patchloom::fallback`):

| Function/type | What it does |
|---------------|-------------|
| `resolve_with_fallback()` | Full fallback chain: exact, anchor, similarity, structured error |
| `anchor_match()` | Anchor-based matching using surrounding context lines |
| `validate_edit()` | Pre-validate a replacement against JSON/YAML/TOML syntax |
| `AnchorMatchResult` | Return type with `matched_text`, `start_offset`, `strategy` |
| `MatchStrategy` | Enum: `Exact`, `Anchor`, `Similarity` |
| `ValidationResult` | Return type with `valid`, `errors`, `warnings` |

**AST module** (`patchloom::ast`, requires `features = ["ast"]`):

| Function | What it does |
|----------|-------------|
| `parse_source()` | Parse source code with tree-sitter (language detection + parser setup) |
| `ts_language_for()` | Map a `Language` to its tree-sitter grammar |
| `child_text_by_kind()` | Extract text from a child node by kind |
| `child_text_by_kinds()` | Extract text from a child node matching any of several kinds |

All high-level AST functions (`extract_symbols`, `search_query`, `validate_source`, `rename_in_source`, `find_refs_in_source`, `structural_diff`, `compute_impact`, `replace_in_symbol`, `generate_map`) were already public.

## Internal cleanup

- **ops.rs split.** The 4,369-line monolithic `src/ops.rs` was split into `src/ops/{doc,md,patch,replace}.rs`.
- **Transaction engine refactoring.** Extracted `build_full_tx_output` (eliminating 6x duplication), `validate_and_prepare_plan` (shared validation), and `execute_doc_op`/`execute_file_op` (breaking up a 460-line match).
- **MCP validator consolidation.** Unified `validate_content_size`/`validate_param_size` into a shared `validate_size` helper.
- **Visibility tightening.** `diff` and `exit` modules narrowed to `pub(crate)`. All 28 MCP param structs narrowed to `pub(crate)`.
- **Dead code removal.** Removed `DiffResult::total_files_changed` (set at 20+ sites but never read).

## Breaking changes

- `diff` and `exit` modules are no longer public. Code importing `patchloom::diff::*` or `patchloom::exit::*` must use the library API instead.
- `Operation::Replace` has three new fields: `word_boundary`, `before_context`, `after_context`. Code constructing this variant via struct literals must include them (use `..Default::default()` or add explicit values).
- `DiffResult::total_files_changed` field removed.
- Internal helper `truncate_str` in the fallback module narrowed from `pub` to `pub(crate)`.
- All MCP param structs narrowed from `pub` to `pub(crate)`.

## Test coverage

1,588 tests (887 unit + 694 integration + 7 PTY), up from 1,476 in v0.2.0.

## Links

- [Full changelog](https://github.com/patchloom/patchloom/compare/patchloom-v0.2.0...patchloom-v0.3.0)
- [Documentation](https://patchloom.github.io/patchloom/)
- [Library API docs](https://docs.rs/patchloom)
- [MCP setup guide](https://patchloom.github.io/patchloom/getting-started/mcp-setup.html)
