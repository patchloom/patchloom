//! Shell command-position token matching for agent hosts (#1494).
//!
//! A token is in **command position** when it is the invocable command of a
//! simple shell fragment: start of line (after whitespace), after `&&` `|` `;`,
//! or after transparent prefixes (`sudo`, `env KEY=val`…, `timeout`, `nice`,
//! and common option flags like `-E` / `-p`). It is **not** command position
//! when it is an argument (`uv pip`) or inside a longer word (`pipenv`).
//!
//! Known false positive: `command -v pip` may treat `pip` as command position
//! because `-v` is peeled as a flag after the transparent `command` prefix.

/// Transparent prefixes that may appear before a command without making the
/// next token an argument.
const TRANSPARENT_PREFIXES: &[&str] = &[
    "sudo", "doas", "env", "command", "builtin", "exec", "time", "nice", "nohup",
    // Agent scripts often wrap installs: `timeout 30 pip install`, `stdbuf -oL …`.
    "timeout", "stdbuf", "ionice",
];

/// Return true if `token` at byte range `[start, end)` is in shell command position.
pub fn is_command_position(content: &str, start: usize, end: usize) -> bool {
    if start > content.len() || end > content.len() || start >= end {
        return false;
    }
    // Must be a standalone token (word boundary).
    let bytes = content.as_bytes();
    if start > 0 && is_token_char(bytes[start - 1]) {
        return false;
    }
    if end < bytes.len() && is_token_char(bytes[end]) {
        return false;
    }

    let before = &content[..start];
    // Walk backward over whitespace and transparent prefixes on this command fragment.
    let mut rest = before;
    loop {
        // Trim only horizontal whitespace. Newlines are command separators and
        // must not be stripped (multi-line `timeout 30 pip` after a prior line
        // would otherwise peel into the previous command's tokens).
        rest = rest.trim_end_matches([' ', '\t']);
        if rest.is_empty() {
            return true;
        }
        let last = rest.as_bytes()[rest.len() - 1];
        // Command separator → command position.
        if matches!(last, b'|' | b';' | b'\n' | b'\r') {
            return true;
        }
        // `&&` or `||`
        if rest.ends_with("&&") || rest.ends_with("||") {
            return true;
        }
        // After `$(` or backtick open is still a subshell command position.
        if last == b'(' || last == b'`' {
            return true;
        }

        // Peel one transparent prefix token (or env assignment) from the end.
        let Some((prefix_start, token)) = last_shell_token(rest) else {
            return false;
        };
        if is_env_assignment(token) {
            rest = &rest[..prefix_start];
            continue;
        }
        // Duration / niceness after wrappers only: `timeout 30 pip`, or value after
        // an arg-taking / option flag (`nice -n 10`). Bare `5 pip` stays non-command.
        if is_duration_or_number(token) {
            let left = rest[..prefix_start].trim_end();
            if let Some((_, prev)) = last_shell_token(left)
                && (TRANSPARENT_PREFIXES.contains(&prev)
                    || is_option_flag(prev)
                    || is_arg_taking_flag(prev))
            {
                rest = &rest[..prefix_start];
                continue;
            }
            return false;
        }
        // Option flags after sudo/time/env (`sudo -E pip`, `time -p pip`).
        // Known false positive: `command -v pip` treats `pip` as command position.
        if is_option_flag(token) {
            rest = &rest[..prefix_start];
            continue;
        }
        if TRANSPARENT_PREFIXES.contains(&token) {
            rest = &rest[..prefix_start];
            continue;
        }
        // Value for an arg-taking flag: `sudo -u root pip` → peel `root` when
        // the token before it is `-u` / `--user` / `-g` / `--group`.
        let left = rest[..prefix_start].trim_end();
        if let Some((flag_start, flag)) = last_shell_token(left)
            && is_arg_taking_flag(flag)
        {
            rest = &left[..flag_start];
            continue;
        }
        // Preceded by a real word → argument position (e.g. `uv pip`, `python -m pip`).
        return false;
    }
}

/// Collect all non-overlapping command-position matches of `token` (literal).
pub fn find_command_position_matches(content: &str, token: &str) -> Vec<(usize, usize)> {
    if token.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = content[search_from..].find(token) {
        let start = search_from + rel;
        let end = start + token.len();
        if is_command_position(content, start, end) {
            out.push((start, end));
        }
        search_from = start + token.len().max(1);
    }
    out
}

/// Replace all command-position occurrences of `from` with `to`.
/// Returns (new_content, match_count).
pub fn replace_command_position(content: &str, from: &str, to: &str) -> (String, usize) {
    let matches = find_command_position_matches(content, from);
    if matches.is_empty() {
        return (content.to_string(), 0);
    }
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    for (start, end) in &matches {
        out.push_str(&content[last..*start]);
        out.push_str(to);
        last = *end;
    }
    out.push_str(&content[last..]);
    (out, matches.len())
}

fn is_token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'/'
}

fn is_env_assignment(token: &str) -> bool {
    // KEY=value form used after `env`.
    if let Some((k, _)) = token.split_once('=') {
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

/// Shell option flag (`-E`, `-p`, `--preserve-env`), not a command name.
fn is_option_flag(token: &str) -> bool {
    if token.len() < 2 || !token.starts_with('-') {
        return false;
    }
    // Reject bare `-` / `--` and numeric tokens like `-1` used as args.
    let body = token.trim_start_matches('-');
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && body.chars().any(|c| c.is_ascii_alphabetic())
}

/// Flags whose next token is an argument (`sudo -u root`, `sudo --user root`).
fn is_arg_taking_flag(token: &str) -> bool {
    matches!(
        token,
        // sudo / doas / env
        "-u" | "--user"
            | "-g"
            | "--group"
            | "-C"
            | "--close-from"
            | "-p"
            | "--prompt"
            // nice / ionice niceness
            | "-n"
            | "--adjustment"
            // ionice class
            | "-c"
            | "--class"
            // stdbuf
            | "-o"
            | "--output"
            | "-e"
            | "--error"
            | "-i"
            | "--input"
            // timeout
            | "-s"
            | "--signal"
            | "-k"
            | "--kill-after"
    )
}

/// Bare duration / niceness token: `30`, `5s`, `1.5m` (GNU timeout style).
fn is_duration_or_number(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let bytes = token.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return false;
    }
    // Optional fractional part.
    if i < bytes.len() && bytes[i] == b'.' {
        let frac_start = i + 1;
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == frac_start {
            return false; // bare trailing dot
        }
    }
    if i == bytes.len() {
        return true; // pure number
    }
    // Optional unit suffix used by timeout(1).
    matches!(&token[i..], "s" | "m" | "h" | "d" | "ms")
}

/// Last shell token in `s` (no leading/trailing ws). Returns (byte start, token).
fn last_shell_token(s: &str) -> Option<(usize, &str)> {
    let s = s.trim_end();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let end = s.len();
    // Include KEY=val as one token.
    let mut i = end;
    while i > 0 {
        let b = bytes[i - 1];
        if is_token_char(b) || b == b'=' {
            i -= 1;
        } else {
            break;
        }
    }
    if i == end {
        return None;
    }
    Some((i, &s[i..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pip_install_is_command() {
        let c = "pip install x\n";
        assert!(is_command_position(c, 0, 3));
        let (out, n) = replace_command_position(c, "pip", "uv");
        assert_eq!(n, 1);
        assert_eq!(out, "uv install x\n");
    }

    #[test]
    fn pipenv_not_command_for_pip() {
        let c = "pipenv install\n";
        assert!(find_command_position_matches(c, "pip").is_empty());
        let (_, n) = replace_command_position(c, "pip", "uv");
        assert_eq!(n, 0);
    }

    #[test]
    fn uv_pip_argument_not_command() {
        let c = "uv pip install\n";
        assert!(find_command_position_matches(c, "pip").is_empty());
    }

    #[test]
    fn python_m_pip_not_command() {
        let c = "python -m pip install\n";
        assert!(find_command_position_matches(c, "pip").is_empty());
    }

    #[test]
    fn sudo_and_env_allow_command() {
        assert_eq!(
            replace_command_position("sudo pip install\n", "pip", "uv").0,
            "sudo uv install\n"
        );
        assert_eq!(
            replace_command_position("env FOO=1 pip install\n", "pip", "uv").0,
            "env FOO=1 uv install\n"
        );
    }

    #[test]
    fn sudo_and_time_option_flags_allow_command() {
        assert_eq!(
            replace_command_position("sudo -E pip install\n", "pip", "uv").0,
            "sudo -E uv install\n"
        );
        assert_eq!(
            replace_command_position("time -p pip install\n", "pip", "uv").0,
            "time -p uv install\n"
        );
    }

    #[test]
    fn sudo_user_flag_allows_command() {
        assert_eq!(
            replace_command_position("sudo -u root pip install\n", "pip", "uv").0,
            "sudo -u root uv install\n"
        );
        assert_eq!(
            replace_command_position("sudo --user alice pip install\n", "pip", "uv").0,
            "sudo --user alice uv install\n"
        );
        // Combined: user flag value then no-arg flags.
        assert_eq!(
            replace_command_position("sudo -u root -E pip install\n", "pip", "uv").0,
            "sudo -u root -E uv install\n"
        );
        assert_eq!(
            replace_command_position("sudo -g wheel pip install\n", "pip", "uv").0,
            "sudo -g wheel uv install\n"
        );
        // Still do not rewrite argument-position pip.
        assert_eq!(
            replace_command_position("echo -u pip\n", "pip", "uv").1,
            0,
            "echo -u pip is not command-position for pip"
        );
    }

    #[test]
    fn nice_timeout_stdbuf_wrappers_allow_command() {
        assert_eq!(
            replace_command_position("nice -n 10 pip install\n", "pip", "uv").0,
            "nice -n 10 uv install\n"
        );
        // Attached form is already an option flag.
        assert_eq!(
            replace_command_position("nice -n10 pip install\n", "pip", "uv").0,
            "nice -n10 uv install\n"
        );
        assert_eq!(
            replace_command_position("timeout 30 pip install\n", "pip", "uv").0,
            "timeout 30 uv install\n"
        );
        assert_eq!(
            replace_command_position("timeout 5s pip install\n", "pip", "uv").0,
            "timeout 5s uv install\n"
        );
        assert_eq!(
            replace_command_position("stdbuf -oL pip install\n", "pip", "uv").0,
            "stdbuf -oL uv install\n"
        );
        assert_eq!(
            replace_command_position("ionice -c 3 pip install\n", "pip", "uv").0,
            "ionice -c 3 uv install\n"
        );
        // Non-wrapper still does not rewrite argument-position tokens after numbers.
        assert_eq!(
            replace_command_position("echo 30 pip\n", "pip", "uv").1,
            0,
            "echo 30 pip is not command-position for pip"
        );
        assert_eq!(
            replace_command_position("5 pip install\n", "pip", "uv").1,
            0,
            "bare number is not a transparent wrapper"
        );
    }

    #[test]
    fn separators_allow_command() {
        for sep in ["&&", "|", ";"] {
            let c = format!("cmd1 {sep} pip install\n");
            let (out, n) = replace_command_position(&c, "pip", "uv");
            assert_eq!(n, 1, "sep={sep}");
            assert!(out.contains("uv install"), "sep={sep}: {out}");
        }
    }

    #[test]
    fn hyphenated_token() {
        let c = "my-tool run\n";
        let (out, n) = replace_command_position(c, "my-tool", "other");
        assert_eq!(n, 1);
        assert_eq!(out, "other run\n");
    }

    #[test]
    fn empty_token_is_noop() {
        let (out, n) = replace_command_position("pip install\n", "", "x");
        assert_eq!(n, 0);
        assert_eq!(out, "pip install\n");
    }

    #[test]
    fn multiple_command_positions_on_line() {
        // Second command after semicolon is rewritten; first too.
        let (out, n) = replace_command_position("pip install; pip list\n", "pip", "uv");
        assert_eq!(n, 2);
        assert_eq!(out, "uv install; uv list\n");
    }

    #[test]
    fn quoted_path_not_command_when_after_word() {
        // `echo pip` should not rewrite pip as command.
        let (_, n) = replace_command_position("echo pip\n", "pip", "uv");
        assert_eq!(n, 0);
    }

    #[test]
    fn multiline_wrapper_after_prior_line() {
        // Newlines must remain separators; do not peel into the previous line.
        let content = "echo hello\ntimeout 30 pip install\nnice -n 10 pip list\n";
        let (out, n) = replace_command_position(content, "pip", "uv");
        assert_eq!(n, 2, "out={out}");
        assert_eq!(
            out,
            "echo hello\ntimeout 30 uv install\nnice -n 10 uv list\n"
        );
    }
    #[test]
    fn crlf_multiline_wrapper() {
        let content = "echo hello\r\ntimeout 30 pip install\r\n";
        let (out, n) = replace_command_position(content, "pip", "uv");
        assert_eq!(n, 1, "out={out:?}");
        assert!(out.contains("uv install"), "{out:?}");
    }

    #[test]
    fn command_position_is_case_sensitive_literal() {
        let (_, n) = replace_command_position("PIP install\n", "pip", "uv");
        assert_eq!(n, 0, "literal match only");
    }
}
