# Embedder host checklist (Rust library)

For **LLM agent hosts / embedders** that call Patchloom as a library (not only
shell out to the CLI). Public API: [docs.rs/patchloom](https://docs.rs/patchloom).

This is the **one-screen** ordered checklist for a first host integration.
Bline is a real embedder that dogfooded these steps; the checklist is host-generic.

## Minimal checklist

1. **PathGuard** from the workspace root (and `allow_temp_directory` if the
   agent writes under `/tmp`). Pass `Some(&guard)` into Apply writers.
2. **Sole-path text load:** `api::load_text` / `load_text_strict` (or
   `is_binary_file` preflight). Peel `Binary` / `InvalidEncoding` /
   `is_load_text_strict_fail` instead of scraping English.
3. **Dual-path replace (primary + fallback):** call
   `ReplaceOptions::for_agent()` in **both** places. Do not hand-copy
   `ReplaceOptions { ... }` twice (options drift is a common footgun).
   Preset: `unique`, `require_change`, fuzzy at `AGENT_MIN_FUZZY_SCORE` (0.90),
   `allow_absent_old: false`, `refuse_suspicious_fuzzy: true`.
4. **Over-wide fuzzy:** with `for_agent()`, refuse is automatic
   (`EditErrorKind::FuzzySpanSuspicious` / `is_fuzzy_span_suspicious`). For
   custom options, call `fuzzy_span_suspicious(old, matched_text, score)` or
   `fuzzy_span_suspicious_with_policy` + `FuzzySpanPolicy` before trusting Apply.
5. **On `Err`:** branch with `edit_error_kind` / `error_kind_str` / `is_*`
   peels. Keep a `_` arm: `EditErrorKind` is `#[non_exhaustive]`.
6. **Apply writers:** prefer `api` file_* / `replace_text` /
   `apply_content_edits_to_file` (hardlink preserve, `backup_session`). Persist
   `EditResult.backup_session` if the host exposes undo.
7. **Multi-op / multi-path honesty:**
   - Buffer multi-op: `apply_content_edits` + **`ContentEditsResult.op_honesty`**
     (per-replace `old` + `matched_text` + `match_score`; #2006).
   - Buffer multi-op with host-owned write: after a successful batch, call
     `refuse_batch_if_suspicious_fuzzy(&batch, &FuzzySpanPolicy::default())`
     before trusting `batch.modified` (#2064). Same kind as single-op refuse;
     only Fuzzy honesty rows are checked. Prefer this over reimplementing the
     loop with `fuzzy_span_suspicious` alone.
   - Plan/tx multi-path: top-level **widest** `matched_text` + **min** fuzzy
     score (#2007); pair refuse with `changes[]` / plan `old`, not unpaired
     rollup fields.
   - Disk multi-op with a final gate:  
     `apply_content_edits_to_file_with_span_policy(path, edits, mode, guard, Some(&FuzzySpanPolicy::default()))`  
     refuses over-wide fuzzy **before** write/backup (#2008).
8. **Path-only file ops on non-text:** `file_rename` / `file_delete` succeed on
   binary and invalid UTF-8 with byte backup and PathGuard (no OS dual-path)
   (#2031). Append/prepend/replace still refuse non-text loads.
9. **Morph-class freeform on disk:** `apply_fragment_to_file(path, fragment,
   FragmentPlacement::After|Before|Replace(...), unique, mode, guard)` strips
   lazy markers and applies via the replace path (#2032).
10. **Host unit tests for `op_honesty`:** `ContentEditHonesty` is
    `#[non_exhaustive]`; use `ContentEditHonesty::exact` / `::fuzzy` instead of
    struct literals (#2033). Prefer live `apply_content_edits` for integration tests.

## Minimal sketch

```rust
use patchloom::api::{
    replace_in_content, ReplaceOptions, edit_error_kind, EditErrorKind,
    is_fuzzy_span_suspicious, ApplyMode, apply_content_edits,
    apply_content_edits_to_file_with_span_policy, refuse_batch_if_suspicious_fuzzy,
    ContentEdit, FuzzySpanPolicy,
};
use patchloom::containment::PathGuard;
use std::path::Path;

// 1) Containment (optional but recommended for sandboxed agents)
let guard = PathGuard::builder(std::env::current_dir()?).build()?;

// 2–5) Dual-path replace: same for_agent() on primary AND fallback call sites
let opts = ReplaceOptions::for_agent();
match replace_in_content(content, old, new, &opts) {
    Ok(r) => { /* refuse_suspicious_fuzzy already ran for fuzzy */ }
    Err(e) => {
        if is_fuzzy_span_suspicious(&e) {
            return Err(e); // over-wide fuzzy
        }
        match edit_error_kind(&e) {
            Some(EditErrorKind::NoMatch) => return Err(e),
            Some(EditErrorKind::Binary) => return Err(e),
            Some(_) | None => return Err(e), // non_exhaustive: keep _
        }
    }
}

// 6–7a) Disk multi-op Apply with optional pre-write span policy
let edits = [ContentEdit::Replace {
    old: old.into(),
    new: new.into(),
    options: ReplaceOptions::for_agent(),
}];
let policy = FuzzySpanPolicy::default();
let _ = apply_content_edits_to_file_with_span_policy(
    Path::new("notes.txt"),
    &edits,
    ApplyMode::Apply,
    Some(&guard),
    Some(&policy),
)?;

// 6–7b) Buffer multi-op + host write: public batch refuse (#2064)
let batch = apply_content_edits("notes body", &edits)?;
refuse_batch_if_suspicious_fuzzy(&batch, &policy)?;
// host write of batch.modified (hardlinks, custom backup, …)
```

Approximate recovery stays an explicit override:

```rust
ReplaceOptions {
    allow_absent_old: true,
    ..ReplaceOptions::for_agent()
}
```

(not a second constructor; #1980). To keep approximate recovery without span
auto-refuse: also set `refuse_suspicious_fuzzy: false`.

## Multi-op notes

`apply_content_edits` rolls up the **widest** `matched_text` and the **minimum**
fuzzy score independently (may be different ops). Prefer **`op_honesty`** for
refuse pairing, or call **`refuse_batch_if_suspicious_fuzzy`** for the full
batch gate (#2064). When each replace uses `for_agent()`, over-wide fuzzy fails
inside that op (all-or-nothing batch). Plan/tx multi-path top-level honesty
matches that worst-case rollup (#2007).

## Related

- [Comparisons](comparisons.md) (Morph, filesystem MCP, yq, ast-grep)
- [Library API table](../reference/README.md#library-api)
- [Introduction: As a Rust library](../introduction.md#as-a-rust-library)
- Crate docs cookbook table in [docs.rs/patchloom](https://docs.rs/patchloom)
