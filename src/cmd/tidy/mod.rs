//! Whitespace / newline / EOL tidy CLI (`patchloom tidy check|fix`).
//!
//! - [`check`]: report issues (parallel scan)
//! - [`fix`]: dirty-file scan then engine stage + finalize_report

mod check;
mod fix;

pub use check::TidyIssue;

use crate::cli::global::GlobalFlags;
use clap::Args;

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom tidy fix . --apply
  patchloom tidy fix src/ --ensure-final-newline --apply
  patchloom tidy fix . --trim-trailing-whitespace --apply
  patchloom tidy check . --normalize-eol lf

With no write-policy flags, `tidy fix` enables final-newline and trailing-
whitespace fixes (the same issues `tidy check` always reports). Pass explicit
flags to narrow the fix set.")]
pub struct TidyArgs {
    #[command(subcommand)]
    pub action: TidyAction,
    #[command(flatten)]
    pub write: crate::cli::global::WriteFlags,
}

#[derive(Debug, clap::Subcommand)]
pub enum TidyAction {
    /// Report newline, EOL, and whitespace issues in text files.
    Check { paths: Vec<String> },
    /// Apply normalization fixes to text files.
    Fix {
        paths: Vec<String>,
        /// Dedent: remove leading whitespace. Values: "auto", "tab", or a number (e.g. "4").
        #[arg(long)]
        dedent: Option<String>,
        /// Indent: add leading whitespace. Values: "tab" or a number (e.g. "4").
        #[arg(long)]
        indent: Option<String>,
        /// Restrict dedent/indent to a line range (1-based inclusive, e.g. "10:50" or "10-50").
        #[arg(long)]
        lines: Option<String>,
    },
}

pub fn run(args: TidyArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("tidy: action={:?}", std::mem::discriminant(&args.action));
    match args.action {
        TidyAction::Check { paths } => check::run_check(&paths, global),
        TidyAction::Fix {
            paths,
            dedent,
            indent,
            lines,
        } => fix::run_fix(paths, dedent, indent, lines, global),
    }
}

#[cfg(test)]
mod tests;
