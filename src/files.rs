//! File walking, binary detection, and text-read helpers shared by CLI commands.
//!
//! size-waiver: accepted single-domain bulk (policy #1408). One module owns
//! ignore-aware walks, glob/exclude filtering, binary/UTF-8 guards, and
//! path-missing helpers used by search/replace/tidy; co-located unit tests push
//! the file over 1000 lines. Do not split for LOC alone.
//!
//! # Text I/O honesty (#1894)
//!
//! - **Strict sole-path:** [`load_text_strict`] — binary → `BinaryError`, invalid UTF-8 → `InvalidEncodingError` (CLI, MCP/tx sole path, library `api::*`).
//! - **Soft content skip (walks):** [`try_read_text_file`] / [`read_text_file`] —
//!   binary / invalid UTF-8 → [`SoftTextSkip`] (content not agent-editable).
//! - **Unreadable is not content SoftSkip:** open/read IO is
//!   [`SoftTextSkip::Unreadable`]. Callers decide: directory walks may
//!   continue but must not report pattern `no_matches` when unreadable
//!   paths may have masked the scan; sole paths use Strict IO errors.
//! - Byte rule: [`classify_text_bytes`] (NUL probe + UTF-8).

#[cfg(feature = "cli")]
use crate::cli::global::GlobalFlags;
#[cfg(any(feature = "cli", feature = "files"))]
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
#[cfg(any(feature = "cli", feature = "files"))]
use ignore::WalkBuilder;
#[cfg(any(feature = "cli", feature = "files"))]
use ignore::WalkState;
use std::path::Path;
#[cfg(any(feature = "cli", feature = "files"))]
use std::path::PathBuf;
#[cfg(any(feature = "cli", feature = "files"))]
use std::sync::Mutex;

/// Compute a display-friendly relative path by stripping a `base` prefix.
///
/// Returns the relative portion if `path` is under `base`, otherwise returns
/// the original path unchanged. Used by diff headers, search results, and JSON
/// output so users see `src/main.rs` instead of `/home/user/project/src/main.rs`.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn relative_display<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base).unwrap_or(path)
}

/// Check if a string contains common regex metacharacters that suggest
/// the user intended a regex pattern but forgot `--regex` (or used `--literal`).
#[cfg(feature = "cli")]
pub(crate) fn has_regex_metacharacters(s: &str) -> bool {
    s.contains('\\')
        || s.contains('[')
        || s.contains('(')
        || s.contains('{')
        || s.contains('*')
        || s.contains('+')
        || s.contains('?')
        || s.contains('|')
        || s.contains('^')
        || s.contains('$')
}

pub fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    memchr::memchr(0, &data[..check_len]).is_some()
}

/// Result of classifying on-disk (or in-memory) bytes as agent-editable text.
///
/// Shared by CLI, MCP/tx, and library loaders so they cannot disagree about
/// "is this text?" (#1894). Binary uses the same 8 KiB NUL probe as [`is_binary`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextBytesKind {
    /// Valid UTF-8 text with no NUL in the binary probe window.
    Text(String),
    /// NUL in the first 8 KiB (or empty probe treated via [`is_binary`]).
    Binary,
    /// Not binary by NUL probe, but not valid UTF-8.
    InvalidUtf8,
}

/// Classify bytes as text, binary, or invalid UTF-8.
///
/// Empty input is **Text** (empty string). This is the single byte-level rule
/// for the text I/O honesty layer (#1894).
pub fn classify_text_bytes(bytes: &[u8]) -> TextBytesKind {
    if is_binary(bytes) {
        return TextBytesKind::Binary;
    }
    match String::from_utf8(bytes.to_vec()) {
        Ok(s) => TextBytesKind::Text(s),
        Err(_) => TextBytesKind::InvalidUtf8,
    }
}

/// Load a path as UTF-8 text under the **Strict** sole-path policy (#1894).
///
/// Use for explicit single-file mutators and sole-path reads (CLI/MCP/tx/`api`).
///
/// | Kind | Result |
/// |------|--------|
/// | Text | `Ok(String)` |
/// | Binary | `Err(BinaryError)` — `target is a binary file: {display}` (`error_kind: binary`) |
/// | Invalid UTF-8 | `Err(InvalidEncodingError)` — `target is not valid UTF-8 text: {display}` (`error_kind: invalid_encoding`) |
/// | Not a file (directory, FIFO, …) | `Err(InvalidInputError)` — `target is not a file: {display}` |
/// | Dangling symlink / special name | `Err(InvalidInputError)` — `target is not a file: {display}` (not IO NotFound) |
/// | IO NotFound | `Err` with `io::Error` + context `failed to read {display}` (`is_io_not_found`) |
/// | IO other (permission, …) | `Err(InvalidInputError)` — `failed to read {display}: {os error}` |
///
/// Binary / invalid UTF-8 are distinct from argument `invalid_input` so hosts can
/// recover overwrite without treating empty paths the same as content SoftSkip
/// (#1963). Non-NotFound IO is typed so agent JSON gets `error_kind: invalid_input`
/// and a single complete message that includes the OS error even when callers
/// format with `Display` / `to_string()` (which drops anyhow cause chains).
/// NotFound stays an IO error so [`crate::exit::is_io_not_found`] keeps working.
///
/// Directory walks and multi-path soft-skip must use [`read_text_file`] (or
/// `tx::read_and_probe`), not this function.
pub fn load_text_strict(path: &Path, display: &str) -> anyhow::Result<String> {
    use crate::ops::file::{PathEntryKind, classify_path_entry};
    let collapsed = crate::ops::file::windows_collapse_dest_path(path);
    let path = collapsed.as_path();
    crate::ops::file::ensure_not_windows_illegal_dest(path, display)?;
    match classify_path_entry(path) {
        PathEntryKind::RealDirectory => {
            return Err(crate::exit::InvalidInputError {
                msg: format!("target is not a file: {display}"),
            }
            .into());
        }
        PathEntryKind::Special if !path.exists() || !path.is_file() => {
            // Dangling symlink / FIFO / socket / symlink-to-dir: present
            // special name, not a missing path. Must not look like IO
            // NotFound (agents would create over the name). Live file
            // symlinks follow via fs::read (#1230).
            return Err(crate::exit::InvalidInputError {
                msg: format!("target is not a file: {display}"),
            }
            .into());
        }
        // RegularFile, live file symlink (Special + exists + is_file), Missing.
        PathEntryKind::RegularFile | PathEntryKind::Special | PathEntryKind::Missing => {}
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Keep io::Error in the chain for `is_io_not_found`, and put the
            // OS detail in the context string so Display (not only `{:#}`)
            // includes "No such file" for agent JSON (MPI 2026-07-23: explain
            // showed "cannot read X: failed to read X" with no OS detail).
            let msg = format!("failed to read {display}: {e}");
            return Err(anyhow::Error::new(e).context(msg));
        }
        Err(e) => {
            // Full message in InvalidInputError so Display and JSON envelopes
            // keep "Permission denied" without requiring `{:#}` (fixrealloop
            // 2026-07-23: CLI read / MCP read dropped the OS detail).
            return Err(crate::exit::InvalidInputError {
                msg: format!("failed to read {display}: {e}"),
            }
            .into());
        }
    };
    match classify_text_bytes(&bytes) {
        TextBytesKind::Text(s) => Ok(s),
        TextBytesKind::Binary => Err(crate::exit::BinaryError {
            msg: format!("target is a binary file: {display}"),
        }
        .into()),
        TextBytesKind::InvalidUtf8 => Err(crate::exit::InvalidEncodingError {
            msg: format!("target is not valid UTF-8 text: {display}"),
        }
        .into()),
    }
}

/// True when `path` is a regular file (or symlink to one) safe to open for
/// text/binary probes. FIFOs, sockets, devices, and directories return false so
/// callers never block forever on open (#2113 family).
fn is_openable_regular_file(path: &Path) -> bool {
    if crate::ops::file::is_windows_illegal_dest_path(path) {
        return false;
    }
    match std::fs::metadata(path) {
        Ok(m) => m.is_file(),
        Err(_) => false,
    }
}

/// Returns whether the file at `path` appears to be binary by reading only its
/// first 8 KiB (streaming, no full allocation for large files).
///
/// Heuristic: true when a NUL byte appears in the probe window (same rule as
/// [`is_binary`]). Returns **false** if the path cannot be opened or read
/// (missing, permission, FIFO/socket/device, …); callers that need a typed open
/// error should open the path themselves.
///
/// Public for embedder preflight (#1884) so hosts do not reimplement the
/// window size or probe semantics. Writers still enforce binary themselves
/// via [`load_text_strict`] or `ops::file::ensure_not_binary_file`.
/// Available with default features and under `features = ["files"]` (always
/// compiled; no `cli` gate).
pub fn is_binary_file(path: &Path) -> bool {
    // Never open special nodes: FIFO/socket open blocks forever.
    if !is_openable_regular_file(path) {
        return false;
    }
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut buf = [0u8; 8192];
    let n = match std::io::Read::read(&mut file, &mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    is_binary(&buf[..n])
}

/// Length of a Windows extended/device prefix whose `?` is not a glob.
/// `\\?\C:\file.txt` and `//?/C:/file.txt` must stay literal dests.
#[must_use]
fn windows_extended_prefix_len(path: &str) -> usize {
    let b = path.as_bytes();
    if b.len() >= 4 {
        let p = &b[..4];
        if p == br"\\?\" || p == b"//?/" || p == br"\\.\" || p == b"//./" {
            return 4;
        }
    }
    0
}

/// True when a dest looks like a glob Unix shells expand (`*.txt`, `sub/*.rs`).
/// Windows cmd and PowerShell pass those through, so scan dests must expand
/// them instead of peeling `not_found` / illegal dest.
/// `*` / `?` in a `\\?\` / `//?/` prefix are not glob metacharacters.
/// Always compiled: plan/tx path validation uses this without dest-glob expand.
#[must_use]
pub(crate) fn looks_like_glob_dest(path: &str) -> bool {
    let rest = &path[windows_extended_prefix_len(path)..];
    rest.as_bytes().iter().any(|b| matches!(b, b'*' | b'?'))
}

/// True when the user supplied explicit path roots and none of them exist.
///
/// Empty `paths` means the caller will default to `.` and is never "all
/// missing" here. Used so search/replace/tidy can distinguish path typos
/// (`not_found`) from pattern/whitespace soft success (`no_matches` / clean).
/// Glob dests are ignored: zero matches is pattern `no_matches`, not dest
/// `not_found`.
#[cfg(feature = "cli")]
pub(crate) fn all_explicit_paths_missing(paths: &[String], root: Option<&Path>) -> bool {
    if paths.is_empty() {
        return false;
    }
    let literals: Vec<&String> = paths.iter().filter(|p| !looks_like_glob_dest(p)).collect();
    if literals.is_empty() {
        // Only glob dests: zero matches is pattern `no_matches`, not dest `not_found`.
        return false;
    }
    literals.iter().all(|p| {
        let resolved = match root {
            Some(r) if !std::path::Path::new(p).is_absolute() => r.join(p),
            _ => std::path::PathBuf::from(p),
        };
        !crate::ops::file::path_entry_exists(&resolved)
    })
}

/// Like [`all_explicit_paths_missing`], but prefers `--files-from` entries when
/// set. Does not re-read stdin (`--files-from -`); those lists skip this check.
#[cfg(feature = "cli")]
pub(crate) fn all_scan_targets_missing(
    global: &GlobalFlags,
    paths: &[String],
    root: Option<&Path>,
) -> anyhow::Result<bool> {
    if global.files_from.as_deref() == Some("-") {
        return Ok(false);
    }
    if global.files_from.is_some() {
        let Some(files) = global.read_files_from()? else {
            return Ok(false);
        };
        return Ok(all_explicit_paths_missing(&files, root));
    }
    Ok(all_explicit_paths_missing(paths, root))
}

/// `--files-from` list entries that do not exist under `cwd` (agent JSON).
///
/// Returns `None` when files-from is unset, is stdin (`-`, single-read), or
/// every listed path exists. Used so soft-skips are visible under `--json`
/// even when `--quiet` suppresses stderr (#1756).
#[cfg(feature = "cli")]
pub(crate) fn files_from_missing_entries(
    global: &crate::cli::global::GlobalFlags,
    cwd: &Path,
) -> anyhow::Result<Option<Vec<String>>> {
    if global.files_from.as_deref() == Some("-") {
        return Ok(None);
    }
    let Some(files) = global.read_files_from()? else {
        return Ok(None);
    };
    Ok(missing_paths_under(cwd, &files))
}

/// Explicit CLI path args that do not exist under `cwd` (agent JSON `skipped`).
///
/// Mirrors [`files_from_missing_entries`] for positional paths so partial
/// multi-path replace/search is not `ok: true` with only stderr warnings.
/// Returns `None` when `paths` is empty (directory walk) or every path exists.
#[cfg(feature = "cli")]
#[must_use]
pub(crate) fn explicit_paths_missing_entries(cwd: &Path, paths: &[String]) -> Option<Vec<String>> {
    if paths.is_empty() {
        return None;
    }
    missing_paths_under(cwd, paths)
}

/// Prefer `--files-from` missing entries when set; otherwise explicit path args.
#[cfg(feature = "cli")]
pub(crate) fn scan_missing_entries(
    global: &crate::cli::global::GlobalFlags,
    cwd: &Path,
    paths: &[String],
) -> anyhow::Result<Option<Vec<String>>> {
    if global.files_from.is_some() {
        files_from_missing_entries(global, cwd)
    } else {
        Ok(explicit_paths_missing_entries(cwd, paths))
    }
}

#[cfg(feature = "cli")]
fn missing_paths_under(cwd: &Path, paths: &[String]) -> Option<Vec<String>> {
    let mut missing = Vec::new();
    for f in paths {
        if looks_like_glob_dest(f) {
            continue;
        }
        if !crate::ops::file::path_entry_exists(&cwd.join(f)) {
            missing.push(f.clone());
        }
    }
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

/// Reject an empty `--files-from` list as `invalid_input` (exit 1).
///
/// Soft `no_matches` (search) or clean success (tidy check) would mislead agents
/// into widening the pattern or treating the workspace as clean when zero paths
/// were scanned. Same contract as replace (#1796), applied to all scan commands.
#[cfg(feature = "cli")]
pub(crate) fn ensure_files_from_nonempty(
    global: &GlobalFlags,
    file_paths: &[PathBuf],
) -> anyhow::Result<()> {
    if global.files_from.is_some() && file_paths.is_empty() {
        return Err(crate::exit::InvalidInputError {
            msg: "empty --files-from path list (no files to scan)".into(),
        }
        .into());
    }
    Ok(())
}

/// Collect file paths from either `--files-from`, or by walking `paths` with
/// `ignore::WalkBuilder` (respects `.gitignore`).  When `root` is `Some`,
/// paths are joined with it before walking.  Tidy commands set
/// `include_hidden = true` so dotfiles are checked.
///
/// Pass `files_from_preload` when the caller already read `--files-from`
/// (including stdin `-`). Stdin can only be consumed once; re-reading yields
/// an empty list and breaks sole-binary / refused honesty.
#[cfg(feature = "cli")]
pub(crate) fn collect_file_paths_opts(
    paths: &[String],
    global: &GlobalFlags,
    include_hidden: bool,
    root: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    collect_file_paths_opts_with_list(paths, global, include_hidden, root, None, None)
}

/// Like [`collect_file_paths_opts`], with optional walk-time [`max_depth`].
///
/// `max_depth` is passed to [`ignore::WalkBuilder::max_depth`] so deep trees
/// are not entered (MCP `list_files` #2078). Semantics match component count
/// under each walk root: `Some(1)` is files directly under each root.
#[cfg(feature = "cli")]
pub(crate) fn collect_file_paths_opts_depth(
    paths: &[String],
    global: &GlobalFlags,
    include_hidden: bool,
    root: Option<&Path>,
    max_depth: Option<usize>,
) -> anyhow::Result<Vec<PathBuf>> {
    collect_file_paths_opts_with_list(paths, global, include_hidden, root, None, max_depth)
}

/// Like [`collect_file_paths_opts`], but accepts a pre-read `--files-from` list.
#[cfg(feature = "cli")]
pub(crate) fn collect_file_paths_opts_with_list(
    paths: &[String],
    global: &GlobalFlags,
    include_hidden: bool,
    root: Option<&Path>,
    files_from_preload: Option<&[String]>,
    max_depth: Option<usize>,
) -> anyhow::Result<Vec<PathBuf>> {
    let files_owned;
    let files_from: Option<&[String]> = if let Some(pre) = files_from_preload {
        Some(pre)
    } else if global.files_from.is_some() {
        files_owned = global.read_files_from()?;
        files_owned.as_deref()
    } else {
        None
    };
    if let Some(files) = files_from {
        // --files-from entries must honor --contain (paths may escape even when
        // CLI positional paths were empty or in-workspace).
        if let Some(r) = root {
            global.check_paths_contained(r, files)?;
        }
        return Ok(files
            .iter()
            .map(|f| {
                let raw = match root {
                    Some(r) => r.join(f),
                    None => PathBuf::from(f),
                };
                crate::ops::file::windows_collapse_dest_path(&raw)
            })
            .collect());
    }
    let defaults;
    let effective: &[String] = if paths.is_empty() {
        defaults = [".".to_string()];
        &defaults
    } else {
        paths
    };
    let resolve = |p: &str| -> PathBuf {
        let raw = match root {
            Some(r) => r.join(p),
            None => PathBuf::from(p),
        };
        crate::ops::file::windows_collapse_dest_path(&raw)
    };
    // Unix shells expand `*.txt` before exec. Windows cmd/PowerShell do not.
    // Split glob dests from literal walk roots so `search KEEP *.txt` is
    // `--glob *.txt` over `.`, not dest `not_found`.
    let mut walk_specs: Vec<String> = Vec::new();
    let mut dest_globs: Vec<String> = Vec::new();
    for p in effective {
        let resolved = resolve(p);
        // On Windows, `exists("C:\\ws\\*.txt")` is true when any .txt
        // exists (wildcard FindFirstFile). That skipped dest-glob expand
        // and walked the tree. `*` / `?` cannot be a real Win32 name.
        let as_glob = looks_like_glob_dest(p)
            && (cfg!(windows) || !crate::ops::file::path_entry_exists(&resolved));
        if as_glob {
            dest_globs.push(p.clone());
        } else {
            walk_specs.push(p.clone());
        }
    }
    let dest_glob_only = walk_specs.is_empty() && !dest_globs.is_empty();
    let dest_walk_depth = dest_glob_walk_max_depth(&dest_globs);
    let literal_specs = walk_specs.clone();
    // Mix + no `**`: walk literals unbounded; walk `.` separately with dest
    // depth so `search KEEP src *.txt` does not recurse the whole cwd.
    let mix_capped_dot = !dest_glob_only
        && dest_walk_depth.is_some()
        && !walk_specs.iter().any(|s| s == "." || s == "./");
    if walk_specs.is_empty() {
        walk_specs.push(".".to_string());
    } else if !dest_globs.is_empty()
        && !walk_specs.iter().any(|s| s == "." || s == "./")
        && !mix_capped_dot
    {
        // Mix + `**`: walk cwd unbounded so dest `**/*.txt` can match.
        walk_specs.push(".".to_string());
    }
    let dest_glob_matcher = build_dest_glob_matcher(&dest_globs)?;
    let effective: &[String] = &walk_specs;
    // Explicit walk roots under --contain (defense-in-depth for callers that
    // skip an early check_paths_contained on the same list).
    if let Some(r) = root {
        global.check_paths_contained(r, effective)?;
    }
    // Warn about nonexistent user-supplied paths so typos are visible
    // instead of silently producing an empty result set (exit 3 / soft success).
    // Under --json/--jsonl, callers list soft-misses in `skipped[]`; skip the
    // human stderr line so agents that merge streams do not treat it as a hard
    // failure (#1797).
    // Callers that want a hard not_found should also check
    // [`all_explicit_paths_missing`].
    for p in effective {
        let resolved = resolve(p);
        if !crate::ops::file::path_entry_exists(&resolved) && !global.json && !global.jsonl {
            eprintln!(
                "patchloom: {}: No such file or directory",
                resolved.display()
            );
        }
    }

    let first = resolve(&effective[0]);
    let mut builder = WalkBuilder::new(&first);
    apply_platform_ignore_case(&mut builder);
    for p in &effective[1..] {
        builder.add(resolve(p));
    }
    if include_hidden {
        builder.hidden(false);
    }
    // Walk-time prune: ignore crate depth is per root (depth 1 = root +
    // immediate children). Matches list_files max_depth component count (#2078).
    // Dest-glob-only without `**` also caps here (`*.txt` is cwd files).
    let walk_depth = if dest_glob_only {
        match (max_depth, dest_walk_depth) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        }
    } else {
        max_depth
    };
    if let Some(depth) = walk_depth {
        builder.max_depth(Some(depth));
    }
    // Never enter .git / .patchloom; prune exclude prefixes (vendor/**) at walk
    // time so we do not descend into trees that apply_exclude_globs would drop.
    let exclude_set = build_glob_matcher(&global.exclude)?;
    attach_walk_entry_filter(
        &mut builder,
        exclude_set.clone(),
        root.map(Path::to_path_buf),
    );
    // Support advanced layered ignores (e.g. .agentignore) for parity with library
    // `collect_file_paths_with_ignores` and `api::SearchOptions` (#821).
    for name in &global.ignore_file {
        builder.add_custom_ignore_filename(name);
    }
    let collected: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    // Flush-on-drop wrapper so entries remaining in a thread-local batch
    // are merged into the shared vec when the per-thread worker is dropped.
    struct FlushOnDrop<'a> {
        batch: Vec<PathBuf>,
        target: &'a Mutex<Vec<PathBuf>>,
    }
    impl Drop for FlushOnDrop<'_> {
        fn drop(&mut self) {
            if !self.batch.is_empty() {
                self.target
                    .lock()
                    .expect("file list mutex")
                    .append(&mut self.batch);
            }
        }
    }

    builder.build_parallel().run(|| {
        let mut state = FlushOnDrop {
            batch: Vec::with_capacity(256),
            target: &collected,
        };
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            // Skip VCS / internal dirs even when include_hidden=true (tidy).
            // .git: objects are binary; walking them emits invalid-UTF-8 noise
            // (fixrealloop 2026-07-15). .patchloom: backup storage (#1349).
            if should_skip_walk_dirname(entry.file_name()) {
                return WalkState::Skip;
            }
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                state.batch.push(entry.into_path());
                if state.batch.len() >= 256 {
                    state
                        .target
                        .lock()
                        .expect("file list mutex")
                        .append(&mut state.batch);
                }
            }
            WalkState::Continue
        })
    });
    let mut paths = collected.into_inner().expect("all walkers done");

    if mix_capped_dot {
        let dest_dot_depth = match (max_depth, dest_walk_depth) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (_, Some(b)) => Some(b),
            (Some(a), None) => Some(a),
            (None, None) => None,
        };
        let mut dest_builder = WalkBuilder::new(resolve("."));
        apply_platform_ignore_case(&mut dest_builder);
        if include_hidden {
            dest_builder.hidden(false);
        }
        if let Some(depth) = dest_dot_depth {
            dest_builder.max_depth(Some(depth));
        }
        attach_walk_entry_filter(
            &mut dest_builder,
            exclude_set.clone(),
            root.map(Path::to_path_buf),
        );
        for name in &global.ignore_file {
            dest_builder.add_custom_ignore_filename(name);
        }
        paths.extend(collect_files_from_walk_builder(dest_builder));
    }

    if let Some(ref matcher) = dest_glob_matcher {
        // Root-relative only. Filename fallback would make `*.txt` match
        // `sub/a.txt`, but Unix shells and `dir *.txt` stay in cwd.
        let dest_roots: Vec<PathBuf> = match root {
            Some(r) => vec![normalize_glob_root(r.to_path_buf())],
            None => vec![PathBuf::from(".")],
        };
        if dest_glob_only {
            paths.retain(|p| matches_dest_glob(p, matcher, &dest_roots));
        } else {
            let literal_roots: Vec<PathBuf> = literal_specs.iter().map(|s| resolve(s)).collect();
            paths.retain(|p| {
                literal_roots.iter().any(|r| p == r || p.starts_with(r))
                    || matches_dest_glob(p, matcher, &dest_roots)
            });
        }
    }

    // Explicit file path args must not be dropped by exclude. Config like
    // exclude.globs = ["vendor/**"] is for walks; a targeted
    // `replace … vendor/pkg/x.js` must still hit that file. Directory roots
    // keep exclude for their contents. --files-from already skips exclude.
    let explicit_files: Vec<PathBuf> = effective
        .iter()
        .map(|p| resolve(p))
        .filter(|p| p.is_file())
        .collect();

    // File-level excludes (*.rs) and defense-in-depth after walk-time prune.
    apply_exclude_globs(&mut paths, &global.exclude, root)?;

    for f in explicit_files {
        if !paths.iter().any(|p| p == &f) {
            paths.push(f);
        }
    }
    Ok(paths)
}

/// Strip leading `./` / `.\` so `./*.txt` and `.\*.txt` match cwd files.
/// Unix shells expand those before exec; Windows cmd/PowerShell pass them
/// through.
#[cfg(any(feature = "cli", feature = "files"))]
fn strip_leading_dot_slash(pattern: &str) -> &str {
    let mut p = pattern;
    loop {
        if let Some(rest) = p.strip_prefix("./") {
            p = rest;
            continue;
        }
        if let Some(rest) = p.strip_prefix(".\\") {
            p = rest;
            continue;
        }
        break;
    }
    p
}

/// Compile a user `--glob` / exclude / `for_each` pattern.
///
/// Windows file names are case-insensitive. `*.txt` must match `Hit.TXT`
/// the same way `dir *.txt` does. Linux stays case-sensitive.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn compile_user_glob(pattern: &str) -> Result<Glob, globset::Error> {
    let stripped = strip_leading_dot_slash(pattern);
    // Win32 `\` is a separator. globset otherwise treats it as a literal
    // (or escape), so `*.txt` matches `sub\a.txt` and `sub\*.txt` misses
    // a `/`-normalized relative path.
    GlobBuilder::new(stripped)
        .case_insensitive(cfg!(windows))
        .build()
}

/// Walk depth for dest-glob retain. `None` is unbounded (`**` or no dest-glob).
/// `*.txt` / `./*.txt` is cwd files (depth 1). `sub/*.txt` is one directory
/// (depth 2). `--glob *.txt` is not dest-glob and stays recursive.
#[cfg(feature = "cli")]
fn dest_glob_walk_max_depth(globs: &[String]) -> Option<usize> {
    if globs.is_empty() {
        return None;
    }
    let mut cap = 0usize;
    for pattern in globs {
        let stripped = strip_leading_dot_slash(pattern);
        let normalized = if cfg!(windows) && stripped.contains('\\') {
            stripped.replace('\\', "/")
        } else {
            stripped.to_string()
        };
        if normalized.contains("**") {
            return None;
        }
        cap = cap.max(normalized.matches('/').count() + 1);
    }
    Some(cap)
}

/// Dest-glob with no `**` (`*.txt`, `sub/*.txt`). Not recursive.
#[cfg(feature = "cli")]
#[must_use]
pub(crate) fn dest_glob_is_non_recursive(path: &str) -> bool {
    looks_like_glob_dest(path) && dest_glob_walk_max_depth(&[path.to_string()]).is_some()
}

/// Last `/` or `\` component of a dest glob (`*.txt`, `sub/*.txt` → `*.txt`).
#[cfg(feature = "cli")]
fn dest_glob_leaf(dest: &str) -> &str {
    dest.rsplit(['/', '\\']).next().unwrap_or(dest)
}

/// Dest-glob that walks cwd files only (`*.txt`, `./*.txt`). `sub/*.txt` is not.
#[cfg(feature = "cli")]
fn dest_glob_is_cwd_only(path: &str) -> bool {
    looks_like_glob_dest(path) && dest_glob_walk_max_depth(&[path.to_string()]) == Some(1)
}

/// Dest-glob scope rule for agent JSON `error` (not stderr-only).
/// `*.txt` is cwd-only; `sub/*.txt` is that directory only. Remedy uses dest leaf.
#[cfg(feature = "cli")]
#[must_use]
pub(crate) fn dest_glob_cwd_only_rule(paths: &[String]) -> Option<String> {
    let dest = paths.iter().find(|p| dest_glob_is_non_recursive(p))?;
    let leaf = dest_glob_leaf(dest);
    let scope = if dest_glob_is_cwd_only(dest) {
        "the current directory only"
    } else {
        "that directory only"
    };
    Some(format!(
        "dest `{dest}` matches files in {scope}; use `**/{leaf}` or `--glob '{leaf}'` for nested files"
    ))
}

/// Append [`dest_glob_cwd_only_rule`] so JSON `error` names dest-glob scope.
#[cfg(feature = "cli")]
#[must_use]
pub(crate) fn with_dest_glob_cwd_only_rule(msg: &str, paths: &[String]) -> String {
    match dest_glob_cwd_only_rule(paths) {
        Some(rule) => format!("{msg}. {rule}"),
        None => msg.to_string(),
    }
}

/// Skip the `-i` tip when dest-glob expanded zero files or dest-glob
/// dropped nested-only hits (cwd-only dest such as `*.txt`).
/// Dest `sub/*.txt` with files found is a content miss: keep the `-i` tip.
#[cfg(feature = "cli")]
#[must_use]
pub(crate) fn dest_glob_skip_case_tip(paths: &[String], dest_glob_files_empty: bool) -> bool {
    if !paths.iter().any(|p| looks_like_glob_dest(p)) {
        return false;
    }
    dest_glob_files_empty || paths.iter().any(|p| dest_glob_is_cwd_only(p))
}

/// Dest-glob compile: `/` separators and `*` does not cross directories.
/// Unix shells and `dir *.txt` stay in one directory; `**` is recursive.
#[cfg(feature = "cli")]
fn compile_dest_glob(pattern: &str) -> Result<Glob, globset::Error> {
    let stripped = strip_leading_dot_slash(pattern);
    let normalized = if cfg!(windows) && stripped.contains('\\') {
        stripped.replace('\\', "/")
    } else {
        stripped.to_string()
    };
    GlobBuilder::new(&normalized)
        .case_insensitive(cfg!(windows))
        .literal_separator(true)
        .build()
}

/// Git on Windows defaults `core.ignorecase=true`. Honor that so
/// `.gitignore` `*.log` also drops `app.LOG` (R121 live red).
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn apply_platform_ignore_case(builder: &mut WalkBuilder) {
    builder.ignore_case_insensitive(cfg!(windows));
}

#[cfg(feature = "cli")]
fn build_dest_glob_matcher(globs: &[String]) -> anyhow::Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        builder.add(compile_dest_glob(pattern)?);
    }
    Ok(Some(builder.build()?))
}

/// Build a compiled glob matcher from globs, or `None` if no globs given.
/// Available for library use when "files" feature is enabled.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn build_glob_matcher(globs: &[String]) -> anyhow::Result<Option<GlobSet>> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in globs {
        builder.add(compile_user_glob(pattern)?);
    }
    Ok(Some(builder.build()?))
}

/// Build from GlobalFlags (cli only).
#[cfg(feature = "cli")]
pub(crate) fn build_glob_matcher_from_global(
    global: &GlobalFlags,
) -> anyhow::Result<Option<GlobSet>> {
    build_glob_matcher(&global.glob)
}

/// Collect roots used for matching globs against walked files.
/// Lib version.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn collect_glob_roots(paths: &[PathBuf], root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for path in paths {
        let resolved = match root {
            Some(r) => r.join(path),
            None => path.clone(),
        };
        let glob_root = if resolved.is_file() {
            resolved
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| resolved.clone())
        } else {
            resolved.clone()
        };
        let glob_root = normalize_glob_root(glob_root);
        if !roots.contains(&glob_root) {
            roots.push(glob_root);
        }
    }

    roots
}

/// Collect roots from GlobalFlags (cli only, for --files-from etc).
#[cfg(feature = "cli")]
pub(crate) fn collect_glob_roots_from_global(
    paths: &[String],
    global: &GlobalFlags,
    root: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    if global.files_from.is_some() {
        return Ok(root.map(|r| vec![r.to_path_buf()]).unwrap_or_default());
    }

    let defaults;
    let effective: &[String] = if paths.is_empty() {
        defaults = [".".to_string()];
        &defaults
    } else {
        paths
    };

    let paths_buf: Vec<PathBuf> = effective
        .iter()
        .map(|p| match root {
            Some(r) => r.join(p),
            None => PathBuf::from(p),
        })
        .collect();

    Ok(collect_glob_roots(&paths_buf, root))
}

#[cfg(any(feature = "cli", feature = "files"))]
fn normalize_glob_root(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

#[cfg(any(feature = "cli", feature = "files"))]
fn glob_matches_path(path: &Path, matcher: &GlobSet) -> bool {
    matcher.is_match(path) || path.file_name().is_some_and(|name| matcher.is_match(name))
}

/// Dest-glob retain: cwd-relative path with `/` separators.
/// Do not match the absolute walk path: on Windows globset treats `\` as
/// a normal character, so `*.txt` would match `C:\ws\sub\a.txt`.
/// `*.txt` is cwd files, like a Unix shell or `dir *.txt`. `--glob`
/// still uses [`matches_glob_with_roots`] (recursive filename match).
#[cfg(feature = "cli")]
fn matches_dest_glob(path: &Path, matcher: &GlobSet, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        if relative.as_os_str().is_empty() {
            return false;
        }
        let rel = relative.to_string_lossy().replace('\\', "/");
        matcher.is_match(rel.as_str())
    })
}

/// Check whether `path` matches any of the globs, either directly or relative
/// to one of the provided roots (always true if no globs).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn matches_glob_with_roots(path: &Path, matcher: Option<&GlobSet>, roots: &[PathBuf]) -> bool {
    match matcher {
        None => true,
        Some(m) => {
            matches_glob(path, Some(m))
                || roots.iter().any(|root| {
                    path.strip_prefix(root).ok().is_some_and(|relative| {
                        !relative.as_os_str().is_empty() && matches_glob(relative, Some(m))
                    })
                })
        }
    }
}

/// Check whether `path` matches any of the globs (always true if no globs).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn matches_glob(path: &Path, matcher: Option<&GlobSet>) -> bool {
    match matcher {
        None => true,
        Some(m) => glob_matches_path(path, m),
    }
}

/// Why a soft walk skipped a path (#1894).
///
/// **Content skips** ([`Binary`], [`InvalidUtf8`]) mean "not agent-editable
/// text." **[`Unreadable`]** is operational (permission/IO), not a content
/// SoftSkip: sole paths should hard-fail via [`load_text_strict`]; multi-path
/// walks may continue but must not report pattern `no_matches` when any
/// unreadable path may have masked the scan.
/// **[`NotRegularFile`]** is FIFO/socket/device/directory (or other non-regular
/// entry): open would hang or is not text; multi-path `refused[]` reason is
/// `not_regular_file` (not `unreadable`) so agents do not retry chmod
/// (fixrealloop 2026-08-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SoftTextSkip {
    /// NUL in the binary probe window.
    Binary,
    /// Valid NUL probe but not UTF-8.
    InvalidUtf8,
    /// Open/read/metadata failed (missing, permission, …).
    Unreadable,
    /// Path exists but is not a regular file (FIFO, socket, device, directory).
    /// Appended after [`Unreadable`] (0.25 honesty; fixrealloop multi-path refuse).
    NotRegularFile,
}

impl SoftTextSkip {
    /// Stable agent-facing reason for `refused[]` / logs.
    pub fn as_reason(self) -> &'static str {
        match self {
            SoftTextSkip::Binary => "binary",
            SoftTextSkip::InvalidUtf8 => "invalid_utf8",
            SoftTextSkip::Unreadable => "unreadable",
            SoftTextSkip::NotRegularFile => "not_regular_file",
        }
    }

    /// Content SoftSkip (binary / invalid UTF-8), not operational unreadable.
    pub fn is_content_skip(self) -> bool {
        matches!(self, SoftTextSkip::Binary | SoftTextSkip::InvalidUtf8)
    }
}

/// Soft-path text load with typed skip reason (#1894).
///
/// Empty files return `Ok("")`. For sole-path mutators use [`load_text_strict`].
///
/// For files larger than 8 KiB, only the first 8 KiB are read initially
/// for the binary check. If the file is binary, no further I/O occurs.
///
/// Never opens FIFOs, sockets, or devices (would block forever); those
/// return [`SoftTextSkip::NotRegularFile`] when the path exists as a non-regular
/// entry, else [`SoftTextSkip::Unreadable`] when missing / metadata fails.
pub fn try_read_text_file(path: &Path) -> Result<String, SoftTextSkip> {
    use std::io::Read;

    let collapsed = crate::ops::file::windows_collapse_dest_path(path);
    let path = collapsed.as_path();

    // `\\.\CON` / `\\.\NUL` / `\\.\pipe\...` are not files. Open hangs
    // (console) or is a device. Same refuse as dest writes (#2322).
    if crate::ops::file::is_windows_illegal_dest_path(path) {
        return Err(SoftTextSkip::NotRegularFile);
    }

    // Metadata-first: `File::open` on a FIFO blocks until a writer connects.
    if !is_openable_regular_file(path) {
        // Same kind as dest-symlink refuse: classify_path_entry, not raw
        // FileType::is_file. Windows dangling file symlinks report is_file
        // true; they must skip as NotRegularFile, not Unreadable.
        use crate::ops::file::{PathEntryKind, classify_path_entry};
        return match classify_path_entry(path) {
            PathEntryKind::Missing | PathEntryKind::RegularFile => Err(SoftTextSkip::Unreadable),
            PathEntryKind::Special | PathEntryKind::RealDirectory => {
                Err(SoftTextSkip::NotRegularFile)
            }
        };
    }

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Err(SoftTextSkip::Unreadable),
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len() as usize,
        Err(_) => return Err(SoftTextSkip::Unreadable),
    };
    if file_len == 0 {
        return Ok(String::new());
    }

    // For files larger than the binary-check window, read just the header
    // first. This avoids allocating megabytes for large binary files that
    // the walker did not filter out.
    const BINARY_CHECK_LEN: usize = 8192;
    if file_len > BINARY_CHECK_LEN {
        let mut header = [0u8; BINARY_CHECK_LEN];
        let n = match file.read(&mut header) {
            Ok(n) => n,
            Err(_) => return Err(SoftTextSkip::Unreadable),
        };
        if is_binary(&header[..n]) {
            return Err(SoftTextSkip::Binary);
        }
        // Header is text; now read the remainder into a single allocation.
        let mut bytes = Vec::with_capacity(file_len);
        bytes.extend_from_slice(&header[..n]);
        if file.read_to_end(&mut bytes).is_err() {
            return Err(SoftTextSkip::Unreadable);
        }
        return match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => Err(SoftTextSkip::InvalidUtf8),
        };
    }

    // Small file: read all at once (single syscall).
    let mut bytes = Vec::with_capacity(file_len);
    if file.read_to_end(&mut bytes).is_err() {
        return Err(SoftTextSkip::Unreadable);
    }

    match classify_text_bytes(&bytes) {
        TextBytesKind::Text(s) => Ok(s),
        TextBytesKind::Binary => Err(SoftTextSkip::Binary),
        TextBytesKind::InvalidUtf8 => Err(SoftTextSkip::InvalidUtf8),
    }
}

/// Soft-skip text load for walks; collapses all skip reasons to `None`.
///
/// Prefer [`try_read_text_file`] when unreadable must not be confused with
/// content SoftSkip (e.g. AST directory rename). Empty files return `Some("")`.
pub fn read_text_file(path: &Path) -> Option<String> {
    try_read_text_file(path).ok()
}

/// Internal version with optional diagnostic logging for CLI commands.
///
/// Re-opens on unreadable only to print the OS error (tests assert permission
/// strings); content SoftSkip logs stay reason-only.
#[cfg(feature = "cli")]
pub(crate) fn read_text_file_logged(path: &Path, cmd: &str, quiet: bool) -> Option<String> {
    match try_read_text_file(path) {
        Ok(s) => Some(s),
        Err(SoftTextSkip::Binary) => None,
        Err(SoftTextSkip::InvalidUtf8) => {
            if !quiet {
                eprintln!("{cmd}: skipping {} (invalid UTF-8)", path.display());
            }
            None
        }
        Err(SoftTextSkip::NotRegularFile) => {
            if !quiet {
                eprintln!("{cmd}: skipping {}: not a regular file", path.display());
            }
            None
        }
        Err(SoftTextSkip::Unreadable) => {
            if !quiet {
                // Regular-file path only: re-open for OS permission detail.
                // Specials use NotRegularFile and never reach here.
                let detail = std::fs::File::open(path)
                    .err()
                    .map(|e| e.to_string())
                    .or_else(|| std::fs::metadata(path).err().map(|e| e.to_string()))
                    .unwrap_or_else(|| "unreadable".into());
                eprintln!("{cmd}: skipping {}: {detail}", path.display());
            }
            None
        }
    }
}

/// Simple file collection for library use (sequential for simplicity; full parallel in par_process_files).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn collect_file_paths(root: &Path, include_hidden: bool) -> anyhow::Result<Vec<PathBuf>> {
    collect_file_paths_with_ignores(root, &[], &[], include_hidden)
}

/// Whether `path` is excluded by the same rules as [`apply_exclude_globs`].
///
/// Checks the full path, the filename fallback, and (when `root` is set) the
/// path relative to that root so `vendor/**` matches `<root>/vendor/lib.rs`.
#[cfg(any(feature = "cli", feature = "files"))]
fn exclude_path_matches(path: &Path, root: Option<&Path>, matcher: &GlobSet) -> bool {
    if glob_matches_path(path, matcher) {
        return true;
    }
    if let Some(r) = root
        && let Ok(rel) = path.strip_prefix(r)
        && !rel.as_os_str().is_empty()
        && matcher.is_match(rel)
    {
        return true;
    }
    false
}

/// Drop paths that match exclude globs (post-filter).
#[cfg(any(feature = "cli", feature = "files"))]
fn apply_exclude_globset(paths: &mut Vec<PathBuf>, matcher: &GlobSet, root: Option<&Path>) {
    paths.retain(|p| !exclude_path_matches(p, root, matcher));
}

/// Apply exclude glob patterns to a list of paths (post-filter).
/// Shared to avoid duplication between collect_file_paths_with_ignores and
/// the advanced logic in collect_file_paths_opts.
///
/// When `root` is provided, each path is also tested as a relative path
/// (stripped from the root prefix) so that patterns like `vendor/**` match
/// files at `<root>/vendor/lib.rs` even though the full path is absolute.
#[cfg(any(feature = "cli", feature = "files"))]
fn apply_exclude_globs(
    paths: &mut Vec<PathBuf>,
    patterns: &[String],
    root: Option<&Path>,
) -> anyhow::Result<()> {
    if let Some(ex) = build_glob_matcher(patterns)? {
        apply_exclude_globset(paths, &ex, root);
    }
    Ok(())
}

/// True when exclude globs would drop every descendant of `dir`.
///
/// Requires a synthetic child *and* grandchild to match so a glob that only
/// hits one path shape does not prune the rest. Same matcher as
/// [`apply_exclude_globs`]; no extra exclude syntax.
#[cfg(any(feature = "cli", feature = "files"))]
fn should_prune_excluded_directory(dir: &Path, root: Option<&Path>, matcher: &GlobSet) -> bool {
    const SENTINEL: &str = "__walk_prune__";
    let child = dir.join(SENTINEL);
    let nested = child.join(SENTINEL);
    exclude_path_matches(&child, root, matcher) && exclude_path_matches(&nested, root, matcher)
}

/// Skip `.git` / `.patchloom` and directories fully covered by exclude globs.
#[cfg(any(feature = "cli", feature = "files"))]
fn attach_walk_entry_filter(
    builder: &mut WalkBuilder,
    exclude: Option<GlobSet>,
    root: Option<PathBuf>,
) {
    builder.filter_entry(move |e| {
        if should_skip_walk_dirname(e.file_name()) {
            return false;
        }
        if let Some(ref matcher) = exclude
            && e.file_type().is_some_and(|ft| ft.is_dir())
            && should_prune_excluded_directory(e.path(), root.as_deref(), matcher)
        {
            return false;
        }
        true
    });
}

/// Directory basenames we never descend into when walking the tree.
///
/// Applied even with `include_hidden=true` (tidy needs dotfiles but not VCS
/// object stores or Patchloom backup sessions). Shared by CLI/library collectors
/// and plan/tx `ast.rename` directory walks.
#[cfg(any(feature = "cli", feature = "files"))]
pub(crate) fn should_skip_walk_dirname(name: &std::ffi::OsStr) -> bool {
    name == ".git" || name == ".patchloom"
}

/// Parallel file listing from a configured [`WalkBuilder`].
///
/// Same engine as CLI [`collect_file_paths_opts`] (`build_parallel` + batched
/// merge). Callers still apply exclude globs / explicit-file keepers.
#[cfg(any(feature = "cli", feature = "files"))]
fn collect_files_from_walk_builder(builder: WalkBuilder) -> Vec<PathBuf> {
    let collected: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    // Flush-on-drop so a thread-local batch merges when the worker is dropped.
    struct FlushOnDrop<'a> {
        batch: Vec<PathBuf>,
        target: &'a Mutex<Vec<PathBuf>>,
    }
    impl Drop for FlushOnDrop<'_> {
        fn drop(&mut self) {
            if !self.batch.is_empty() {
                self.target
                    .lock()
                    .expect("file list mutex")
                    .append(&mut self.batch);
            }
        }
    }

    builder.build_parallel().run(|| {
        let mut state = FlushOnDrop {
            batch: Vec::with_capacity(256),
            target: &collected,
        };
        Box::new(move |result| {
            let Ok(entry) = result else {
                return WalkState::Continue;
            };
            if should_skip_walk_dirname(entry.file_name()) {
                return WalkState::Skip;
            }
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                state.batch.push(entry.into_path());
                if state.batch.len() >= 256 {
                    state
                        .target
                        .lock()
                        .expect("file list mutex")
                        .append(&mut state.batch);
                }
            }
            WalkState::Continue
        })
    });
    collected.into_inner().expect("all walkers done")
}

/// Collect files while respecting .gitignore + custom ignore files (e.g. .agentignore)
/// + additional exclude globs.
///
/// This is the reusable primitive for library consumers
/// who want the same precedence as `api::search_directory` (#813).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn collect_file_paths_with_ignores(
    root: &Path,
    custom_ignore_filenames: &[String],
    exclude_patterns: &[String],
    include_hidden: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    let mut builder = WalkBuilder::new(root);
    apply_platform_ignore_case(&mut builder);
    if include_hidden {
        builder.hidden(false);
    }
    // Do not enter .git / .patchloom; prune exclude prefixes before descending.
    let exclude_set = build_glob_matcher(exclude_patterns)?;
    attach_walk_entry_filter(&mut builder, exclude_set.clone(), Some(root.to_path_buf()));
    for name in custom_ignore_filenames {
        builder.add_custom_ignore_filename(name);
    }
    let mut paths = collect_files_from_walk_builder(builder);
    apply_exclude_globs(&mut paths, exclude_patterns, Some(root))?;
    Ok(paths)
}

/// Process file paths using adaptive parallelism via `std::thread::scope`.
///
/// Files are split into chunks (one per available core). The calling thread
/// processes the first chunk immediately while spawned threads handle the
/// rest. Thread creation cost is ~0.05ms per thread (vs ~2ms for rayon's
/// global thread pool init), so overhead is near-zero even for small
/// workloads. For large workloads, all cores run concurrently.
#[cfg(any(feature = "cli", feature = "files"))]
pub fn par_process_files<T, F>(
    paths: &[PathBuf],
    glob_matcher: Option<&GlobSet>,
    glob_roots: &[PathBuf],
    f: F,
) -> Vec<T>
where
    T: Send,
    F: Fn(&Path) -> Option<T> + Sync,
{
    fn process_slice<T, F>(
        paths: &[PathBuf],
        glob_matcher: Option<&GlobSet>,
        glob_roots: &[PathBuf],
        f: &F,
    ) -> Vec<T>
    where
        T: Send,
        F: Fn(&Path) -> Option<T> + Sync,
    {
        paths
            .iter()
            .filter(|p| matches_glob_with_roots(p, glob_matcher, glob_roots))
            .filter_map(|p| f(p.as_path()))
            .collect()
    }

    let num_splits = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(paths.len());

    if num_splits <= 1 {
        return process_slice(paths, glob_matcher, glob_roots, &f);
    }

    let chunk_size = paths.len().div_ceil(num_splits);
    let chunks: Vec<&[PathBuf]> = paths.chunks(chunk_size).collect();

    std::thread::scope(|s| {
        // Spawn threads for all chunks except the first.
        let handles: Vec<_> = chunks[1..]
            .iter()
            .map(|chunk| s.spawn(|| process_slice(chunk, glob_matcher, glob_roots, &f)))
            .collect();

        // Process the first chunk on the calling thread immediately.
        let mut results = process_slice(chunks[0], glob_matcher, glob_roots, &f);

        // Collect results from spawned threads.
        //
        // This `expect` is not a recovery path: release builds set
        // `panic = "abort"` (`Cargo.toml`), so a panicking worker aborts the
        // process and never returns a `join` error here. It documents the
        // invariant and stays reachable under the unwinding dev profile used
        // by `cargo test`. See #184 for why the signature is not `Result`, and
        // #2379 for the decision to keep `abort`.
        for handle in handles {
            results.extend(handle.join().expect("worker thread panicked"));
        }

        results
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── has_regex_metacharacters ──────────────────────────────────────

    #[test]
    #[cfg(feature = "cli")]
    fn plain_text_has_no_regex_meta() {
        assert!(!has_regex_metacharacters("hello world"));
        assert!(!has_regex_metacharacters("foo-bar_baz"));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn regex_patterns_detected() {
        assert!(has_regex_metacharacters("fn\\s+main"));
        assert!(has_regex_metacharacters("v1\\.0"));
        assert!(has_regex_metacharacters("[a-z]+"));
        assert!(has_regex_metacharacters("(group)"));
        assert!(has_regex_metacharacters("a|b"));
        assert!(has_regex_metacharacters("^start"));
        assert!(has_regex_metacharacters("end$"));
    }

    // ── ensure_files_from_nonempty (#1796) ─────────────────────────────

    #[test]
    #[cfg(feature = "cli")]
    fn ensure_files_from_nonempty_rejects_empty_list() {
        let global = GlobalFlags {
            files_from: Some("list.txt".into()),
            ..GlobalFlags::default()
        };
        let err = ensure_files_from_nonempty(&global, &[]).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("empty --files-from"),
            "expected empty-list message: {msg}"
        );
        assert_eq!(
            crate::exit::classify_typed_error(&err).map(|(kind, _)| kind),
            Some("invalid_input")
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn ensure_files_from_nonempty_ok_when_paths_present() {
        let global = GlobalFlags {
            files_from: Some("list.txt".into()),
            ..GlobalFlags::default()
        };
        ensure_files_from_nonempty(&global, &[PathBuf::from("a.txt")]).unwrap();
    }

    #[test]
    #[cfg(feature = "cli")]
    fn ensure_files_from_nonempty_skips_when_unset() {
        // Directory walk with zero matching files is not invalid_input.
        let global = GlobalFlags::default();
        ensure_files_from_nonempty(&global, &[]).unwrap();
    }

    // ── is_binary ─────────────────────────────────────────────────────

    #[test]
    fn text_is_not_binary() {
        assert!(!is_binary(b"hello world\n"));
    }

    #[test]
    fn empty_is_not_binary() {
        assert!(!is_binary(b""));
    }

    #[test]
    fn nul_byte_makes_binary() {
        assert!(is_binary(b"hello\x00world"));
    }

    #[test]
    fn nul_at_8k_boundary_is_binary() {
        let mut data = vec![b'a'; 8191];
        data.push(0);
        assert!(is_binary(&data));
    }

    #[test]
    fn nul_past_8k_is_not_binary() {
        let mut data = vec![b'a'; 8192];
        data.push(0);
        assert!(!is_binary(&data));
    }

    // ── is_binary_file ────────────────────────────────────────────────

    #[test]
    fn is_binary_file_detects_nul_in_real_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("bin.dat");
        std::fs::write(&p, b"hello\x00world").unwrap();
        assert!(is_binary_file(&p));
    }

    #[test]
    fn is_binary_file_returns_false_for_text_and_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("text.txt");
        std::fs::write(&p, b"hello world\n").unwrap();
        assert!(!is_binary_file(&p));
        assert!(!is_binary_file(&dir.path().join("nope.bin"))); // open fails -> false
    }

    /// Public binary probe must not open FIFOs (blocks forever).
    #[cfg(unix)]
    #[test]
    fn is_binary_file_fifo_no_hang() {
        use std::time::Instant;
        let dir = tempfile::TempDir::new().unwrap();
        let fifo = dir.path().join("p.fifo");
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        let start = Instant::now();
        assert!(!is_binary_file(&fifo));
        assert!(
            start.elapsed().as_secs() < 2,
            "is_binary_file on FIFO took {:?}",
            start.elapsed()
        );
    }

    // ── Text I/O honesty (#1894) ──────────────────────────────────────

    #[test]
    fn classify_text_bytes_empty_is_text() {
        assert_eq!(classify_text_bytes(b""), TextBytesKind::Text(String::new()));
    }

    #[test]
    fn classify_text_bytes_utf8_text() {
        assert_eq!(
            classify_text_bytes(b"hello\n"),
            TextBytesKind::Text("hello\n".into())
        );
    }

    #[test]
    fn classify_text_bytes_binary_nul() {
        assert_eq!(
            classify_text_bytes(b"hello\x00world"),
            TextBytesKind::Binary
        );
    }

    #[test]
    fn classify_text_bytes_invalid_utf8() {
        assert_eq!(
            classify_text_bytes(b"hello \xff world"),
            TextBytesKind::InvalidUtf8
        );
    }

    #[test]
    fn load_text_strict_ok_for_text() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("t.txt");
        std::fs::write(&p, "line\n").unwrap();
        assert_eq!(load_text_strict(&p, "t.txt").unwrap(), "line\n");
    }

    #[cfg(windows)]
    #[test]
    fn load_text_strict_refuses_dot_con_device() {
        let err = load_text_strict(std::path::Path::new(r"\\.\CON"), r"\\.\CON").unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert!(err.to_string().contains("not a file name"), "msg: {err}");
    }

    #[cfg(windows)]
    #[test]
    fn try_read_text_file_skips_dot_con_device() {
        assert_eq!(
            try_read_text_file(std::path::Path::new(r"\\.\CON")),
            Err(SoftTextSkip::NotRegularFile)
        );
    }

    #[test]
    fn load_text_strict_rejects_binary() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("b.bin");
        std::fs::write(&p, b"hello\x00world").unwrap();
        let err = load_text_strict(&p, "b.bin").unwrap_err();
        assert!(crate::exit::is_binary(&err), "{err:#}");
        assert!(!crate::exit::is_invalid_input(&err), "{err:#}");
        assert!(err.to_string().contains("binary file"), "msg: {err}");
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::Binary)
        );
        assert_eq!(crate::fallback::error_kind_str(&err), Some("binary"));
        assert_eq!(std::fs::read(&p).unwrap(), b"hello\x00world");
    }

    #[test]
    fn load_text_strict_rejects_invalid_utf8() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("bad.txt");
        std::fs::write(&p, b"hello \xff world").unwrap();
        let err = load_text_strict(&p, "bad.txt").unwrap_err();
        assert!(crate::exit::is_invalid_encoding(&err), "{err:#}");
        assert!(!crate::exit::is_invalid_input(&err), "{err:#}");
        assert!(err.to_string().contains("UTF-8"), "msg: {err}");
        assert_eq!(
            crate::fallback::edit_error_kind(&err),
            Some(crate::fallback::EditErrorKind::InvalidEncoding)
        );
        assert_eq!(
            crate::fallback::error_kind_str(&err),
            Some("invalid_encoding")
        );
    }

    #[test]
    fn load_text_strict_rejects_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = load_text_strict(dir.path(), "dir").unwrap_err();
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        assert!(err.to_string().contains("not a file"), "msg: {err}");
    }

    #[test]
    #[cfg(unix)]
    fn load_text_strict_unreadable_is_invalid_input_with_os_detail() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("locked.txt");
        std::fs::write(&p, "secret\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root (common in Docker) can still read mode-000 files.
        if std::fs::read_to_string(&p).is_ok() {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }
        let err = load_text_strict(&p, "locked.txt").unwrap_err();
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644));
        assert!(crate::exit::is_invalid_input(&err), "{err:#}");
        // Display (not only {:#}) must include OS detail for agent JSON paths
        // that use e.to_string().
        let msg = err.to_string();
        assert!(msg.contains("failed to read locked.txt"), "msg: {msg}");
        assert!(
            msg.contains("Permission denied")
                || msg.contains("PermissionDenied")
                || msg.contains("os error"),
            "OS detail missing from Display: {msg}"
        );
        assert_eq!(
            msg.matches("failed to read").count(),
            1,
            "must not double-wrap: {msg}"
        );
    }

    /// Create a file symlink; return false when the OS refuses (Windows
    /// without Developer Mode / admin).
    fn try_symlink_file(target: &std::path::Path, link: &std::path::Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            match std::os::windows::fs::symlink_file(target, link) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("skip file symlink test: {e}");
                    false
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            false
        }
    }

    #[test]
    fn load_text_strict_dangling_symlink_is_invalid_input() {
        let dir = tempfile::TempDir::new().unwrap();
        let link = dir.path().join("dangling.txt");
        if !try_symlink_file(&dir.path().join("missing-target"), &link) {
            return;
        }
        let err = load_text_strict(&link, "dangling.txt").unwrap_err();
        assert!(
            crate::exit::is_invalid_input(&err),
            "dangling must be InvalidInputError, got: {err:#}"
        );
        assert!(
            !crate::exit::is_io_not_found(&err),
            "dangling must not look missing: {err:#}"
        );
        assert!(err.to_string().contains("not a file"), "msg: {err}");
    }

    #[test]
    fn load_text_strict_live_symlink_to_text_ok() {
        let dir = tempfile::TempDir::new().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, "hello via link\n").unwrap();
        let link = dir.path().join("live.txt");
        if !try_symlink_file(&target, &link) {
            return;
        }
        assert_eq!(
            load_text_strict(&link, "live.txt").unwrap(),
            "hello via link\n"
        );
    }

    #[cfg(windows)]
    #[test]
    fn load_text_strict_collapses_trailing_separators() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("keep.txt");
        std::fs::write(&file, "KEEP\n").unwrap();
        let slashed = std::path::PathBuf::from(format!("{}\\", file.display()));
        assert_eq!(load_text_strict(&slashed, r"keep.txt\\").unwrap(), "KEEP\n");
        assert_eq!(try_read_text_file(&slashed).unwrap(), "KEEP\n");
    }

    #[test]
    fn load_text_strict_missing_is_io_not_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path().join("nope.txt");
        let err = load_text_strict(&p, "nope.txt").unwrap_err();
        assert!(
            crate::exit::is_io_not_found(&err),
            "expected is_io_not_found, got: {err:#}"
        );
        assert!(
            !crate::exit::is_invalid_input(&err),
            "NotFound must not be invalid_input: {err:#}"
        );
        // Display must include OS detail (not only `{:#}`).
        let msg = err.to_string();
        assert!(msg.contains("failed to read nope.txt"), "msg: {msg}");
        assert!(
            msg.contains("No such file") || msg.contains("os error 2") || msg.contains("not found"),
            "OS detail missing from Display: {msg}"
        );
        assert_eq!(
            msg.matches("failed to read").count(),
            1,
            "must not double-wrap: {msg}"
        );
        // Agent JSON / {:#} consumers must not double the OS detail either.
        let agent = crate::exit::agent_error_message(&err);
        let os_hits = agent
            .matches("No such file")
            .count()
            .max(agent.matches("os error 2").count())
            .max(agent.matches("not found").count());
        assert_eq!(
            os_hits, 1,
            "agent_error_message must keep OS detail once: {agent}"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn all_explicit_paths_missing_detects_typos() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = vec!["nope.txt".to_string()];
        assert!(all_explicit_paths_missing(&missing, Some(dir.path())));
        assert!(!all_explicit_paths_missing(&[], Some(dir.path())));
        std::fs::write(dir.path().join("exists.txt"), b"x\n").unwrap();
        let mixed = vec!["exists.txt".to_string(), "nope.txt".to_string()];
        assert!(!all_explicit_paths_missing(&mixed, Some(dir.path())));
    }

    #[test]
    fn looks_like_glob_dest_star_and_question() {
        assert!(looks_like_glob_dest("*.txt"));
        assert!(looks_like_glob_dest("sub/*.rs"));
        assert!(looks_like_glob_dest(r"sub\*.rs"));
        assert!(looks_like_glob_dest("file?.txt"));
        assert!(looks_like_glob_dest(r"\\?\C:\temp\*.txt"));
        assert!(looks_like_glob_dest("//?/C:/temp/*.txt"));
        assert!(!looks_like_glob_dest("keep.txt"));
        assert!(!looks_like_glob_dest("sub/keep.txt"));
        assert!(!looks_like_glob_dest(r"\\?\C:\temp\keep.txt"));
        assert!(!looks_like_glob_dest("//?/C:/temp/keep.txt"));
        assert!(!looks_like_glob_dest(r"\\.\C:\temp\keep.txt"));
        assert!(!looks_like_glob_dest("//./C:/temp/keep.txt"));
    }

    #[cfg(feature = "cli")]
    fn dest_glob_set(pattern: &str) -> GlobSet {
        let mut builder = GlobSetBuilder::new();
        builder.add(compile_dest_glob(pattern).expect("valid dest glob"));
        builder.build().expect("globset")
    }

    #[cfg(feature = "cli")]
    fn dest_glob_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\pl-dest-glob")
        } else {
            PathBuf::from("/tmp/pl-dest-glob")
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn dest_glob_matcher_star_star_matches_nested_star_does_not() {
        let root = dest_glob_root();
        let nested = root.join("sub").join("a.txt");
        let roots = [root];
        let rec = dest_glob_set("**/*.txt");
        let star = dest_glob_set("*.txt");
        assert!(
            matches_dest_glob(&nested, &rec, &roots),
            "**/*.txt must match sub/a.txt"
        );
        assert!(
            !matches_dest_glob(&nested, &star, &roots),
            "*.txt must not match sub/a.txt"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn compile_dest_glob_and_matches_dest_glob_edges() {
        let root = dest_glob_root();
        let cwd = root.join("keep.txt");
        let nested = root.join("sub").join("a.txt");
        let roots = [root.clone()];

        let star = dest_glob_set("*.txt");
        assert!(matches_dest_glob(&cwd, &star, &roots));
        assert!(!matches_dest_glob(&nested, &star, &roots));

        let rec = dest_glob_set("**/*.txt");
        assert!(matches_dest_glob(&cwd, &rec, &roots));
        assert!(matches_dest_glob(&nested, &rec, &roots));

        let q = dest_glob_set("file?.txt");
        assert!(matches_dest_glob(&root.join("fileA.txt"), &q, &roots));
        assert!(!matches_dest_glob(&root.join("fileAB.txt"), &q, &roots));

        assert!(compile_dest_glob("*[").is_err());

        #[cfg(windows)]
        {
            let win_slash = dest_glob_set(r"sub\*.txt");
            assert!(matches_dest_glob(&nested, &win_slash, &roots));
            assert!(!matches_dest_glob(&cwd, &win_slash, &roots));
            assert!(matches_dest_glob(&root.join("Hit.TXT"), &star, &roots));
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn dest_glob_cwd_only_rule_and_skip_case_tip() {
        let cwd = vec!["*.txt".to_string()];
        let rec = vec!["**/*.txt".to_string()];
        let literal = vec!["keep.txt".to_string()];
        let rule = dest_glob_cwd_only_rule(&cwd).expect("cwd dest-glob rule");
        assert!(
            rule.contains("dest `*.txt` matches files in the current directory only"),
            "rule must name dest-subject cwd-only: {rule}"
        );
        assert!(
            rule.contains("--glob"),
            "rule must name --glob as nested remedy: {rule}"
        );
        assert!(dest_glob_cwd_only_rule(&rec).is_none());
        assert!(dest_glob_cwd_only_rule(&literal).is_none());
        let with_rule = with_dest_glob_cwd_only_rule("no matches for 'KEEP' in *.txt", &cwd);
        assert!(
            with_rule.contains("dest `*.txt` matches files in the current directory only")
                && with_rule.contains("--glob"),
            "appended rule must name dest-subject cwd-only and --glob nested: {with_rule}"
        );
        assert!(dest_glob_skip_case_tip(&cwd, true));
        assert!(dest_glob_skip_case_tip(&cwd, false));
        assert!(dest_glob_skip_case_tip(&rec, true));
        assert!(!dest_glob_skip_case_tip(&rec, false));
        assert!(!dest_glob_skip_case_tip(&literal, true));
        assert!(!dest_glob_skip_case_tip(&literal, false));

        let sub = vec!["sub/*.txt".to_string()];
        let sub_rule = dest_glob_cwd_only_rule(&sub).expect("subdir dest-glob rule");
        assert!(
            sub_rule.contains("dest `sub/*.txt`")
                && sub_rule.contains("that directory only")
                && !sub_rule.contains("current directory only")
                && sub_rule.contains("--glob")
                && sub_rule.contains("**/*.txt"),
            "subdir dest-glob rule must name dest-subject that-directory-only and **/*.txt: {sub_rule}"
        );
        let with_sub = with_dest_glob_cwd_only_rule("no matches for 'KEEP' in sub/*.txt", &sub);
        assert!(
            with_sub.contains("dest `sub/*.txt`")
                && with_sub.contains("that directory only")
                && !with_sub.contains("current directory only")
                && with_sub.contains("--glob")
                && with_sub.contains("**/*.txt"),
            "appended subdir rule must name dest-subject that-directory-only: {with_sub}"
        );
        let rs = dest_glob_cwd_only_rule(&["src/*.rs".to_string()]).expect("src dest-glob rule");
        assert!(
            rs.contains("**/*.rs") && rs.contains("--glob '*.rs'") && !rs.contains("*.txt"),
            "src/*.rs remedy must name **/*.rs / --glob '*.rs', not *.txt: {rs}"
        );
        assert!(!dest_glob_skip_case_tip(&["sub/*.txt".to_string()], false));
        assert!(dest_glob_skip_case_tip(&["sub/*.txt".to_string()], true));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn dest_glob_walk_max_depth_cwd_vs_recursive() {
        assert_eq!(dest_glob_walk_max_depth(&["*.txt".into()]), Some(1));
        assert_eq!(dest_glob_walk_max_depth(&["./*.txt".into()]), Some(1));
        assert_eq!(dest_glob_walk_max_depth(&[r".\*.txt".into()]), Some(1));
        assert_eq!(dest_glob_walk_max_depth(&["sub/*.txt".into()]), Some(2));
        assert_eq!(dest_glob_walk_max_depth(&["a/b/*.txt".into()]), Some(3));
        assert_eq!(
            dest_glob_walk_max_depth(&["*.txt".into(), "sub/*.txt".into()]),
            Some(2)
        );
        assert_eq!(dest_glob_walk_max_depth(&["**/*.txt".into()]), None);
        assert_eq!(
            dest_glob_walk_max_depth(&["*.txt".into(), "**/*.md".into()]),
            None
        );
        assert_eq!(dest_glob_walk_max_depth(&[]), None);
        #[cfg(windows)]
        {
            assert_eq!(dest_glob_walk_max_depth(&[r"sub\*.txt".into()]), Some(2));
            assert_eq!(dest_glob_walk_max_depth(&[r"**\*.txt".into()]), None);
        }
    }

    #[test]
    #[cfg(feature = "cli")]
    fn dest_glob_mix_literal_root_stays_recursive() {
        use std::fs;
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src/deep")).unwrap();
        fs::create_dir_all(root.join("other")).unwrap();
        fs::write(root.join("keep.txt"), "k\n").unwrap();
        fs::write(root.join("src/deep/in_src.txt"), "s\n").unwrap();
        fs::write(root.join("other/nested.txt"), "o\n").unwrap();
        let global = GlobalFlags::test_with_cwd(root);
        let paths =
            collect_file_paths_opts(&["src".into(), "*.txt".into()], &global, false, Some(root))
                .unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            rels.iter().any(|r| r == "keep.txt"),
            "cwd dest *.txt: {rels:?}"
        );
        assert!(
            rels.iter().any(|r| r.ends_with("in_src.txt")),
            "literal src must stay recursive: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.contains("other")),
            "dest *.txt must not pick other/: {rels:?}"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn all_explicit_paths_missing_ignores_glob_dests() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(
            !all_explicit_paths_missing(&["*.txt".into()], Some(dir.path())),
            "glob dests are not dest not_found"
        );
        std::fs::write(dir.path().join("keep.txt"), "KEEP\n").unwrap();
        assert!(!all_explicit_paths_missing(
            &["keep.txt".into(), "*.md".into()],
            Some(dir.path())
        ));
    }

    #[cfg(all(windows, feature = "cli"))]
    #[test]
    fn all_explicit_paths_missing_collapses_trailing_separators() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("keep.txt"), "KEEP\n").unwrap();
        let slashed = vec![r"keep.txt\\".to_string()];
        assert!(
            !all_explicit_paths_missing(&slashed, Some(dir.path())),
            "Win32 keep.txt\\ must not peel not_found"
        );
    }

    // ── matches_glob ──────────────────────────────────────────────────
    // These require the glob/walker APIs which are behind "cli" or "files" feature.

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn no_matcher_matches_everything() {
        assert!(matches_glob(Path::new("any/file.rs"), None));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn glob_matches_extension() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let matcher = builder.build().unwrap();
        assert!(matches_glob(Path::new("src/main.rs"), Some(&matcher)));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn glob_rejects_non_matching() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let matcher = builder.build().unwrap();
        assert!(!matches_glob(Path::new("src/main.py"), Some(&matcher)));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn glob_matches_nested_relative_pattern_with_root() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("sub/*.txt").unwrap());
        let matcher = builder.build().unwrap();
        let roots = vec![PathBuf::from("/tmp/project")];

        assert!(matches_glob_with_roots(
            Path::new("/tmp/project/sub/file.txt"),
            Some(&matcher),
            &roots,
        ));
        assert!(!matches_glob_with_roots(
            Path::new("/tmp/project/other.txt"),
            Some(&matcher),
            &roots,
        ));
    }

    #[test]
    #[cfg(feature = "cli")]
    fn collect_glob_roots_normalizes_current_directory_segments() {
        let global = GlobalFlags::test_default();
        let roots =
            collect_glob_roots_from_global(&[], &global, Some(Path::new("/tmp/project"))).unwrap();

        assert_eq!(roots, vec![PathBuf::from("/tmp/project")]);
    }

    // ── par_process_files ─────────────────────────────────────────────
    // These require the glob/walker APIs which are behind "cli" or "files" feature.

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn par_process_single_file() {
        let paths = vec![PathBuf::from("a.txt")];
        let results = par_process_files(&paths, None, &[], |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert_eq!(results, vec!["a.txt"]);
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn par_process_filters_with_glob() {
        let paths = vec![
            PathBuf::from("a.rs"),
            PathBuf::from("b.py"),
            PathBuf::from("c.rs"),
        ];
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let matcher = builder.build().unwrap();
        let results = par_process_files(&paths, Some(&matcher), &[], |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"a.rs".to_string()));
        assert!(results.contains(&"c.rs".to_string()));
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn par_process_filters_with_relative_glob_root() {
        let paths = vec![
            PathBuf::from("/tmp/project/sub/a.txt"),
            PathBuf::from("/tmp/project/other.txt"),
        ];
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("sub/*.txt").unwrap());
        let matcher = builder.build().unwrap();
        let roots = vec![PathBuf::from("/tmp/project")];
        let results = par_process_files(&paths, Some(&matcher), &roots, |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert_eq!(results, vec!["/tmp/project/sub/a.txt".to_string()]);
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn par_process_empty_paths() {
        let paths: Vec<PathBuf> = vec![];
        let results: Vec<String> = par_process_files(&paths, None, &[], |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert!(results.is_empty());
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn par_process_closure_can_filter() {
        let paths = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        let results = par_process_files(&paths, None, &[], |p| {
            if p.to_string_lossy().contains('a') {
                Some(1)
            } else {
                None
            }
        });
        assert_eq!(results, vec![1]);
    }

    // ── read_text_file ────────────────────────────────────────────────

    #[test]
    fn read_text_file_returns_content_for_utf8_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, "hello world\n").unwrap();
        let result = read_text_file(&file);
        assert_eq!(result.unwrap(), "hello world\n");
    }

    #[test]
    fn read_text_file_returns_none_for_binary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("binary.bin");
        std::fs::write(&file, b"hello\x00world").unwrap();
        assert!(read_text_file(&file).is_none());
        assert_eq!(try_read_text_file(&file).unwrap_err(), SoftTextSkip::Binary);
    }

    #[test]
    fn try_read_text_file_reasons_utf8_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.txt");
        std::fs::write(&bad, b"hello \xff world").unwrap();
        assert_eq!(
            try_read_text_file(&bad).unwrap_err(),
            SoftTextSkip::InvalidUtf8
        );
        assert_eq!(
            try_read_text_file(dir.path().join("missing.txt").as_path()).unwrap_err(),
            SoftTextSkip::Unreadable
        );
        assert!(SoftTextSkip::Binary.is_content_skip());
        assert!(!SoftTextSkip::Unreadable.is_content_skip());
        assert_eq!(SoftTextSkip::InvalidUtf8.as_reason(), "invalid_utf8");
    }

    /// Dangling symlink is a present special entry, not missing/unreadable.
    #[cfg(unix)]
    #[test]
    fn try_read_text_file_dangling_symlink_is_not_regular() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling.txt");
        std::os::unix::fs::symlink(dir.path().join("missing-target"), &link).unwrap();
        assert_eq!(
            try_read_text_file(&link).unwrap_err(),
            SoftTextSkip::NotRegularFile
        );
    }

    /// Windows dangling file symlink reports `FileType::is_file()`; soft
    /// read must still skip as NotRegularFile, not Unreadable.
    #[cfg(windows)]
    #[test]
    fn try_read_text_file_dangling_file_symlink_is_not_regular() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling.txt");
        if let Err(e) = std::os::windows::fs::symlink_file(dir.path().join("missing-target"), &link)
        {
            // File symlinks need Developer Mode or admin.
            eprintln!("skip dangling file symlink test: {e}");
            return;
        }
        assert_eq!(
            try_read_text_file(&link).unwrap_err(),
            SoftTextSkip::NotRegularFile
        );
    }

    /// Soft text load must not open FIFOs (blocks forever).
    #[cfg(unix)]
    #[test]
    fn try_read_text_file_fifo_no_hang() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("p.fifo");
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        let start = Instant::now();
        assert_eq!(
            try_read_text_file(&fifo).unwrap_err(),
            SoftTextSkip::NotRegularFile
        );
        assert_eq!(SoftTextSkip::NotRegularFile.as_reason(), "not_regular_file");
        assert!(
            start.elapsed().as_secs() < 2,
            "try_read_text_file on FIFO took {:?}",
            start.elapsed()
        );
    }

    /// CLI soft-read diagnostic must not re-open FIFOs for OS error text
    /// (hang residual after soft refuse; fixloop 2026-08-02).
    #[cfg(all(unix, feature = "cli"))]
    #[test]
    fn read_text_file_logged_fifo_no_hang() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("p.fifo");
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        let start = Instant::now();
        assert!(
            read_text_file_logged(&fifo, "replace", false).is_none(),
            "FIFO must soft-skip"
        );
        assert!(
            start.elapsed().as_secs() < 2,
            "read_text_file_logged on FIFO took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn read_text_file_returns_empty_string_for_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("empty.txt");
        std::fs::write(&file, b"").unwrap();
        let result = read_text_file(&file);
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn read_text_file_returns_none_for_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.txt");
        std::fs::write(&file, b"hello \xff world\n").unwrap();
        assert!(read_text_file(&file).is_none());
    }

    #[test]
    fn read_text_file_returns_none_for_missing_file() {
        assert!(read_text_file(Path::new("/tmp/patchloom_nonexistent_xyz.txt")).is_none());
    }

    #[test]
    fn read_text_file_large_file_two_phase_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("large.txt");
        // Create a text file larger than the 8 KiB binary-check probe.
        let content = "a".repeat(10_000) + "\n";
        std::fs::write(&file, &content).unwrap();
        let result = read_text_file(&file);
        assert_eq!(result.unwrap(), content);
    }

    #[test]
    fn read_text_file_large_binary_rejected_via_header_probe() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("large.bin");
        // Create a binary file larger than the 8 KiB probe with a NUL
        // in the header. The two-phase read should detect the NUL in the
        // first 8 KiB and return None without reading the rest.
        let mut data = vec![b'a'; 10_000];
        data[4096] = 0; // NUL in the first 8 KiB
        std::fs::write(&file, &data).unwrap();
        assert!(read_text_file(&file).is_none());
    }

    #[test]
    fn read_text_file_large_file_invalid_utf8_past_header() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad_tail.txt");
        // First 8 KiB is valid ASCII; byte 9000 is invalid UTF-8.
        let mut data = vec![b'a'; 10_000];
        data[9000] = 0xff;
        std::fs::write(&file, &data).unwrap();
        // The two-phase read should detect invalid UTF-8 in the second
        // phase (read_to_end) and return None.
        assert!(read_text_file(&file).is_none());
    }

    #[test]
    fn read_text_file_binary_past_8k_still_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mostly_text.txt");
        // 8 KiB of text, then a NUL byte and newline. The binary check only
        // inspects the first 8 KiB, so the file is still treated as text.
        let mut data = vec![b'a'; 8192];
        data.push(0);
        data.push(b'\n');
        std::fs::write(&file, &data).unwrap();
        let result = read_text_file(&file).expect("NUL past 8KiB should still read as text");
        assert_eq!(result.len(), 8194);
    }

    // ── collect_file_paths_opts with advanced ignores (for #821) ────────

    #[test]
    #[cfg(feature = "cli")]
    fn collect_file_paths_opts_respects_ignore_file_and_exclude() {
        use crate::cli::global::GlobalFlags;
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create tree
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn foo() {}\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("target/debug"), "binary").unwrap(); // will be excluded by pattern
        fs::write(root.join("README.md"), "# hi\n").unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\n").unwrap(); // should survive .agentignore + exclude
        fs::write(root.join(".agentignore"), "target/\n*.md\n").unwrap();

        let mut global = GlobalFlags::test_default();
        global.cwd = Some(root.to_string_lossy().into_owned());
        global.ignore_file = vec![".agentignore".to_string()];
        global.exclude = vec!["*.rs".to_string()]; // further exclude rs on top

        let paths =
            collect_file_paths_opts(&[".".to_string()], &global, false, Some(root)).unwrap();

        // .agentignore skips target/ and *.md; then exclude *.rs skips the rs files.
        // Only nothing should remain? Wait, adjust: actually with exclude *.rs and ignore md/target, expect empty or adjust expectation.
        // Simpler assertion: the ignore_file was honored (no target, no md), and additional exclude removed rs.
        let rels: Vec<_> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            rels.contains(&"Cargo.toml".to_string()),
            "surviving file missing: {:?}",
            rels
        );
        assert!(
            !rels
                .iter()
                .any(|r| r.starts_with("target") || r.ends_with(".md") || r.ends_with(".rs")),
            "advanced ignores not applied: {:?}",
            rels
        );
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn collect_file_paths_skips_patchloom_directory() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("real.txt"), "hello\n").unwrap();
        fs::create_dir_all(root.join(".patchloom/backups/12345")).unwrap();
        fs::write(root.join(".patchloom/backups/12345/manifest.json"), "{}\n").unwrap();

        let paths = collect_file_paths(root, false).unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            rels.contains(&"real.txt".to_string()),
            "real.txt should be collected: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.contains(".patchloom")),
            ".patchloom files should be excluded: {rels:?}"
        );
    }

    /// #2078: WalkBuilder max_depth must not enter pruned dirs (only top-level files).
    #[test]
    #[cfg(feature = "cli")]
    fn collect_file_paths_opts_depth_prunes_nested() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("top.txt"), "t\n").unwrap();
        fs::create_dir_all(root.join("a/b/c")).unwrap();
        fs::write(root.join("a/mid.txt"), "m\n").unwrap();
        fs::write(root.join("a/b/c/deep.txt"), "d\n").unwrap();

        let global = GlobalFlags::test_with_cwd(root);
        let shallow =
            collect_file_paths_opts_depth(&[".".into()], &global, false, Some(root), Some(1))
                .unwrap();
        let shallow_rels: Vec<_> = shallow
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            shallow_rels.iter().any(|r| r == "top.txt"),
            "depth 1 includes top: {shallow_rels:?}"
        );
        assert!(
            !shallow_rels
                .iter()
                .any(|r| r.contains("mid") || r.contains("deep")),
            "depth 1 must not enter a/: {shallow_rels:?}"
        );

        let mid = collect_file_paths_opts_depth(&[".".into()], &global, false, Some(root), Some(2))
            .unwrap();
        let mid_rels: Vec<_> = mid
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            mid_rels.iter().any(|r| r.ends_with("mid.txt")),
            "depth 2 includes a/mid: {mid_rels:?}"
        );
        assert!(
            !mid_rels.iter().any(|r| r.contains("deep")),
            "depth 2 must not reach a/b/c: {mid_rels:?}"
        );
    }

    /// #2078: max_depth is per walk root when multiple roots are collected.
    #[test]
    #[cfg(feature = "cli")]
    fn collect_file_paths_opts_depth_multi_root() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("left/deep")).unwrap();
        fs::create_dir_all(root.join("right/deep")).unwrap();
        fs::write(root.join("left/l.txt"), "l\n").unwrap();
        fs::write(root.join("left/deep/x.txt"), "x\n").unwrap();
        fs::write(root.join("right/r.txt"), "r\n").unwrap();
        fs::write(root.join("right/deep/y.txt"), "y\n").unwrap();
        let global = GlobalFlags::test_with_cwd(root);
        let paths = collect_file_paths_opts_depth(
            &["left".into(), "right".into()],
            &global,
            false,
            Some(root),
            Some(1),
        )
        .unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            rels.iter().any(|r| r.ends_with("l.txt")) && rels.iter().any(|r| r.ends_with("r.txt")),
            "top of each root: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.contains("deep")),
            "deep under each root pruned: {rels:?}"
        );
    }

    #[test]
    #[cfg(feature = "cli")]
    fn collect_file_paths_opts_skips_patchloom_directory() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("real.txt"), "hello\n").unwrap();
        fs::create_dir_all(root.join(".patchloom/backups/12345")).unwrap();
        fs::write(root.join(".patchloom/backups/12345/manifest.json"), "{}\n").unwrap();

        let global = GlobalFlags::test_with_cwd(root);
        let paths = collect_file_paths_opts(&[".".to_string()], &global, true, Some(root)).unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            rels.contains(&"real.txt".to_string()),
            "real.txt should be collected: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.contains(".patchloom")),
            ".patchloom files should be excluded even with include_hidden=true: {rels:?}"
        );
    }

    /// tidy uses include_hidden=true so .env is checked; .git must still be skipped
    /// (otherwise binary objects cause invalid-UTF-8 skip noise on stderr).
    #[test]
    #[cfg(feature = "cli")]
    fn collect_file_paths_opts_skips_git_directory_when_hidden() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("real.txt"), "hello\n").unwrap();
        fs::create_dir_all(root.join(".git/objects/ab")).unwrap();
        // Invalid UTF-8 "object" that would trip the text reader if walked.
        fs::write(root.join(".git/objects/ab/cdef"), [0xffu8, 0xfe, 0x00]).unwrap();
        fs::write(root.join(".env"), "SECRET=1\n").unwrap();

        let global = GlobalFlags::test_with_cwd(root);
        let paths = collect_file_paths_opts(&[".".to_string()], &global, true, Some(root)).unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().to_string())
            .collect();
        assert!(
            rels.iter()
                .any(|r| r == "real.txt" || r.ends_with("real.txt")),
            "real.txt should be collected: {rels:?}"
        );
        assert!(
            rels.iter().any(|r| r == ".env" || r.ends_with(".env")),
            ".env should be collected with include_hidden: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.contains(".git")),
            ".git must not be walked with include_hidden=true: {rels:?}"
        );
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn exclude_glob_matches_files_in_subdirs() {
        // Regression: exclude globs like "*.rs" must match files in
        // subdirectories via filename fallback, same as include globs.
        let mut paths = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("README.md"),
        ];
        apply_exclude_globs(&mut paths, &["*.rs".into()], None).unwrap();
        assert_eq!(
            paths,
            vec![PathBuf::from("README.md")],
            "*.rs should exclude files in subdirs"
        );
    }

    #[test]
    #[cfg(any(feature = "cli", feature = "files"))]
    fn exclude_glob_matches_directory_pattern_with_root() {
        // Regression: patterns like "vendor/**" must match absolute paths
        // when the root prefix is stripped for comparison.
        let root = PathBuf::from("/project");
        let mut paths = vec![
            PathBuf::from("/project/src/main.rs"),
            PathBuf::from("/project/vendor/lib.rs"),
            PathBuf::from("/project/vendor/sub/dep.rs"),
        ];
        apply_exclude_globs(&mut paths, &["vendor/**".into()], Some(&root)).unwrap();
        assert_eq!(
            paths,
            vec![PathBuf::from("/project/src/main.rs")],
            "vendor/** with root should exclude vendor files"
        );
    }
}

#[cfg(all(test, feature = "cli"))]
mod explicit_exclude_tests {
    use super::*;
    use crate::cli::global::GlobalFlags;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn explicit_file_arg_not_dropped_by_exclude_glob() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("vendor")).unwrap();
        let file = root.join("vendor/x.js");
        fs::write(&file, "foo\n").unwrap();
        let global = GlobalFlags {
            exclude: vec!["vendor/**".into()],
            ..GlobalFlags::test_default()
        };
        let paths = collect_file_paths_opts_with_list(
            &["vendor/x.js".into()],
            &global,
            false,
            Some(root),
            None,
            None,
        )
        .unwrap();
        assert!(
            paths.iter().any(|p| p.ends_with("x.js")),
            "explicit file must survive exclude: {paths:?}"
        );
    }

    #[test]
    fn directory_root_still_honors_exclude() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("vendor")).unwrap();
        fs::write(root.join("vendor/x.js"), "foo\n").unwrap();
        fs::write(root.join("app.js"), "foo\n").unwrap();
        let global = GlobalFlags {
            exclude: vec!["vendor/**".into()],
            ..GlobalFlags::test_default()
        };
        let paths = collect_file_paths_opts_with_list(
            &[".".into()],
            &global,
            false,
            Some(root),
            None,
            None,
        )
        .unwrap();
        assert!(
            paths.iter().any(|p| p.ends_with("app.js")),
            "app.js should remain: {paths:?}"
        );
        assert!(
            !paths.iter().any(|p| p.to_string_lossy().contains("vendor")),
            "vendor walk contents still excluded: {paths:?}"
        );
    }

    #[test]
    fn excluded_directory_tree_is_omitted() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("vendor/pkg/deep")).unwrap();
        fs::write(root.join("vendor/pkg/deep/lib.js"), "v\n").unwrap();
        fs::write(root.join("vendor/top.js"), "v\n").unwrap();
        fs::write(root.join("app.js"), "a\n").unwrap();
        let global = GlobalFlags {
            exclude: vec!["vendor/**".into()],
            ..GlobalFlags::test_default()
        };
        let paths = collect_file_paths_opts_with_list(
            &[".".into()],
            &global,
            false,
            Some(root),
            None,
            None,
        )
        .unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(rels.iter().any(|r| r == "app.js"), "kept app.js: {rels:?}");
        assert!(
            !rels.iter().any(|r| r.contains("vendor")),
            "vendor/** must omit the tree: {rels:?}"
        );
    }

    #[test]
    fn extension_exclude_still_collects_other_files_under_dir() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn x() {}\n").unwrap();
        fs::write(root.join("src/keep.md"), "ok\n").unwrap();
        let global = GlobalFlags {
            exclude: vec!["*.rs".into()],
            ..GlobalFlags::test_default()
        };
        let paths = collect_file_paths_opts_with_list(
            &[".".into()],
            &global,
            false,
            Some(root),
            None,
            None,
        )
        .unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(
            rels.iter().any(|r| r == "src/keep.md"),
            "*.rs must not prune src/: {rels:?}"
        );
        assert!(
            !rels.iter().any(|r| r.ends_with(".rs")),
            "*.rs still excludes rust files: {rels:?}"
        );
    }

    #[test]
    fn vendor_star_exclude_omits_nested_like_double_star() {
        // globset `*` matches `/` (literal_separator is off). `vendor/*` is
        // the same exclude language as `vendor/**`.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("vendor/pkg")).unwrap();
        fs::write(root.join("vendor/top.js"), "t\n").unwrap();
        fs::write(root.join("vendor/pkg/deep.js"), "d\n").unwrap();
        fs::write(root.join("app.js"), "a\n").unwrap();
        let global = GlobalFlags {
            exclude: vec!["vendor/*".into()],
            ..GlobalFlags::test_default()
        };
        let paths = collect_file_paths_opts_with_list(
            &[".".into()],
            &global,
            false,
            Some(root),
            None,
            None,
        )
        .unwrap();
        let rels: Vec<_> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert!(rels.iter().any(|r| r == "app.js"), "kept app.js: {rels:?}");
        assert!(
            !rels.iter().any(|r| r.contains("vendor")),
            "vendor/* omits the tree: {rels:?}"
        );
    }
}

#[cfg(all(test, any(feature = "cli", feature = "files")))]
mod gitignore_case_tests {
    use super::*;
    use std::fs;

    #[test]
    fn gitignore_star_log_skips_uppercase_ext_only_on_windows() {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        fs::write(dir.path().join("app.LOG"), "hit\n").unwrap();
        fs::write(dir.path().join("keep.txt"), "hit\n").unwrap();
        let paths = collect_file_paths_with_ignores(dir.path(), &[], &[], false).unwrap();
        let names: Vec<String> = paths
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        assert!(names.iter().any(|n| n == "keep.txt"), "{names:?}");
        #[cfg(windows)]
        assert!(
            !names.iter().any(|n| n.eq_ignore_ascii_case("app.LOG")),
            "gitignore *.log must drop app.LOG on Windows: {names:?}"
        );
        #[cfg(not(windows))]
        assert!(
            names.iter().any(|n| n == "app.LOG"),
            "gitignore stays case-sensitive off Windows: {names:?}"
        );
    }
}

#[cfg(all(test, any(feature = "cli", feature = "files")))]
mod user_glob_case_tests {
    use super::*;

    #[test]
    fn star_txt_matches_uppercase_ext_only_on_windows() {
        let set = build_glob_matcher(&["*.txt".into()]).unwrap().unwrap();
        #[cfg(windows)]
        assert!(
            set.is_match("Hit.TXT"),
            "*.txt must match Hit.TXT on Windows"
        );
        #[cfg(not(windows))]
        assert!(
            !set.is_match("Hit.TXT"),
            "*.txt stays case-sensitive off Windows"
        );
        assert!(set.is_match("hit.txt"));
    }
}

#[cfg(all(test, any(feature = "cli", feature = "files")))]
mod walk_prune_tests {
    use super::*;
    use std::path::Path;

    fn matcher(pat: &str) -> globset::GlobSet {
        build_glob_matcher(&[pat.into()]).unwrap().unwrap()
    }

    #[test]
    fn prune_vendor_double_star() {
        let m = matcher("vendor/**");
        let root = Path::new("/project");
        assert!(should_prune_excluded_directory(
            &root.join("vendor"),
            Some(root),
            &m
        ));
        assert!(!should_prune_excluded_directory(
            &root.join("src"),
            Some(root),
            &m
        ));
    }

    #[test]
    fn prune_vendor_star_same_as_double_star() {
        // globset `*` matches `/`; `vendor/*` covers every descendant.
        let m = matcher("vendor/*");
        let root = Path::new("/project");
        assert!(should_prune_excluded_directory(
            &root.join("vendor"),
            Some(root),
            &m
        ));
    }

    #[test]
    fn do_not_prune_extension_glob() {
        let m = matcher("*.rs");
        let root = Path::new("/project");
        assert!(!should_prune_excluded_directory(
            &root.join("src"),
            Some(root),
            &m
        ));
    }

    #[test]
    fn prune_nested_node_modules_prefix() {
        let m = matcher("**/node_modules/**");
        let root = Path::new("/project");
        assert!(should_prune_excluded_directory(
            &root.join("src/node_modules"),
            Some(root),
            &m
        ));
    }

    #[test]
    fn prune_target_double_star() {
        let m = matcher("target/**");
        let root = Path::new("/project");
        assert!(should_prune_excluded_directory(
            &root.join("target"),
            Some(root),
            &m
        ));
    }
}
