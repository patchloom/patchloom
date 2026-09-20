//! Codex `*** Begin Patch` grammar: detect, dest list, parse, hunk apply.
//!
//! Hosts dest-deny with [`begin_patch_declared_paths`] then call
//! [`crate::api::apply_patch`] / [`crate::api::apply_patch_file`]. Do not copy
//! this parser.

/// True when the first non-blank line trims to `*** Begin Patch` with optional trailing ` ***`.
///
/// Mid-document or unified-diff context/`+` lines that mention the marker
/// stay unified (#2505).
#[must_use]
pub fn looks_like_begin_patch(patch: &str) -> bool {
    patch
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .is_some_and(|l| peel_trailing_marker_stars(l) == "*** Begin Patch")
}

/// True when Begin Patch markers appear with unified-diff file headers.
#[must_use]
pub fn has_mixed_begin_patch_grammar(patch: &str) -> bool {
    if !looks_like_begin_patch(patch) {
        return false;
    }
    patch
        .lines()
        .any(crate::ops::patch::line_looks_like_unified_file_header)
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
        // Markers must start at column 0. Do not left-trim (#2503).
        if is_col0_marker(trimmed, "*** Begin Patch") {
            seen_begin = true;
            continue;
        }
        if !seen_begin {
            continue;
        }
        if is_col0_marker(trimmed, "*** End Patch") {
            finish_op(&mut current, &mut ops)?;
            seen_end = true;
            break;
        }
        // `*** End of File` is a hunk EOF-anchor, not an op terminator (#2504).
        if let Some(path) = take_marker_dest(trimmed, "*** Add File:")? {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Add {
                path: path.to_owned(),
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(path) = take_marker_dest(trimmed, "*** Delete File:")? {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Delete {
                path: path.to_owned(),
            });
            continue;
        }
        if let Some(path) = take_marker_dest(trimmed, "*** Update File:")? {
            finish_op(&mut current, &mut ops)?;
            current = Some(OpBuilder::Update {
                path: path.to_owned(),
                hunks: Vec::new(),
                move_to: None,
            });
            continue;
        }
        if let Some(dest) = take_marker_dest(trimmed, "*** Move to:")? {
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
    let hunk_chunks = parse_codex_hunks(body)?;
    if hunk_chunks.is_empty() {
        return Err(invalid_err("Begin Patch Update File has no @@ hunks"));
    }

    let eol = crate::write::detect_eol(source);
    let had_final_newline = source.ends_with('\n') || source.ends_with("\r\n") || source.is_empty();
    let mut src_lines: Vec<String> = source.lines().map(String::from).collect();

    for chunk in hunk_chunks {
        apply_one_hunk(&mut src_lines, &chunk)?;
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

/// Peel optional trailing Codex token ` ***` (space + three asterisks).
fn peel_trailing_marker_stars(line: &str) -> &str {
    let t = line.trim_end();
    t.strip_suffix(" ***").map(str::trim_end).unwrap_or(t)
}

fn is_col0_marker(line: &str, marker: &str) -> bool {
    peel_trailing_marker_stars(line) == marker
}

fn strip_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    peel_trailing_marker_stars(line)
        .strip_prefix(marker)
        .map(str::trim)
}

fn take_marker_dest<'a>(line: &'a str, marker: &str) -> anyhow::Result<Option<&'a str>> {
    let Some(path) = strip_marker(line, marker) else {
        return Ok(None);
    };
    if path.is_empty() {
        return Err(invalid_err(format!("{marker} dest path must not be empty")));
    }
    Ok(Some(path))
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

struct CodexHunk {
    hint: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
    eof_anchored: bool,
}

fn parse_codex_hunks(body: &str) -> anyhow::Result<Vec<CodexHunk>> {
    let mut hunks = Vec::new();
    let mut current = Vec::new();
    let mut hint = None;
    let mut started = false;
    let mut eof_anchored = false;

    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if is_col0_marker(line, "*** End of File") {
            eof_anchored = true;
            continue;
        }
        if line.starts_with("@@") {
            if started {
                let (old_lines, new_lines) = hunk_old_new_lines(&current.join("\n"))?;
                hunks.push(CodexHunk {
                    hint,
                    old_lines,
                    new_lines,
                    eof_anchored,
                });
                current.clear();
                eof_anchored = false;
            }
            started = true;
            let rest = line.get(2..).unwrap_or("").trim();
            hint = if rest.is_empty() {
                None
            } else {
                Some(rest.to_owned())
            };
            continue;
        }
        if started {
            current.push(line.to_owned());
        } else if !line.trim().is_empty() {
            started = true;
            current.push(line.to_owned());
        }
    }
    if started {
        let (old_lines, new_lines) = hunk_old_new_lines(&current.join("\n"))?;
        hunks.push(CodexHunk {
            hint,
            old_lines,
            new_lines,
            eof_anchored,
        });
    }
    Ok(hunks)
}

fn apply_one_hunk(src: &mut Vec<String>, chunk: &CodexHunk) -> anyhow::Result<()> {
    let (search_start, search_end) = if let Some(hint) = &chunk.hint {
        let hint_idx = find_unique_hint(src, hint)?;
        let start = hint_idx + 1;
        let end = scope_end(src, start, leading_indent(&src[hint_idx]));
        (start, end)
    } else {
        (0, src.len())
    };

    if chunk.old_lines.is_empty() {
        if src.is_empty() {
            *src = chunk.new_lines.clone();
            return Ok(());
        }
        if chunk.hint.is_some() {
            src.splice(search_start..search_start, chunk.new_lines.iter().cloned());
            return Ok(());
        }
        return Err(invalid_err(
            "Begin Patch hunk has no context/delete lines to match",
        ));
    }

    let window = &src[search_start..search_end];
    let rel = find_line_span(window, &chunk.old_lines, chunk.eof_anchored)?;
    let pos = search_start + rel;
    src.splice(
        pos..pos + chunk.old_lines.len(),
        chunk.new_lines.iter().cloned(),
    );
    Ok(())
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn line_matches_hint(line: &str, hint: &str) -> bool {
    let n_line = normalize_ws(line);
    let n_hint = normalize_ws(hint);
    !n_hint.is_empty() && (n_line == n_hint || n_line.contains(&n_hint))
}

fn find_unique_hint(src: &[String], hint: &str) -> anyhow::Result<usize> {
    let mut found = None;
    for (i, line) in src.iter().enumerate() {
        if line_matches_hint(line, hint) {
            match found {
                None => found = Some(i),
                Some(_) => {
                    return Err(crate::fallback::EditError::new(
                        crate::fallback::EditErrorKind::AmbiguousTarget,
                        "Begin Patch @@ scope matched 2+ times; make the context unique",
                    )
                    .into());
                }
            }
        }
    }
    found.ok_or_else(|| {
        crate::fallback::EditError::new(
            crate::fallback::EditErrorKind::NoMatch,
            format!("Begin Patch @@ scope did not match file content: {hint:?}"),
        )
        .into()
    })
}

fn leading_indent(s: &str) -> usize {
    s.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

fn scope_end(src: &[String], after: usize, hint_indent: usize) -> usize {
    src.iter()
        .enumerate()
        .skip(after)
        .find(|(_, line)| !line.trim().is_empty() && leading_indent(line) <= hint_indent)
        .map(|(i, _)| i)
        .unwrap_or(src.len())
}

fn find_line_span(src: &[String], needle: &[String], eof: bool) -> anyhow::Result<usize> {
    if eof && !needle.is_empty() && needle.len() <= src.len() {
        let at = src.len() - needle.len();
        if src[at..] == needle[..] {
            return Ok(at);
        }
    }
    find_unique_line_span(src, needle)
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
        assert!(looks_like_begin_patch(
            "\n\n*** Begin Patch\n*** End Patch\n"
        ));
        assert!(!looks_like_begin_patch("--- a/x\n+++ b/x\n"));
        // Mid-document trimmed match is not an envelope (#2505).
        assert!(!looks_like_begin_patch(
            "preamble\n*** Begin Patch\n*** End Patch\n"
        ));
        assert!(
            !looks_like_begin_patch(
                "--- a/u.txt\n+++ b/u.txt\n@@ -1,3 +1,3 @@\n-x\n+X\n *** Begin Patch\n y\n"
            ),
            "unified context line must stay unified"
        );
        assert!(
            !looks_like_begin_patch(
                "--- a/u.txt\n+++ b/u.txt\n@@ -1,2 +1,3 @@\n x\n+*** Begin Patch\n y\n"
            ),
            "unified + line must stay unified"
        );
    }

    #[test]
    fn looks_like_begin_patch_extra_stars() {
        assert!(looks_like_begin_patch(
            "*** Begin Patch ***\n*** End Patch ***\n"
        ));
        assert!(looks_like_begin_patch("*** Begin Patch\n*** End Patch\n"));
        // Mid-document trimmed match is not an envelope (#2505).
        assert!(!looks_like_begin_patch(
            "preamble\n*** Begin Patch ***\n*** End Patch ***\n"
        ));
        assert!(
            !looks_like_begin_patch(
                "--- a/u.txt\n+++ b/u.txt\n@@ -1,3 +1,3 @@\n-x\n+X\n *** Begin Patch ***\n y\n"
            ),
            "unified context line must stay unified"
        );
        assert!(
            !looks_like_begin_patch(
                "--- a/u.txt\n+++ b/u.txt\n@@ -1,2 +1,3 @@\n x\n+*** Begin Patch ***\n y\n"
            ),
            "unified + line must stay unified"
        );
    }

    #[test]
    fn parse_begin_patch_extra_stars_update_dest() {
        let patch = "\
*** Begin Patch ***
*** Update File: code.rs ***
@@
-fn old() {}
+fn new() {}
*** End Patch ***
";
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "code.rs");
                assert!(!path.ends_with("***"), "dest must not keep trailing stars");
                assert!(hunks.contains("-fn old() {}"));
                assert!(hunks.contains("+fn new() {}"));
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn parse_begin_patch_extra_stars_add_delete_move_dests() {
        let patch = "\
*** Begin Patch ***
*** Add File: new.rs ***
+hello
*** Update File: src.rs ***
*** Move to: dest.rs ***
@@
-fn a() {}
+fn b() {}
*** Delete File: gone.rs ***
*** End Patch ***
";
        let paths = begin_patch_declared_paths(patch).expect("paths");
        assert_eq!(paths, vec!["new.rs", "src.rs", "dest.rs", "gone.rs"]);
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(ops.len(), 3);
        match &ops[0] {
            BeginPatchOp::Add { path, .. } => assert_eq!(path, "new.rs"),
            other => panic!("expected Add, got {other:?}"),
        }
        match &ops[1] {
            BeginPatchOp::Update { path, move_to, .. } => {
                assert_eq!(path, "src.rs");
                assert_eq!(move_to.as_deref(), Some("dest.rs"));
            }
            other => panic!("expected Update, got {other:?}"),
        }
        match &ops[2] {
            BeginPatchOp::Delete { path } => assert_eq!(path, "gone.rs"),
            other => panic!("expected Delete, got {other:?}"),
        }
    }

    #[test]
    fn parse_begin_patch_extra_stars_empty_update_dest_is_invalid_input() {
        let patch = "\
*** Begin Patch ***
*** Update File: ***
@@
-a
+b
*** End Patch ***
";
        let err = parse_begin_patch(patch).expect_err("empty dest");
        assert!(
            crate::exit::is_invalid_input(&err),
            "empty dest after peel must be invalid_input, not dest ***: {err}"
        );
        assert!(
            err.to_string().to_lowercase().contains("empty")
                || err.to_string().to_lowercase().contains("dest"),
            "error should name empty dest: {err}"
        );
    }

    #[test]
    fn apply_codex_hunks_extra_stars_end_of_file() {
        let out = apply_codex_hunks("x\nx\n", "-x\n+y\n*** End of File ***\n").expect("eof");
        assert_eq!(out, "x\ny\n");
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
    fn mixed_grammar_backslash_git_prefix_is_rejected() {
        let patch = "\
*** Begin Patch
*** Update File: code.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
--- a\\other.rs
+++ b\\other.rs
";
        let err = parse_begin_patch(patch).expect_err("mixed backslash");
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

    #[test]
    fn context_end_patch_does_not_drop_later_update() {
        let patch = "\
*** Begin Patch
*** Update File: g.md
@@
 *** End Patch
-old
+new
*** Update File: b.rs
@@
-x
+y
*** End Patch
";
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(
            ops.len(),
            2,
            "context End Patch must not finish the document"
        );
        match &ops[0] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "g.md");
                assert!(
                    hunks.contains(" *** End Patch"),
                    "space-prefixed marker is hunk context: {hunks:?}"
                );
                let out = apply_codex_hunks("*** End Patch\nold\n", hunks).expect("g.md");
                assert_eq!(out, "*** End Patch\nnew\n");
            }
            other => panic!("expected first Update, got {other:?}"),
        }
        match &ops[1] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "b.rs");
                let out = apply_codex_hunks("x\n", hunks).expect("b.rs");
                assert_eq!(out, "y\n");
            }
            other => panic!("expected second Update, got {other:?}"),
        }
    }

    #[test]
    fn context_add_file_does_not_start_an_op() {
        let patch = "\
*** Begin Patch
*** Update File: g.md
@@
 *** Add File: sneaky.rs
-old
+new
*** End Patch
";
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "g.md");
                assert!(hunks.contains(" *** Add File: sneaky.rs"), "{hunks:?}");
            }
            other => panic!("expected Update, got {other:?}"),
        }
        let paths = begin_patch_declared_paths(patch).expect("paths");
        assert_eq!(paths, vec!["g.md"]);
    }

    #[test]
    fn end_of_file_keeps_following_hunk_and_applies_both() {
        let patch = "\
*** Begin Patch
*** Update File: a2.rs
@@
-a
+b
*** End of File
@@
-c
+d
*** End Patch
";
        let ops = parse_begin_patch(patch).expect("parse");
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            BeginPatchOp::Update { path, hunks, .. } => {
                assert_eq!(path, "a2.rs");
                let out = apply_codex_hunks("a\nc\n", hunks).expect("both hunks");
                assert_eq!(out, "b\nd\n");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn end_of_file_anchors_last_occurrence() {
        let out = apply_codex_hunks("x\nx\n", "-x\n+y\n*** End of File\n").expect("eof");
        assert_eq!(out, "x\ny\n");
    }

    fn two_fn_fixture() -> &'static str {
        "\
fn a() {
    x
}
fn b() {
    x
}
"
    }

    #[test]
    fn at_at_scope_hint_disambiguates_identical_bodies() {
        let out =
            apply_codex_hunks(two_fn_fixture(), "@@ fn b()\n-    x\n+    y\n").expect("scope");
        assert_eq!(
            out,
            "\
fn a() {
    x
}
fn b() {
    y
}
"
        );
    }

    #[test]
    fn at_at_insert_only_hunk_valid_with_hint() {
        let out = apply_codex_hunks(two_fn_fixture(), "@@ fn b()\n+    z\n").expect("insert");
        assert_eq!(
            out,
            "\
fn a() {
    x
}
fn b() {
    z
    x
}
"
        );
    }

    #[test]
    fn bare_at_at_stays_global_unique_match() {
        let err = apply_codex_hunks(two_fn_fixture(), "@@\n-    x\n+    y\n").expect_err("bare");
        assert!(crate::fallback::is_ambiguous(&err));
    }
}
