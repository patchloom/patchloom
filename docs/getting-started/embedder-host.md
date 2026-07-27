# Embedder host checklist (Rust library)

For agent runtimes that **embed** Patchloom (not only shell out to the CLI). Public API surface: [docs.rs/patchloom](https://docs.rs/patchloom).

## Host checklist

1. **Shared replace policy:** call `ReplaceOptions::for_agent()` on every primary and fallback replace path (`unique`, `require_change`, fuzzy at `AGENT_MIN_FUZZY_SCORE` 0.90, `allow_absent_old: false`).
2. **Branch on peels:** use `edit_error_kind` / `error_kind_str` / bools (`is_no_match`, `is_binary`, `is_already_exists`, …). Do not treat all failures as one string.
3. **After fuzzy Apply:** call `fuzzy_span_suspicious(old, matched_text, match_score)` (or `FuzzySpanPolicy`) before treating the write as trusted. Patchloom does **not** auto-refuse over-wide spans; the host must.
4. **Containment:** for sandboxed agents, use `PathGuard` / workspace roots; do not let the model widen `--cwd` under CLI contain (#1832).
5. **Undo:** after Apply, persist `EditResult.backup_session` if the host exposes undo.

## Minimal sketch

```rust
use patchloom::api::{
    replace_in_content, ReplaceOptions, fuzzy_span_suspicious,
    edit_error_kind, EditErrorKind, MatchMode, AGENT_MIN_FUZZY_SCORE,
};

let opts = ReplaceOptions::for_agent();
match replace_in_content(content, old, new, &opts) {
    Ok(r) => {
        if r.match_mode == Some(MatchMode::Fuzzy)
            && fuzzy_span_suspicious(old, r.matched_text.as_deref(), r.match_score)
        {
            // Host refuse: do not commit this buffer / surface to the model
            return Err(/* host policy */);
        }
        Ok(r)
    }
    Err(e) => match edit_error_kind(&e) {
        Some(EditErrorKind::NoMatch) => { /* agent retry or skip */ Err(e) }
        Some(EditErrorKind::Binary) => { /* peel */ Err(e) }
        _ => Err(e),
    },
}
```

Approximate recovery stays an explicit override: `ReplaceOptions { allow_absent_old: true, ..ReplaceOptions::for_agent() }` (not a second constructor; #1980).

## Multi-op

`apply_content_edits` rolls up the **widest** `matched_text` and the **minimum** fuzzy score independently. Prefer per-op checks with the matching `old` when possible.

## Related

- [Comparisons](comparisons.md) (Morph, filesystem MCP, yq, ast-grep)
- [Library API table](../reference/README.md#library-api)
- [Introduction: As a Rust library](../introduction.md#as-a-rust-library)
