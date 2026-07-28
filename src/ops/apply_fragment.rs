//! Constrained freeform fragment apply (#2018).
//!
//! Covers Morph-class *jobs* (lazy snippets, freeform placement) without a
//! Morph clone: Morph-style `// ... existing code ...` markers are stripped,
//! but **placement anchors are required** and matching is fail-closed.

use crate::exit::InvalidInputError;

/// Placement for a cleaned fragment. Exactly one mode must be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentPlacement {
    /// Insert cleaned fragment immediately after the first unique match of `anchor`.
    After(String),
    /// Insert cleaned fragment immediately before the first unique match of `anchor`.
    Before(String),
    /// Replace the matched `old` span with the cleaned fragment.
    Replace(String),
}

/// Validated apply-fragment request (anchors required; no free guess).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyFragmentSpec {
    /// Optional human note (not used for merge).
    pub instruction: Option<String>,
    /// Fragment after Morph marker strip.
    pub fragment: String,
    /// Required placement.
    pub placement: FragmentPlacement,
    /// Enforce a single anchor match (default true for agents).
    pub unique: bool,
}

/// True when a line is a Morph-style lazy placeholder (not real source).
///
/// Only **comment-shaped** whole lines can be markers (avoids stripping Python
/// `...`, string literals, or code with trailing notes). Recognized forms:
/// - `// ... existing code ...` / `// ...existing code...`
/// - `# ... existing code ...`
/// - `/* ... existing code ... */`
/// - pure ellipsis comments: `// ...` / `# ...`
pub fn is_lazy_marker_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    // Whole line must be a comment first.
    let core = if (lower.starts_with("/*") && lower.ends_with("*/"))
        || (lower.starts_with("/**") && lower.ends_with("*/"))
    {
        lower
            .trim_start_matches("/**")
            .trim_start_matches("/*")
            .trim_end_matches("*/")
            .trim()
            .to_string()
    } else if lower.starts_with("//") {
        lower.trim_start_matches("//").trim().to_string()
    } else if lower.starts_with('#') {
        lower.trim_start_matches('#').trim().to_string()
    } else {
        return false;
    };
    if core.is_empty() {
        return false;
    }
    let has_ellipsis = core.contains("...") || core.contains('\u{2026}');
    if !has_ellipsis {
        return false;
    }
    if core.contains("existing code") || core.contains("existingcode") {
        return true;
    }
    let alnum: String = core.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    alnum.is_empty() || alnum == "existingcode" || alnum == "code"
}

fn dominant_eol(s: &str) -> &'static str {
    let crlf = s.matches("\r\n").count();
    let lf_only = s.as_bytes().iter().filter(|&&b| b == b'\n').count() - crlf;
    let cr_only = s.as_bytes().iter().filter(|&&b| b == b'\r').count() - crlf;
    if crlf >= lf_only && crlf >= cr_only && crlf > 0 {
        "\r\n"
    } else if cr_only > lf_only && cr_only > 0 {
        "\r"
    } else {
        "\n"
    }
}

/// Remove Morph-style lazy marker lines; preserve other content and dominant EOL.
///
/// Does **not** invent placement. Empty result after strip is an error at the
/// call site (invalid_input).
pub fn strip_lazy_markers(fragment: &str) -> String {
    if fragment.is_empty() {
        return String::new();
    }
    let eol = dominant_eol(fragment);
    let ends_with_eol = fragment.ends_with("\r\n")
        || (!fragment.ends_with("\r\n") && fragment.ends_with('\n'))
        || fragment.ends_with('\r');
    let mut out_lines: Vec<String> = Vec::new();
    // Normalize split to logical lines without dropping empty ones incorrectly.
    let normalized = fragment.replace("\r\n", "\n").replace('\r', "\n");
    let had_trailing = normalized.ends_with('\n');
    let body = normalized.strip_suffix('\n').unwrap_or(normalized.as_str());
    for line in body.split('\n') {
        if is_lazy_marker_line(line) {
            continue;
        }
        out_lines.push(line.to_string());
    }
    let mut s = out_lines.join(eol);
    if (ends_with_eol || had_trailing) && !s.is_empty() && !s.ends_with(eol) {
        s.push_str(eol);
    }
    s
}

/// Build a validated spec from raw Morph-style inputs.
///
/// Exactly one of `after`, `before`, `old` must be `Some` and non-empty after trim.
/// `fragment` is stripped of lazy markers and must remain non-empty.
pub fn build_apply_fragment_spec(
    fragment: &str,
    instruction: Option<&str>,
    after: Option<&str>,
    before: Option<&str>,
    old: Option<&str>,
    unique: bool,
) -> Result<ApplyFragmentSpec, InvalidInputError> {
    let cleaned = strip_lazy_markers(fragment);
    if cleaned.trim().is_empty() {
        return Err(InvalidInputError {
            msg: "apply.fragment: fragment empty after stripping lazy markers \
(// ... existing code ...); provide real new text"
                .into(),
        });
    }

    // Preserve anchor whitespace (indentation is significant for exact match).
    let after = after.filter(|s| !s.trim().is_empty()).map(str::to_string);
    let before = before.filter(|s| !s.trim().is_empty()).map(str::to_string);
    let old = old.filter(|s| !s.trim().is_empty()).map(str::to_string);

    let n =
        usize::from(after.is_some()) + usize::from(before.is_some()) + usize::from(old.is_some());
    if n == 0 {
        return Err(InvalidInputError {
            msg: "apply.fragment: require exactly one placement anchor \
(--after / after, --before / before, or --old / old); no Morph-style guess without anchors (#2018)"
                .into(),
        });
    }
    if n > 1 {
        return Err(InvalidInputError {
            msg:
                "apply.fragment: after, before, and old are mutually exclusive; pick one placement"
                    .into(),
        });
    }

    let placement = match (after, before, old) {
        (Some(a), None, None) => FragmentPlacement::After(a),
        (None, Some(b), None) => FragmentPlacement::Before(b),
        (None, None, Some(o)) => FragmentPlacement::Replace(o),
        _ => {
            return Err(InvalidInputError {
                msg: "apply.fragment: internal placement error".into(),
            });
        }
    };

    Ok(ApplyFragmentSpec {
        instruction: instruction
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        fragment: cleaned,
        placement,
        unique,
    })
}

/// Desugar a validated spec into replace `old` + optional insert_after/before + new text.
///
/// Maps onto the existing replace write path (no parallel writer).
pub fn desugar_to_replace_fields(spec: &ApplyFragmentSpec) -> DesugaredReplace {
    match &spec.placement {
        FragmentPlacement::After(anchor) => DesugaredReplace {
            old: anchor.clone(),
            new_text: None,
            insert_after: Some(spec.fragment.clone()),
            insert_before: None,
            unique: spec.unique,
        },
        FragmentPlacement::Before(anchor) => DesugaredReplace {
            old: anchor.clone(),
            new_text: None,
            insert_after: None,
            insert_before: Some(spec.fragment.clone()),
            unique: spec.unique,
        },
        FragmentPlacement::Replace(old) => DesugaredReplace {
            old: old.clone(),
            new_text: Some(spec.fragment.clone()),
            insert_after: None,
            insert_before: None,
            unique: spec.unique,
        },
    }
}

/// Fields for building `Operation::Replace` / `replace_text` from apply.fragment.
#[derive(Debug, Clone)]
pub struct DesugaredReplace {
    pub old: String,
    pub new_text: Option<String>,
    pub insert_after: Option<String>,
    pub insert_before: Option<String>,
    pub unique: bool,
}

/// Build a plan [`crate::plan::Operation::Replace`] from a validated apply.fragment spec.
pub fn desugar_to_replace_operation(
    path: &str,
    spec: &ApplyFragmentSpec,
) -> crate::plan::Operation {
    let d = desugar_to_replace_fields(spec);
    crate::plan::Operation::Replace {
        glob: None,
        path: Some(path.to_string()),
        regex: false,
        old: d.old,
        new_text: d.new_text,
        nth: None,
        insert_before: d.insert_before,
        insert_after: d.insert_after,
        case_insensitive: false,
        multiline: false,
        if_exists: false,
        whole_line: false,
        range: None,
        word_boundary: false,
        before_context: None,
        after_context: None,
        unique: d.unique,
        require_change: true,
        command_position: false,
        fuzzy: false,
        min_fuzzy_score: None,
        allow_absent_old: false,
    }
}

/// Validate raw apply.fragment plan fields and desugar to `Operation::Replace`.
pub fn plan_apply_fragment_to_replace(
    path: &str,
    fragment: &str,
    instruction: Option<&str>,
    after: Option<&str>,
    before: Option<&str>,
    old: Option<&str>,
    unique: bool,
) -> Result<crate::plan::Operation, InvalidInputError> {
    let spec = build_apply_fragment_spec(fragment, instruction, after, before, old, unique)?;
    Ok(desugar_to_replace_operation(path, &spec))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_morph_marker_lines() {
        let raw = "// ... existing code ...\nfn new() {}\n// ... existing code ...\n";
        let cleaned = strip_lazy_markers(raw);
        assert_eq!(cleaned, "fn new() {}\n");
    }

    #[test]
    fn strips_hash_and_block_markers() {
        assert!(is_lazy_marker_line("# ... existing code ..."));
        assert!(is_lazy_marker_line("/* ... existing code ... */"));
        assert!(is_lazy_marker_line("  // ...existing code...  "));
        assert!(is_lazy_marker_line("// ..."));
        assert!(!is_lazy_marker_line("fn main() { /* real */ }"));
        assert!(!is_lazy_marker_line("let x = 1; // keep comment"));
        // Must not strip non-comment code (reviewer #2021)
        assert!(!is_lazy_marker_line("    ..."));
        assert!(!is_lazy_marker_line(
            r#"const s = "... existing code ...";"#
        ));
        assert!(!is_lazy_marker_line("x = 1  # ... existing code ..."));
    }

    #[test]
    fn strip_preserves_crlf() {
        let raw = "// ... existing code ...\r\nfn new() {}\r\n// ... existing code ...\r\n";
        let cleaned = strip_lazy_markers(raw);
        assert_eq!(cleaned, "fn new() {}\r\n");
    }

    #[test]
    fn build_requires_anchor() {
        let err = build_apply_fragment_spec("fn x() {}", None, None, None, None, true).unwrap_err();
        assert!(err.msg.contains("placement anchor"), "{msg}", msg = err.msg);
    }

    #[test]
    fn build_rejects_empty_after_strip() {
        let err = build_apply_fragment_spec(
            "// ... existing code ...\n",
            None,
            Some("a"),
            None,
            None,
            true,
        )
        .unwrap_err();
        assert!(
            err.msg.contains("empty after stripping"),
            "{msg}",
            msg = err.msg
        );
    }

    #[test]
    fn build_after_ok() {
        let s = build_apply_fragment_spec(
            "// ... existing code ...\n  b();\n// ... existing code ...\n",
            Some("add b call"),
            Some("  a();"),
            None,
            None,
            true,
        )
        .unwrap();
        assert_eq!(s.fragment.trim(), "b();");
        assert_eq!(s.placement, FragmentPlacement::After("  a();".into()));
        assert_eq!(s.instruction.as_deref(), Some("add b call"));
    }

    #[test]
    fn build_mutual_exclusion() {
        let err =
            build_apply_fragment_spec("x", None, Some("a"), Some("b"), None, true).unwrap_err();
        assert!(
            err.msg.contains("mutually exclusive"),
            "{msg}",
            msg = err.msg
        );
    }

    #[test]
    fn build_replace_mode() {
        let s = build_apply_fragment_spec("NEW", None, None, None, Some("OLD"), true).unwrap();
        assert_eq!(s.placement, FragmentPlacement::Replace("OLD".into()));
        assert_eq!(s.fragment, "NEW");
    }

    #[test]
    fn desugar_after_maps_to_insert_after() {
        let s = build_apply_fragment_spec("NEW\n", None, Some("ANCHOR"), None, None, true).unwrap();
        let d = desugar_to_replace_fields(&s);
        assert_eq!(d.old, "ANCHOR");
        assert_eq!(d.insert_after.as_deref(), Some("NEW\n"));
        assert!(d.insert_before.is_none());
        assert!(d.new_text.is_none());
        assert!(d.unique);
    }

    #[test]
    fn plan_apply_fragment_to_replace_op_name() {
        let op = plan_apply_fragment_to_replace(
            "f.rs",
            "// ... existing code ...\nx\n",
            None,
            Some("y"),
            None,
            None,
            true,
        )
        .unwrap();
        match op {
            crate::plan::Operation::Replace {
                path,
                old,
                insert_after,
                new_text,
                unique,
                require_change,
                ..
            } => {
                assert_eq!(path.as_deref(), Some("f.rs"));
                assert_eq!(old, "y");
                assert_eq!(insert_after.as_deref(), Some("x\n"));
                assert!(new_text.is_none());
                assert!(unique);
                assert!(require_change);
            }
            other => panic!("expected Replace, got {other:?}"),
        }
    }
}
