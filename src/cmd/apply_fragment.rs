//! CLI: constrained freeform fragment apply (#2018).

use crate::cli::global::GlobalFlags;
use crate::cmd::output::run_write_op;
use crate::ops::apply_fragment::build_apply_fragment_spec;
use crate::plan::Operation;
use clap::Args;
use serde::Serialize;

#[derive(Debug, Args)]
#[command(
    name = "apply-fragment",
    after_help = "\
EXAMPLES:
  # Morph-style snippet + required after-anchor (markers stripped)
  patchloom apply-fragment src/lib.rs --after 'fn foo() {' --fragment '// ... existing code ...
  bar();
// ... existing code ...' --apply

  # Replace a unique span with freeform new text
  patchloom apply-fragment src/lib.rs --old 'return 0;' --fragment 'return 1;' --apply

NOTES:
  Exactly one of --after, --before, or --old is required (no anchor-less Morph merge).
  Lazy marker lines such as // ... existing code ... are stripped from --fragment.
  Dry-run by default (exit 2 when changes would apply). See docs/plans/morph-gap-matrix.md."
)]
pub struct ApplyFragmentArgs {
    /// Path of the file to edit.
    pub file: String,
    /// New text or Morph-style snippet (lazy marker lines stripped).
    #[arg(long)]
    pub fragment: Option<String>,
    /// Read fragment from stdin.
    #[arg(long)]
    pub stdin: bool,
    /// Optional human note (not used for merge).
    #[arg(long)]
    pub instruction: Option<String>,
    /// Insert cleaned fragment after this unique anchor.
    #[arg(long)]
    pub after: Option<String>,
    /// Insert cleaned fragment before this unique anchor.
    #[arg(long)]
    pub before: Option<String>,
    /// Replace this unique span with the cleaned fragment.
    #[arg(long)]
    pub old: Option<String>,
    /// Allow non-unique anchors (default: require unique).
    #[arg(long)]
    pub allow_non_unique: bool,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, Serialize)]
struct ApplyFragmentOutput {
    ok: bool,
    path: String,
    op: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    applied: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_session: Option<String>,
}

pub fn run(args: ApplyFragmentArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    if args.fragment.is_some() && args.stdin {
        let msg = "--fragment and --stdin cannot be combined";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    }
    let raw = if let Some(f) = args.fragment {
        f
    } else if args.stdin {
        std::io::read_to_string(std::io::stdin())?
    } else {
        let msg = "provide --fragment or --stdin";
        global.emit_error_json_kind(Some("invalid_input"), msg)?;
        return Ok(crate::exit::FAILURE);
    };

    let unique = !args.allow_non_unique;
    // Validate early for agent JSON kind.
    if let Err(e) = build_apply_fragment_spec(
        &raw,
        args.instruction.as_deref(),
        args.after.as_deref(),
        args.before.as_deref(),
        args.old.as_deref(),
        unique,
    ) {
        global.emit_error_json_kind(Some("invalid_input"), &e.msg)?;
        return Ok(crate::exit::FAILURE);
    }

    let op = Operation::ApplyFragment {
        path: args.file.clone(),
        fragment: raw,
        instruction: args.instruction.clone(),
        after: args.after.clone(),
        before: args.before.clone(),
        old: args.old.clone(),
        unique,
    };

    match run_write_op(
        op,
        global,
        |phase, diff, backup| ApplyFragmentOutput {
            ok: true,
            path: args.file.clone(),
            op: "apply.fragment",
            instruction: args.instruction.clone(),
            diff,
            applied: phase.applied_flag(),
            backup_session: backup,
        },
        "fragment apply would change files",
        "applied fragment",
    ) {
        Ok(code) => Ok(code),
        Err(e) => {
            // Engine hard-fails no-match / unique multi-match as typed errors
            // (require_change + unique on desugar). Map to agent exit codes.
            if let Some((kind, code)) = crate::exit::classify_typed_error(&e) {
                let msg = e.to_string();
                global.emit_error_json_kind(Some(kind), &msg)?;
                return Ok(code);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use tempfile::TempDir;

    fn g_apply() -> GlobalFlags {
        GlobalFlags {
            apply: true,
            ..GlobalFlags::default()
        }
    }

    #[test]
    fn apply_fragment_after_anchor_strips_markers() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "fn foo() {\n  a();\n}\n").unwrap();
        let file = path.to_string_lossy().into_owned();
        let code = run(
            ApplyFragmentArgs {
                file: file.clone(),
                fragment: Some(
                    "// ... existing code ...\n  b();\n// ... existing code ...\n".into(),
                ),
                stdin: false,
                instruction: Some("add b".into()),
                after: Some("  a();".into()),
                before: None,
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &g_apply(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::SUCCESS);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("  a();\n  b();\n"), "{body}");
        assert!(!body.contains("existing code"), "{body}");
    }

    #[test]
    fn apply_fragment_missing_anchor_invalid_input() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "x\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("y".into()),
                stdin: false,
                instruction: None,
                after: None,
                before: None,
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &GlobalFlags::default(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::FAILURE);
    }

    #[test]
    fn apply_fragment_preview_exit_2() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "a\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("b\n".into()),
                stdin: false,
                instruction: None,
                after: Some("a".into()),
                before: None,
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &GlobalFlags::default(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::CHANGES_DETECTED);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\n");
    }

    #[test]
    fn apply_fragment_replace_old() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "return 0;\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("return 1;\n".into()),
                stdin: false,
                instruction: None,
                after: None,
                before: None,
                old: Some("return 0;".into()),
                allow_non_unique: false,
                write: Default::default(),
            },
            &g_apply(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::SUCCESS);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.starts_with("return 1;"),
            "expected replace to return 1; got {body:?}"
        );
    }

    #[test]
    fn apply_fragment_before_anchor() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "fn foo() {\n  a();\n}\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("  pre();\n".into()),
                stdin: false,
                instruction: None,
                after: None,
                before: Some("  a();".into()),
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &g_apply(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::SUCCESS);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("  pre();\n  a();"), "{body}");
    }

    #[test]
    fn apply_fragment_anchor_miss_no_matches() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "only\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("x".into()),
                stdin: false,
                instruction: None,
                after: Some("missing_anchor".into()),
                before: None,
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &g_apply(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::NO_MATCHES);
    }

    #[test]
    fn apply_fragment_unique_multi_match_ambiguous() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.rs");
        std::fs::write(&path, "dup\ndup\n").unwrap();
        let g = GlobalFlags {
            apply: true,
            json: true,
            quiet: true,
            ..GlobalFlags::default()
        };
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("x".into()),
                stdin: false,
                instruction: None,
                after: Some("dup".into()),
                before: None,
                old: None,
                allow_non_unique: false,
                write: Default::default(),
            },
            &g,
        )
        .unwrap();
        assert_eq!(code, crate::exit::AMBIGUOUS);
    }

    #[test]
    fn apply_fragment_allow_non_unique_applies_all() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.txt");
        std::fs::write(&path, "dup\ndup\n").unwrap();
        let code = run(
            ApplyFragmentArgs {
                file: path.to_string_lossy().into(),
                fragment: Some("X".into()),
                stdin: false,
                instruction: None,
                after: Some("dup".into()),
                before: None,
                old: None,
                allow_non_unique: true,
                write: Default::default(),
            },
            &g_apply(),
        )
        .unwrap();
        assert_eq!(code, crate::exit::SUCCESS);
        let body = std::fs::read_to_string(&path).unwrap();
        // Whole-line anchors use line-oriented insert (#1885).
        assert_eq!(
            body, "dup\nX\ndup\nX\n",
            "both anchors should get insert: {body:?}"
        );
    }
}
