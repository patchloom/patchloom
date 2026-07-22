use crate::cli::global::GlobalFlags;
use crate::cmd::agent_rules::{
    AGENT_RULES_GENERATED_MARKER, AgentMode, AgentPlatform, AgentRulesArgs, generate_agent_rules,
};
use crate::exit;
use anyhow::Context;
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
#[command(after_help = "\
EXAMPLES:
  patchloom init
  patchloom init --yes")]
pub struct InitArgs {
    /// Skip confirmation prompts.
    #[arg(long, short = 'y')]
    pub yes: bool,
}

/// Candidate files where agent instructions may already exist.
const AGENT_FILES: &[&str] = &[
    "AGENTS.md",
    "Agents.md",
    "AGENT.md",
    "Claude.md",
    "CLAUDE.md",
    "PATCHLOOM.md",
];

pub fn run(args: InitArgs, global: &GlobalFlags) -> anyhow::Result<u8> {
    crate::verbose!("init: yes={}", args.yes);
    let cwd = global.resolve_cwd()?;
    let auto_yes = args.yes;
    let quiet = global.quiet;
    let structured = global.json || global.jsonl;
    // Agent/CI structured runs: do not leave ok:true with agent_rules=skipped
    // when AGENTS.md was never written (#1833). Interactive TTY still prompts.
    let auto_agent_rules = auto_yes || structured;

    // Helper: print to stderr unless --quiet or structured (JSON owns stdout).
    macro_rules! status {
        ($($arg:tt)*) => {
            if !quiet && !structured { eprintln!($($arg)*); }
        };
    }

    #[derive(Default)]
    struct InitReport {
        agent_rules: String,
        agent_rules_path: Option<String>,
        completions: String,
        gitignore: String,
    }
    let mut report = InitReport::default();

    // 1. Generate and write agent rules.
    let rules = generate_agent_rules(&AgentRulesArgs {
        mode: AgentMode::All,
        platform: AgentPlatform::All,
    });

    let target = find_agent_file(&cwd);
    let (target_path, action) = match target {
        Some(existing) => (existing, "append"),
        None => (cwd.join("AGENTS.md"), "create"),
    };
    let rel_target = target_path
        .strip_prefix(&cwd)
        .unwrap_or(&target_path)
        .display()
        .to_string();
    report.agent_rules_path = Some(rel_target.clone());

    if action == "append" {
        // Check if patchloom rules are already present.
        let content =
            crate::files::load_text_strict(&target_path, &target_path.display().to_string())
                .with_context(|| format!("reading existing {}", target_path.display()))?;
        if content.contains(AGENT_RULES_GENERATED_MARKER) {
            report.agent_rules = "skipped_already_present".into();
            status!("{rel_target} already contains patchloom rules, skipping.");
        } else if auto_agent_rules || confirm(&format!("Append patchloom rules to {rel_target}?")) {
            let mut content = content;
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
            content.push_str(&rules);
            std::fs::write(&target_path, content)
                .with_context(|| format!("writing {}", target_path.display()))?;
            report.agent_rules = "appended".into();
            status!("appended patchloom rules to {rel_target}");
        } else {
            // Non-interactive decline (no TTY, no --yes/--json): explain next step.
            report.agent_rules = "skipped_use_yes".into();
            status!(
                "skipped {rel_target} (declined or non-interactive; re-run with --yes or --json)"
            );
        }
    } else if auto_agent_rules || confirm(&format!("Create {rel_target}?")) {
        std::fs::write(&target_path, &rules)
            .with_context(|| format!("writing {}", target_path.display()))?;
        report.agent_rules = "created".into();
        status!("created {rel_target}");
    } else {
        report.agent_rules = "skipped_use_yes".into();
        status!("skipped {rel_target} (declined or non-interactive; re-run with --yes or --json)");
    }

    // 2. Shell completions: auto-install or hint.
    if let Some(shell) = detect_shell() {
        if let Some(target) = completion_install_path(&shell) {
            let parent_ready = target.parent().map_or(Ok(()), |parent| {
                if parent.exists() {
                    Ok(())
                } else {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("creating {}", parent.display()))
                }
            });
            if let Err(e) = parent_ready {
                report.completions = format!("failed:{e}");
                status!("failed to prepare completion directory: {e}");
                let cmd = completion_command(&shell);
                status!("\nshell completions ({shell}):");
                status!("  {cmd}");
            } else if auto_yes
                || confirm(&format!(
                    "Install {} completions to {}?",
                    shell,
                    target.display()
                ))
            {
                match generate_completions(&shell, &target) {
                    Ok(()) => {
                        report.completions = format!("installed:{shell}");
                        status!("installed {shell} completions to {}", target.display());
                    }
                    Err(e) => {
                        report.completions = format!("failed:{e}");
                        status!("failed to install completions: {e}");
                    }
                }
            } else {
                report.completions = format!("hint:{shell}");
                let cmd = completion_command(&shell);
                status!("\nshell completions ({shell}):");
                status!("  {cmd}");
            }
        } else {
            report.completions = format!("hint:{shell}");
            let cmd = completion_command(&shell);
            status!("\nshell completions ({shell}):");
            status!("  {cmd}");
        }
    } else {
        report.completions = "hint:all".into();
        if !quiet && !structured {
            eprintln!("\nshell completions:");
            eprintln!("  patchloom completions <bash|zsh|fish|elvish>");
        }
    }

    // 3. Keep undo backups out of git noise (pairs with `status` filtering).
    match ensure_gitignore_patchloom(&cwd) {
        Ok(GitignorePatchloom::Created) => {
            report.gitignore = "created".into();
            status!("created .gitignore with .patchloom/");
        }
        Ok(GitignorePatchloom::Appended) => {
            report.gitignore = "appended".into();
            status!("appended .patchloom/ to .gitignore");
        }
        Ok(GitignorePatchloom::AlreadyPresent) => {
            report.gitignore = "already_present".into();
            status!(".gitignore already ignores .patchloom/");
        }
        Err(e) => {
            report.gitignore = format!("error:{e}");
            status!("could not update .gitignore: {e}");
        }
    }

    if !quiet && !structured {
        // 4. MCP setup hint.
        eprintln!();
        if cfg!(feature = "mcp") {
            let mcp_json_hint = r#"    { "servers": { "patchloom": { "command": "patchloom", "args": ["mcp-server"] } } }"#;
            eprintln!("MCP server is available. Add to your agent's config:");
            if cwd.join(".grok").is_dir() || home_file_exists(".grok/config.toml") {
                eprintln!("  Grok: add to ~/.grok/config.toml:");
                eprintln!("    [mcp_servers.patchloom]");
                eprintln!("    command = \"patchloom\"");
                eprintln!("    args = [\"mcp-server\"]");
            }
            if cwd.join(".vscode").is_dir() {
                eprintln!("  VS Code: create .vscode/mcp.json:");
                eprintln!("{mcp_json_hint}");
            }
            if cwd.join(".cursor").is_dir() {
                eprintln!("  Cursor: create .cursor/mcp.json:");
                eprintln!("{mcp_json_hint}");
            }
        } else {
            eprintln!(
                "MCP server not available (this binary was built with --no-default-features)."
            );
        }

        eprintln!();
        eprintln!("setup complete.");
    }

    if structured {
        #[derive(serde::Serialize)]
        struct InitJson<'a> {
            ok: bool,
            agent_rules: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            agent_rules_path: Option<&'a str>,
            completions: &'a str,
            gitignore: &'a str,
            mcp_available: bool,
        }
        let payload = InitJson {
            ok: true,
            agent_rules: &report.agent_rules,
            agent_rules_path: report.agent_rules_path.as_deref(),
            completions: &report.completions,
            gitignore: &report.gitignore,
            mcp_available: cfg!(feature = "mcp"),
        };
        global.emit_json(&payload)?;
    }
    Ok(exit::SUCCESS)
}

#[derive(Debug, PartialEq, Eq)]
enum GitignorePatchloom {
    Created,
    Appended,
    AlreadyPresent,
}

const GITIGNORE_PATCHLOOM_LINE: &str = ".patchloom/";

/// Ensure `.gitignore` ignores Patchloom backup sessions so `git status`
/// stays clean after `--apply` (CLI `status` already filters these paths).
fn ensure_gitignore_patchloom(cwd: &Path) -> anyhow::Result<GitignorePatchloom> {
    let path = cwd.join(".gitignore");
    if path.exists() {
        let content = crate::files::load_text_strict(&path, &path.display().to_string())
            .with_context(|| format!("reading {}", path.display()))?;
        if gitignore_already_covers_patchloom(&content) {
            return Ok(GitignorePatchloom::AlreadyPresent);
        }
        let mut next = content;
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(GITIGNORE_PATCHLOOM_LINE);
        next.push('\n');
        std::fs::write(&path, next).with_context(|| format!("writing {}", path.display()))?;
        return Ok(GitignorePatchloom::Appended);
    }
    let body =
        format!("# Patchloom undo sessions (created by --apply)\n{GITIGNORE_PATCHLOOM_LINE}\n");
    std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
    Ok(GitignorePatchloom::Created)
}

fn gitignore_already_covers_patchloom(content: &str) -> bool {
    content.lines().any(|line| {
        let t = line.trim();
        t == ".patchloom/"
            || t == ".patchloom"
            || t == "**/.patchloom/"
            || t == "**/.patchloom"
            || t.starts_with(".patchloom/")
    })
}

fn find_agent_file(cwd: &Path) -> Option<std::path::PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(cwd)
        .ok()?
        .filter_map(Result::ok)
        .collect();
    for name in AGENT_FILES {
        for entry in &entries {
            if entry.file_name() == std::ffi::OsStr::new(name) {
                return Some(entry.path());
            }
        }
    }
    None
}

fn detect_shell() -> Option<String> {
    std::env::var("SHELL").ok().and_then(|s| {
        let name = Path::new(&s).file_name()?.to_str()?.to_string();
        match name.as_str() {
            "bash" | "zsh" | "fish" | "elvish" => Some(name),
            _ => None,
        }
    })
}

fn completion_command(shell: &str) -> String {
    match shell {
        "bash" => "patchloom completions bash > /etc/bash_completion.d/patchloom".into(),
        "zsh" => "patchloom completions zsh > ~/.zfunc/_patchloom".into(),
        "fish" => "patchloom completions fish > ~/.config/fish/completions/patchloom.fish".into(),
        "elvish" => "patchloom completions elvish >> ~/.config/elvish/rc.elv".into(),
        _ => format!("patchloom completions {shell}"),
    }
}

fn completion_install_path(shell: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let home = Path::new(&home);
    match shell {
        "bash" => {
            let xdg = home.join(".local/share/bash-completion/completions");
            if xdg.is_dir() {
                return Some(xdg.join("patchloom"));
            }
            // System path as fallback (may need root).
            let sys = Path::new("/etc/bash_completion.d/patchloom");
            Some(sys.to_path_buf())
        }
        "zsh" => Some(home.join(".zfunc/_patchloom")),
        "fish" => Some(home.join(".config/fish/completions/patchloom.fish")),
        _ => None,
    }
}

fn generate_completions(shell: &str, target: &Path) -> anyhow::Result<()> {
    use crate::cli::Cli;
    let clap_shell = match shell {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "elvish" => clap_complete::Shell::Elvish,
        _ => {
            return Err(crate::exit::InvalidInputError {
                msg: format!("unsupported shell: {shell}"),
            }
            .into());
        }
    };
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let mut buf = Vec::new();
    clap_complete::generate(clap_shell, &mut cmd, "patchloom", &mut buf);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    std::fs::write(target, buf)
        .with_context(|| format!("writing completions to {}", target.display()))?;
    Ok(())
}

fn home_file_exists(rel: &str) -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        return Path::new(&home).join(rel).exists();
    }
    false
}

fn confirm(prompt: &str) -> bool {
    crate::cli::global::confirm_prompt(prompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_shell_from_env() {
        // Just ensure the function doesn't panic.
        let _ = detect_shell();
    }

    #[test]
    fn completion_command_known_shells() {
        assert!(completion_command("bash").contains("bash_completion"));
        assert!(completion_command("zsh").contains("_patchloom"));
        assert!(completion_command("fish").contains("completions/patchloom.fish"));
    }

    #[test]
    fn completion_command_elvish_uses_append() {
        // #1177: Elvish completions must use >> (append) not > (overwrite)
        // to avoid destroying the user's existing rc.elv configuration.
        let cmd = completion_command("elvish");
        assert!(
            cmd.contains(">>"),
            "elvish command should use >> (append), got: {cmd}"
        );
        assert!(
            !cmd.contains(" > "),
            "elvish command must not use > (overwrite), got: {cmd}"
        );
    }

    #[test]
    fn find_agent_file_none_in_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(find_agent_file(dir.path()).is_none());
    }

    /// fixrealloop: plain `init` without --yes on non-TTY must not look like
    /// a silent no-op; report.agent_rules names the remediation.
    #[test]
    fn init_without_yes_noninteractive_reports_skipped_use_yes() {
        let _guard = crate::cli::global::ConfirmAnswerGuard::force(false);
        let dir = tempfile::TempDir::new().unwrap();
        let global = GlobalFlags {
            cwd: Some(dir.path().to_string_lossy().into_owned()),
            json: false,
            quiet: true,
            ..GlobalFlags::default()
        };
        let code = run(InitArgs { yes: false }, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        // No AGENTS.md written when confirm declines.
        assert!(
            !dir.path().join("AGENTS.md").exists(),
            "must not create AGENTS.md without --yes when confirm declines"
        );
        // --yes still creates.
        let code = run(InitArgs { yes: true }, &global).unwrap();
        assert_eq!(code, exit::SUCCESS);
        assert!(
            dir.path().join("AGENTS.md").exists(),
            "init --yes must create AGENTS.md"
        );
    }

    #[test]
    fn ensure_gitignore_creates_when_missing() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            ensure_gitignore_patchloom(dir.path()).unwrap(),
            GitignorePatchloom::Created
        );
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(GITIGNORE_PATCHLOOM_LINE));
        assert_eq!(
            ensure_gitignore_patchloom(dir.path()).unwrap(),
            GitignorePatchloom::AlreadyPresent
        );
    }

    #[test]
    fn ensure_gitignore_appends_when_present_without_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        assert_eq!(
            ensure_gitignore_patchloom(dir.path()).unwrap(),
            GitignorePatchloom::Appended
        );
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains("target/"));
        assert!(content.contains(GITIGNORE_PATCHLOOM_LINE));
    }

    #[test]
    fn find_agent_file_finds_agents_md() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find AGENTS.md");
        assert!(found.ends_with("AGENTS.md"));
    }

    #[test]
    fn find_agent_file_preserves_claude_md_case() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Claude.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find Claude.md");
        assert!(found.ends_with("Claude.md"));
    }

    #[test]
    fn find_agent_file_supports_agents_md_variant() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Agents.md"), "# Rules\n").unwrap();
        let found = find_agent_file(dir.path()).expect("should find Agents.md");
        assert!(found.ends_with("Agents.md"));
    }

    #[test]
    fn generate_completions_writes_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("completions/patchloom");
        generate_completions("bash", &target).unwrap();
        assert!(target.exists());
        let content = std::fs::read_to_string(&target).unwrap();
        assert!(content.contains("patchloom"));
    }

    #[test]
    fn completion_install_path_returns_some_for_known_shells() {
        // Requires HOME to be set (normal in test environments).
        if std::env::var("HOME").is_ok() {
            // strengthened from bare is_some per Test Auditor (gives message on fail)
            let _ = completion_install_path("bash").expect("bash shell path");
            let _ = completion_install_path("zsh").expect("zsh shell path");
            let _ = completion_install_path("fish").expect("fish shell path");
            assert!(completion_install_path("elvish").is_none());
        }
    }
}
