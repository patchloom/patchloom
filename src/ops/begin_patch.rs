//! Codex `*** Begin Patch` grammar: detect, dest list, parse, hunk apply.
//!
//! Hosts dest-deny with [`begin_patch_declared_paths`] then call
//! [`crate::api::apply_patch`] / [`crate::api::apply_patch_file`]. Do not copy
//! this parser.

/// True when any line trims to `*** Begin Patch`.
#[must_use]
pub fn looks_like_begin_patch(patch: &str) -> bool {
    patch.lines().any(|l| l.trim() == "*** Begin Patch")
}

/// True when Begin Patch markers appear with unified-diff file headers.
#[must_use]
pub fn has_mixed_begin_patch_grammar(patch: &str) -> bool {
    if !looks_like_begin_patch(patch) {
        return false;
    }
    patch.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("diff --git ")
            || t.starts_with("--- a/")
            || t.starts_with("--- b/")
            || t.starts_with("+++ a/")
            || t.starts_with("+++ b/")
    })
}

/// One file operation from a Begin Patch payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginPatchOp {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        hunks: String,
        move_to: Option<String>,
    },
}

impl BeginPatchOp {
    /// Dest paths for dest-deny / PathGuard (Add/Update/Delete plus Move to).
    #[must_use]
    pub fn dests(&self) -> Vec<&str> {
        match self {
            Self::Add { path, .. } | Self::Delete { path } => vec![path.as_str()],
            Self::Update { path, move_to, .. } => {
                let mut out = vec![path.as_str()];
                if let Some(dest) = move_to {
                    out.push(dest.as_str());
                }
                out
            }
        }
    }
}

/// Every Add / Update / Delete / Move path in a Begin Patch document.
///
/// Parse errors are typed (`ParseErrorError`) so hosts can dest-deny before apply.
pub fn begin_patch_declared_paths(patch: &str) -> anyhow::Result<Vec<String>> {
    let ops = parse_begin_patch(patch)?;
    let mut out = Vec::new();
    for op in &ops {
        for dest in op.dests() {
            if !out.iter().any(|p| p == dest) {
                out.push(dest.to_owned());
            }
        }
    }
    Ok(out)
}

/// Parse a Codex Begin Patch document into file operations.
pub fn parse_begin_patch(patch: &str) -> anyhow::Result<Vec<BeginPatchOp>> {
    if has_mixed_begin_patch_grammar(patch) {
        return Err(parse_err(
            "mixed Begin Patch and unified diff grammar is not supported; \
             send one grammar per apply_patch call",
        ));
    }
    if !looks_like_begin_patch(patch) {
        return Err(parse_err("patch does not contain *** Begin Patch"));
    }

    let mut ops = Vec::new();
    let mut current: Option<OpBuilder> = None;
    let mut seen_begin = false;
    let mut seen_end = false;

    for line in patch.lines() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.trim() == "*** Begin Patch" {
            seen_begin = true;
            continue;
        }
        if !seen_begin {
            continue;
        }
        if trimmed.trim() == "*** End Patch" {
            finish_op(&mut current, &mut ops)?;
            seen_end = true;
            break;
        }
        if trimmed.trim() == "*** End of File" {
            finish_op(&mut current, &mut ops)?;
            continue;
        }
        if let Some(path) = strip_marker(trimmed, "*** Add File:") {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Add {
                path: path.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(path) = strip_marker(trimmed, "*** Delete File:") {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Delete {
                path: path.to_owned(),
            });
            continue;
        }
        if let Some(path) = strip_marker(trimmed, "*** Update File:") {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Update {
                path: path.to_owned(),
                hunks: Vec::new(),
                move_to: None,
            });
            continue;
        }
        if let Some(dest) = strip_marker(trimmed, "*** Move to:") {
            match &mut current {
                Some(OpBuilder::Update { move_to, .. }) => *move_to = Some(dest.to_owned()),
                _ => {
                    return Err(parse_err(
                        "*** Move to: is only valid after *** Update File:",
                    ));
                }
            }
            continue;
        }
        match &mut current {
            Some(OpBuilder::Add { lines, .. }) => {
                if let Some(rest) = trimmed.strip_prefix('+') {
                    lines.push(rest.to_owned());
                } else if trimmed.starts_with("***") {
                    return Err(parse_err(format!(
                        "unexpected Begin Patch marker: {trimmed}"
                    )));
                } else {
                    lines.push(trimmed.to_owned());
                }
            }
            Some(OpBuilder::Update { hunks, .. }) => {
                hunks.push(trimmed.to_owned());
            }
            Some(OpBuilder::Delete { .. }) => {
                if !trimmed.trim().is_empty() && !trimmed.starts_with('-') {
                    return Err(parse_err("*** Delete File: does not take hunk content"));
                }
            }
            None => {
                if !trimmed.trim().is_empty() && !trimmed.starts_with('#') {
                    return Err(parse_err(format!(
                        "Begin Patch content outside a file op: {trimmed}"
                    )));
                }
            }
        }
    }

    if !seen_end {
        return Err(parse_err("Begin Patch is missing *** End Patch"));
    }
    finish_op(&mut current, &mut ops)?;
    if ops.is_empty() {
        return Err(parse_err("Begin Patch contained no file operations"));
    }
    Ok(ops)
}

/// Apply Codex `@@` hunks (or a bare hunk) as unique exact replacements.
///
/// Matching is line-wise (same as unified-diff `apply_hunks`) so CRLF
/// sources match LF hunks and the file's EOL is preserved.
pub fn apply_codex_hunks(source: &str, hunks: &str) -> anyhow::Result<String> {
    let body = hunks.trim_start_matches('\n');
    if body.trim().is_empty() {
        return Ok(source.to_owned());
    }
    let hunk_chunks = split_hunks(body);
    if hunk_chunks.is_empty() {
        return Err(invalid_err("Begin Patch Update File has no @@ hunks"));
    }

    let eol = crate::write::detect_eol(source);
    let had_final_newline = source.ends_with('\n') || source.ends_with("\r\n") || source.is_empty();
    let mut src_lines: Vec<String> = source.lines().map(String::from).collect();

    for chunk in hunk_chunks {
        let (old_lines, new_lines) = hunk_old_new_lines(&chunk)?;
        if old_lines.is_empty() {
            if src_lines.is_empty() {
                src_lines = new_lines;
                continue;
            }
            return Err(invalid_err(
                "Begin Patch hunk has no context/delete lines to match",
            ));
        }
        let pos = find_unique_line_span(&src_lines, &old_lines)?;
        src_lines.splice(pos..pos + old_lines.len(), new_lines);
    }

    let mut out = src_lines.join(eol);
    if had_final_newline && !out.is_empty() {
        out.push_str(eol);
    }
    Ok(out)
}

/// `(path, use_entry)`. Delete and Move dest use entry PathGuard; Add and
/// Update source use follow.
#[must_use]
pub fn begin_patch_containment_checks(ops: &[BeginPatchOp]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for op in ops {
        match op {
            BeginPatchOp::Delete { path } => out.push((path.clone(), true)),
            BeginPatchOp::Add { path, .. } => out.push((path.clone(), false)),
            BeginPatchOp::Update { path, move_to, .. } => {
                out.push((path.clone(), false));
                if let Some(dest) = move_to {
                    out.push((dest.clone(), true));
                }
            }
        }
    }
    out
}

/// Remap a relative dest onto `file_hint` only when dest is that path or a suffix.
///
/// Same basename in another directory stays off the hint.
#[must_use]
pub fn resolve_begin_patch_dest(
    cwd: &std::path::Path,
    dest: &str,
    file_hint: Option<&std::path::Path>,
) -> std::path::PathBuf {
    let dest_path = std::path::Path::new(dest);
    if dest_path.is_absolute() {
        return dest_path.to_path_buf();
    }
    if let Some(hint) = file_hint
        && hint.ends_with(dest)
    {
        return hint.to_path_buf();
    }
    cwd.join(dest)
}

fn parse_err(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(crate::exit::ParseErrorError { msg: msg.into() })
}

fn invalid_err(msg: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(crate::exit::InvalidInputError { msg: msg.into() })
}

enum OpBuilder {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        hunks: Vec<String>,
        move_to: Option<String>,
    },
}

fn strip_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.trim()
        .strip_prefix(marker)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn finish_op(current: &mut Option<OpBuilder>, ops: &mut Vec<BeginPatchOp>) -> anyhow::Result<()> {
    let Some(builder) = current.take() else {
        return Ok(());
    };
    match builder {
        OpBuilder::Add { path, lines } => {
            let mut content = lines.join("\n");
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            ops.push(BeginPatchOp::Add { path, content });
        }
        OpBuilder::Delete { path } => ops.push(BeginPatchOp::Delete { path }),
        OpBuilder::Update {
            path,
            hunks,
            move_to,
        } => {
            let hunks = hunks.join("\n");
            ops.push(BeginPatchOp::Update {
                path,
                hunks,
                move_to,
            });
        }
    }
    Ok(())
}

fn split_hunks(body: &str) -> Vec<String> {
    let mut hunks = Vec::new();
    let mut current = Vec::new();
    let mut started = false;
    for line in body.lines() {
        if line.starts_with("@@") {
            if started && !current.is_empty() {
                hunks.push(current.join("\n"));
                current.clear();
            }
            started = true;
            continue;
        }
        if started {
            current.push(line.to_owned());
        } else if !line.trim().is_empty() {
            started = true;
            current.push(line.to_owned());
        }
    }
    if !current.is_empty() {
        hunks.push(current.join("\n"));
    }
    hunks
}

fn hunk_old_new_lines(hunk: &str) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    for line in hunk.lines() {
        if line.starts_with("@@") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('-') {
            old_lines.push(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix('+') {
            new_lines.push(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix(' ') {
            old_lines.push(rest.to_owned());
            new_lines.push(rest.to_owned());
        } else if line.is_empty() {
            old_lines.push(String::new());
            new_lines.push(String::new());
        } else {
            return Err(invalid_err(format!(
                "invalid Begin Patch hunk line (expected ' ', '-', or '+'): {line}"
            )));
        }
    }
    Ok((old_lines, new_lines))
}

fn find_unique_line_span(src: &[String], needle: &[String]) -> anyhow::Result<usize> {
    if needle.is_empty() || needle.len() > src.len() {
        return Err(crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::NoMatch,
            format!("Begin Patch hunk did not match file content: {needle:?}"),
        )
        .into());
    }
    let mut found = None;
    let n = needle.len();
    for i in 0..=src.len() - n {
        if src[i..i + n] == needle[..] {
            match found {
                None => found = Some(i),
                Some(_) => {
                    return Err(crate::fallback::EditError::new(
                        crate::fallback::EditErrorKind::AmbiguousTarget,
                        "Begin Patch hunk matched 2+ times; make the context unique",
                    )
                    .into());
                }
            }
        }
    }
    found.ok_or_else(|| {
        crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::NoMatch,
            format!("Begin Patch hunk did not match file content: {needle:?}"),
        )
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn looks_like_begin_patch_detects_marker() {
        assert!(looks_like_begin_patch("*** Begin Patch\n*** End Patch\n"));
        assert!(looks_like_begin_patch("  *** Begin Patch  \n"));
        assert!(!looks_like_begin_patch("--- a/x\n+++ b/x\n"));
    }

    #[test]
    fn parse_update_file() {
        let patch = "\
*** Begin Patch
*** Update File: code.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
";
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "code.rs");
                assert!(hunks.contains("-fn old() {}"));
                assert!(hunks.contains("+fn new() {}"));
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn begin_patch_declared_paths_lists_add_update_delete_move() {
        let patch = "\
*** Begin Patch
*** Add File: a.rs
+hello
*** Update File: b.rs
*** Move to: c.rs
@@
-old
+new
*** Delete File: d.rs
*** End Patch
";
        let paths = begin_patch_declared_paths(patch).expect("paths");
        assert_eq!(paths, vec!["a.rs", "b.rs", "c.rs", "d.rs"]);
    }

    #[test]
    fn resolve_dest_remaps_relative_hint_file_not_other_basename() {
        let hint = Path::new("/repo/crates/bline-cli/src/main.rs");
        let cwd = Path::new("/repo/crates/bline-cli/src");

        let same = resolve_begin_patch_dest(cwd, "main.rs", Some(hint));
        assert_eq!(same, hint, "relative dest that is the hint file must remap");

        let suffix = resolve_begin_patch_dest(cwd, "crates/bline-cli/src/main.rs", Some(hint));
        assert_eq!(suffix, hint, "dest that is a suffix of the hint must remap");

        let other = resolve_begin_patch_dest(cwd, "crates/bline-tools/src/main.rs", Some(hint));
        assert_eq!(
            other,
            cwd.join("crates/bline-tools/src/main.rs"),
            "same basename in a different directory must not collapse onto the hint"
        );

        let sibling = resolve_begin_patch_dest(cwd, "../tools/src/main.rs", Some(hint));
        assert_eq!(
            sibling,
            cwd.join("../tools/src/main.rs"),
            "relative dest with the same basename must stay off the hint"
        );
    }

    #[test]
    fn mixed_grammar_is_rejected() {
        let patch = "\
*** Begin Patch
*** Update File: code.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
--- a/other.rs
+++ b/other.rs
";
        let err = parse_begin_patch(patch).expect_err("mixed");
        assert!(err.to_string().to_lowercase().contains("mixed"));
        assert!(crate::exit::is_parse_error(&err));
    }

    #[test]
    fn missing_end_patch_is_error() {
        let patch = "*** Begin Patch\n*** Update File: a.rs\n@@\n-a\n+b\n";
        let err = parse_begin_patch(patch).expect_err("end");
        assert!(err.to_string().contains("End Patch"));
    }

    #[test]
    fn apply_codex_hunks_unique_and_bare() {
        let src = "fn old() {}\n";
        let out = apply_codex_hunks(src, "-fn old() {}\n+fn new() {}\n").expect("bare hunk");
        assert_eq!(out, "fn new() {}\n");
    }

    #[test]
    fn apply_codex_hunks_ambiguous() {
        let src = "x\nx\n";
        let err = apply_codex_hunks(src, "-x\n+y\n").expect_err("ambiguous");
        assert!(crate::fallback::is_ambiguous(&err));
    }

    #[test]
    fn apply_codex_hunks_no_match() {
        let src = "fn live() {}\n";
        let err = apply_codex_hunks(src, "-fn missing() {}\n+fn x() {}\n").expect_err("miss");
        assert!(crate::fallback::is_no_match(&err));
    }

    #[test]
    fn apply_codex_hunks_preserves_crlf() {
        let src = "fn old() {}\r\nfn keep() {}\r\n";
        let out = apply_codex_hunks(src, "-fn old() {}\n+fn new() {}\n").expect("crlf");
        assert_eq!(out, "fn new() {}\r\nfn keep() {}\r\n");
    }
}
