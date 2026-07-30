# Embedder host case study: prefer Patchloom library contracts

> **Audience:** product teams that embed Patchloom as a Rust crate (hosts such as multi-agent CLIs) instead of reimplementing search/replace, config edits, and peels.
> **APIs:** public surface as of 0.23+ (0.24 when released). No confidential host internals.

## The problem hosts reinvent

Without a shared library, agent hosts often grow a private edit stack:

- fuzzy or approximate replace after exact match fails
- multi-op batch apply with partial success
- path containment and binary/encoding soft-skip
- Morph-class freeform snippets with `// ... existing code ...` markers
- ad hoc undo

That glue tends to drift from CLI and MCP behavior, so the same agent prompt works in one surface and fails in another.

## Prefer these Patchloom host APIs

| Job | API |
|-----|-----|
| Agent-safe replace defaults | `ReplaceOptions::for_agent()` (`unique`, `require_change`, fuzzy floor, `allow_absent_old: false`, `refuse_suspicious_fuzzy`) |
| Classify errors for branching | `edit_error_kind` / `error_kind_str` / peels such as `is_binary`, `is_already_exists`, `is_not_found`, `is_guard_rejected` |
| Refuse over-wide fuzzy spans | `fuzzy_span_suspicious` / `FuzzySpanPolicy` |
| Multi-op buffer refuse before write | `refuse_batch_if_suspicious_fuzzy` |
| Multi-op content apply | `apply_content_edits` / `apply_content_edits_to_file` (+ span policy variant) |
| Morph-class fragment with anchor | `apply_fragment_to_file` (markers stripped; requires after/before/old) |
| Path-only binary/invalid UTF-8 rename/delete | library file ops with path-only honesty constructors |
| Atomic multi-file library write | finalize-before-mutate multi-write path (`write_if_apply_many` internally) |
| Plan/tx | `execute_plan` with optional `PathGuard` |

Scale: hosts that previously maintained a few hundred to a few thousand lines of private replace/path/peel glue can delete most of it once they pin a Patchloom version that includes these exports (exact line counts vary by host; the win is one contract across CLI, MCP, and library).

## Routing rules that reduce Morph / yq / FS MCP

1. **Configs / multi-doc YAML / TOML / JSON:** `doc_*` / `doc set`, not text replace or yq.
2. **Markdown structure:** `md_*`, not whole-file rewrite.
3. **Identifiers:** `ast rename` / project rename, not fuzzy text.
4. **Inventory:** MCP `list_files` / library walk helpers, not a second filesystem MCP.
5. **Freeform snippet with known anchor:** `apply_fragment_to_file` / CLI `apply-fragment`.
6. **Freeform with no anchors:** still Morph/IDE apply territory (non-goal for Patchloom).

## Verify before you pin

```bash
make embedder-smoke
make test-library-hygiene
cargo test --lib fuzzy_span refuse_batch --all-features
```

See also [comparisons](../getting-started/comparisons.md), [MCP setup](../getting-started/mcp-setup.md), and crate docs under `patchloom::api`.
