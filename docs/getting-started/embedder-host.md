# Embedder host checklist (Rust library)

For **LLM agent hosts / embedders** that call Patchloom as a library (not only
shell out to the CLI). Public API: [docs.rs/patchloom](https://docs.rs/patchloom).

This is the **one-screen** ordered checklist for a first host integration.
Bline is a real embedder that dogfooded these steps; the checklist is host-generic.

**Version dual-path:** a Cargo pin of patchloom (library) can be ahead of the
operator shell binary (`brew` / `scoop` / old `cargo install`). When dogfooding
CLI and library side by side, run `patchloom --version` and compare to
`Cargo.lock` before treating CLI exit codes as a library regression. See
[Installation: verify which binary](installation.md#verify-which-binary-and-crate-you-have).

**`doc set` vs `doc update`:** single concrete paths use `doc.set` / MCP
`doc_set`; predicate or wildcard multi-match uses `doc.update` / `doc_update`.
Do not expose only `doc_set` if agents need list updates by name. See
[Comparisons](comparisons.md#doc-set-vs-doc-update-agent-dx).

## Minimal checklist

1. **PathGuard** from the workspace root (and `allow_temp_directory` if the
   agent writes under `/tmp`). Pass `Some(&guard)` into Apply writers.
   Blank or whitespace-only paths fail closed as `ContainmentError::EmptyPath`
   (`path must not be empty`); they do not resolve as the workspace root.
   Match `ContainmentError` with a `_` arm: the enum is `#[non_exhaustive]`
   (0.28.0).
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
   `EditResult.backup_session` on success if the host exposes undo. On `Err`
   after write/backup (FormatFailed or fail-restore), use
   `api::backup_session_from_error(&err)` for the same session id without
   scraping English Display (#2127).
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
   (#2031). Both also handle FIFO/socket/device and symlinks (including
   dangling and symlink-to-dir) as directory-entry moves/unlinks without
   following the target; directories stay refused (#2087, #2091). Soft-loading a
   symlink as text then writing would rewrite the **target**; rename uses an
   empty path-only snapshot so write policies never mutate the link target.
   **Entry containment (#2115):** delete and path-only rename use
   `PathGuard::check_path_entry` (parent follows; final component does not).
   A workspace link whose target is outside the root can be unlinked or
   renamed under a workspace guard without parent-only host workarounds or
   `guard: None`. Content writes (`replace`, `doc_*`, append) still use
   follow-mode `check_path`. Append/prepend refuse non-text and special
   entries (including dangling symlinks) with `invalid_input` (not
   `not_found`). Sole explicit replace/search/tidy paths use the same
   classification via `sole_explicit_non_text` so agents do not mis-branch
   on `not_found` for a present non-file entry.
9. **Doc presentation honesty:** library `EditResult.style_changed` (and
   `is_style_changed`) mirrors CLI/MCP when YAML block-sequence layout
   collapses; values can still be correct (#2088). Warn hosts/agents; do not
   treat as failure.
10. **Morph-class freeform on disk:** `apply_fragment_to_file(path, fragment,
   FragmentPlacement::After|Before|Replace(...), unique, mode, guard)` strips
   lazy markers and applies via the replace path (#2032).
11. **Host unit tests for `op_honesty`:** `ContentEditHonesty` is
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

**Multi-file `apply_patch_file`:** preflights every hunk, then one backup
session for every path (including creates and rename destinations). Mid-batch
write failure restores the whole session (no orphan creates or half-renames).
Prefer this (or plan/`execute_plan` patch ops) over looping single-file
`apply_patch` when a unified diff touches multiple files.

## Plan `for_each` and lifecycle commands

`for_each` expansion requires the `files` feature (the CLI feature already
enables `files`). It does **not** require `cli`. Call
`api::expand_for_each(&mut plan, cwd)` before walking
`Operation::declared_paths()`, or rely on `execute_plan`, which expands
before PathGuard (#2169). Zero-match globs are `NoMatch`. Unparseable
`glob`/`exclude` and a `filter` other than `has_symbol(NAME)` are
`InvalidInput`. Do not combine `plan.cwd` with `for_each`.

Plan `format` / `validate` steps are raw shell (`sh -c` / `cmd /C`). MCP
`execute_plan` still strips them (#1142). Library hosts that pass
`Some(&PathGuard)` get an automatic refuse of redirects, pipelines, and
substitutions before commit (`EditErrorKind::GuardRejected`). Hosts that
call `execute_plan` with `None` should preflight:

```rust
use patchloom::api::{lifecycle_cmds, refuse_lifecycle_shell_metas};

for cmd in lifecycle_cmds(&plan) {
    refuse_lifecycle_shell_metas(cmd)?; // InvalidInput on `|`, `>`, `$`, …
}
```

`true`, `cargo fmt`, and `rustfmt` with no metas still run. This is not a
POSIX shell parser.

## Patch dest preflight

`parse_unified_diff` C-unescapes `---` / `+++` / rename / copy dests and
lists git-meta dests that apply refuses (binary payload, `Binary files
differ`, mode-only chmod). Hosts that deny secret names (`.env`) before
`apply_patch_file` must use the shared helpers, not quote-peel or
whitespace-split `diff --git`:

```rust
use patchloom::api::{
    parse_diff_file_path, parse_diff_git_paths, patch_declared_paths,
    unquote_git_c_string,
};

assert_eq!(unquote_git_c_string(r"\056env"), ".env");
assert_eq!(parse_diff_file_path(r#"+++ "b/\056env""#), ".env");
// Full line or the pair after `diff --git ` (prefix optional).
let (a, b) = parse_diff_git_paths(r#""a/notes.txt" "b/.env secret""#).unwrap();
assert_eq!((a.as_str(), b.as_str()), ("notes.txt", ".env secret"));
let dests = patch_declared_paths(diff_text)?;
```

Git 100% `copy from` / `copy to` creates the dest and keeps the source.
Dest-exists without force peels `AlreadyExists`. Mixed patches no longer
drop copy / binary / empty-create dests. Empty-create apply writes an
empty dest and reports `changed: true` (empty-to-empty is still a create).

## Related

- [Comparisons](comparisons.md) (Morph, filesystem MCP, yq, ast-grep)
- [Library API table](../reference/README.md#library-api)
- [Introduction: As a Rust library](../introduction.md#as-a-rust-library)
- Crate docs cookbook table in [docs.rs/patchloom](https://docs.rs/patchloom)
