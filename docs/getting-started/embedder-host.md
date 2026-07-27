# Embedder host checklist (Rust library)

For agent runtimes that **embed** Patchloom (not only shell out to the CLI). Public API surface: [docs.rs/patchloom](https://docs.rs/patchloom).

## Host checklist

1. **Shared replace policy:** call `ReplaceOptions::for_agent()` on every primary and fallback replace path (`unique`, `require_change`, fuzzy at `AGENT_MIN_FUZZY_SCORE` 0.90, `allow_absent_old: false`, `refuse_suspicious_fuzzy: true`).
2. **Branch on peels:** use `edit_error_kind` / `error_kind_str` / bools (`is_no_match`, `is_binary`, `is_already_exists`, `is_fuzzy_span_suspicious`, …). Keep a `_` arm (`EditErrorKind` is `#[non_exhaustive]`).
3. **Over-wide fuzzy:** with `for_agent()`, refuse is automatic (`EditErrorKind::FuzzySpanSuspicious`). For custom options, call `fuzzy_span_suspicious(old, matched_text, match_score)` (or `FuzzySpanPolicy`) before trusting Apply.
4. **Containment:** for sandboxed agents, use `PathGuard` / workspace roots; do not let the model widen `--cwd` under CLI contain (#1832).
5. **Undo:** after Apply, persist `EditResult.backup_session` if the host exposes undo.

## Minimal sketch

```rust
use patchloom::api::{
    replace_in_content, ReplaceOptions, edit_error_kind, EditErrorKind,
    is_fuzzy_span_suspicious,
};

let opts = ReplaceOptions::for_agent();
match replace_in_content(content, old, new, &opts) {
    Ok(r) => Ok(r), // refuse_suspicious_fuzzy already ran for fuzzy
    Err(e) => {
        if is_fuzzy_span_suspicious(&e) {
            return Err(e); // over-wide fuzzy
        }
        match edit_error_kind(&e) {
            Some(EditErrorKind::NoMatch) => Err(e), // agent retry or skip
            Some(EditErrorKind::Binary) => Err(e),
            _ => Err(e),
        }
    }
}
```

Approximate recovery stays an explicit override: `ReplaceOptions { allow_absent_old: true, ..ReplaceOptions::for_agent() }` (not a second constructor; #1980). To keep approximate recovery without span auto-refuse: also set `refuse_suspicious_fuzzy: false`.

## Multi-op

`apply_content_edits` rolls up the **widest** `matched_text` and the **minimum** fuzzy score independently (may be different ops). Prefer **`ContentEditsResult.op_honesty`**: each replace entry has `old`, `matched_text`, and `match_score` for correct refuse pairing (#2006). When each replace uses `for_agent()`, over-wide fuzzy fails inside that op (all-or-nothing batch).

Plan/tx multi-path top-level honesty matches that worst-case rollup (#2007). For disk multi-op with a final gate (hardlink/backup write path), use `apply_content_edits_to_file_with_span_policy(path, edits, mode, guard, Some(&FuzzySpanPolicy::default()))` so refuse happens **before** write (#2008).

## Related

- [Comparisons](comparisons.md) (Morph, filesystem MCP, yq, ast-grep)
- [Library API table](../reference/README.md#library-api)
- [Introduction: As a Rust library](../introduction.md#as-a-rust-library)
