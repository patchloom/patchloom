#[cfg(feature = "cli")]
use crate::cli::global::GlobalFlags;
#[cfg(any(feature = "cli", feature = "files"))]
use globset::{Glob, GlobSet, GlobSetBuilder};
#[cfg(any(feature = "cli", feature = "files"))]
use ignore::WalkBuilder;
#[cfg(feature = "cli")]
use ignore::WalkState;
use std::path::{Path, PathBuf};
#[cfg(feature = "cli")]
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

/// Returns whether the file at `path` appears to be binary by reading only its
/// first 8 KiB (streaming, no full allocation for large files). Returns false
/// on open/read errors (the subsequent content read will surface the real error).
#[cfg(test)]
pub(crate) fn is_binary_file(path: &Path) -> bool {
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

/// Collect file paths from either `--files-from`, or by walking `paths` with
/// `ignore::WalkBuilder` (respects `.gitignore`).  When `root` is `Some`,
/// paths are joined with it before walking.  Tidy commands set
/// `include_hidden = true` so dotfiles are checked.
#[cfg(feature = "cli")]
#[cfg(feature = "cli")]
pub(crate) fn collect_file_paths_opts(
    paths: &[String],
    global: &GlobalFlags,
    include_hidden: bool,
    root: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(files) = global.read_files_from()? {
        return Ok(files
            .iter()
            .map(|f| match root {
                Some(r) => r.join(f),
                None => PathBuf::from(f),
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
        match root {
            Some(r) => r.join(p),
            None => PathBuf::from(p),
        }
    };
    // Warn about nonexistent user-supplied paths so typos are visible
    // instead of silently producing an empty result set (exit 3).
    for p in effective {
        let resolved = resolve(p);
        if !resolved.exists() {
            eprintln!(
                "patchloom: {}: No such file or directory",
                resolved.display()
            );
        }
    }

    let first = resolve(&effective[0]);
    let mut builder = WalkBuilder::new(&first);
    for p in &effective[1..] {
        builder.add(resolve(p));
    }
    if include_hidden {
        builder.hidden(false);
    }
    // Support advanced layered ignores (e.g. .blineignore) for parity with library
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
            if let Ok(entry) = result
                && entry.file_type().is_some_and(|ft| ft.is_file())
            {
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

    // Apply runtime exclude patterns (shared helper for parity with with_ignores).
    apply_exclude_globs(&mut paths, &global.exclude)?;
    Ok(paths)
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
        builder.add(Glob::new(pattern)?);
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

/// Read a file as UTF-8 text, returning `None` for binary, empty,
/// unreadable, or non-UTF-8 files.
///
/// For files larger than 8 KiB, only the first 8 KiB are read initially
/// for the binary check. If the file is binary, no further I/O occurs,
/// avoiding a full read of large binary files (images, compiled objects)
/// that pass through the directory walker.
pub fn read_text_file(path: &Path) -> Option<String> {
    read_text_file_inner(path, None)
}

/// Internal version with optional diagnostic logging for CLI commands.
#[cfg(feature = "cli")]
pub(crate) fn read_text_file_logged(path: &Path, cmd: &str, quiet: bool) -> Option<String> {
    if quiet {
        read_text_file_inner(path, None)
    } else {
        read_text_file_inner(path, Some(cmd))
    }
}

fn read_text_file_inner(path: &Path, log_label: Option<&str>) -> Option<String> {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            if let Some(label) = log_label {
                eprintln!("{label}: skipping {}: {e}", path.display());
            }
            return None;
        }
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len() as usize,
        Err(_) => return None,
    };
    if file_len == 0 {
        return Some(String::new());
    }

    // For files larger than the binary-check window, read just the header
    // first. This avoids allocating megabytes for large binary files that
    // the walker did not filter out.
    const BINARY_CHECK_LEN: usize = 8192;
    if file_len > BINARY_CHECK_LEN {
        let mut header = [0u8; BINARY_CHECK_LEN];
        let n = match file.read(&mut header) {
            Ok(n) => n,
            Err(e) => {
                if let Some(label) = log_label {
                    eprintln!("{label}: skipping {}: {e}", path.display());
                }
                return None;
            }
        };
        if is_binary(&header[..n]) {
            return None;
        }
        // Header is text; now read the remainder into a single allocation.
        let mut bytes = Vec::with_capacity(file_len);
        bytes.extend_from_slice(&header[..n]);
        if let Err(e) = file.read_to_end(&mut bytes) {
            if let Some(label) = log_label {
                eprintln!("{label}: skipping {}: {e}", path.display());
            }
            return None;
        }
        return match String::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(_) => {
                if let Some(label) = log_label {
                    eprintln!("{label}: skipping {} (invalid UTF-8)", path.display());
                }
                None
            }
        };
    }

    // Small file: read all at once (single syscall).
    let mut bytes = Vec::with_capacity(file_len);
    if let Err(e) = file.read_to_end(&mut bytes) {
        if let Some(label) = log_label {
            eprintln!("{label}: skipping {}: {e}", path.display());
        }
        return None;
    }

    if is_binary(&bytes) {
        return None;
    }

    match String::from_utf8(bytes) {
        Ok(s) => Some(s),
        Err(_) => {
            if let Some(label) = log_label {
                eprintln!("{label}: skipping {} (invalid UTF-8)", path.display());
            }
            None
        }
    }
}

/// Simple file collection for library use (sequential for simplicity; full parallel in par_process_files).
#[cfg(any(feature = "cli", feature = "files"))]
pub fn collect_file_paths(root: &Path, include_hidden: bool) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths = vec![];
    let mut builder = WalkBuilder::new(root);
    if include_hidden {
        builder.hidden(false);
    }
    for entry in builder.build().filter_map(Result::ok) {
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            paths.push(entry.into_path());
        }
    }
    Ok(paths)
}

/// Apply exclude glob patterns to a list of paths (post-filter).
/// Shared to avoid duplication between collect_file_paths_with_ignores and
/// the advanced logic in collect_file_paths_opts.
#[cfg(any(feature = "cli", feature = "files"))]
fn apply_exclude_globs(paths: &mut Vec<PathBuf>, patterns: &[String]) -> anyhow::Result<()> {
    if patterns.is_empty() {
        return Ok(());
    }
    let mut exb = globset::GlobSetBuilder::new();
    for pat in patterns {
        exb.add(globset::Glob::new(pat)?);
    }
    let ex = exb.build()?;
    paths.retain(|p| !ex.is_match(p));
    Ok(())
}

/// Collect files while respecting .gitignore + custom ignore files (e.g. .blineignore)
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
    if include_hidden {
        builder.hidden(false);
    }
    for name in custom_ignore_filenames {
        builder.add_custom_ignore_filename(name);
    }
    let mut paths: Vec<PathBuf> = builder
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .map(|e| e.into_path())
        .collect();

    apply_exclude_globs(&mut paths, exclude_patterns)?;
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
        fs::write(root.join("Cargo.toml"), "[package]\n").unwrap(); // should survive .blineignore + exclude
        fs::write(root.join(".blineignore"), "target/\n*.md\n").unwrap();

        let mut global = GlobalFlags::test_default();
        global.cwd = Some(root.to_string_lossy().into_owned());
        global.ignore_file = vec![".blineignore".to_string()];
        global.exclude = vec!["*.rs".to_string()]; // further exclude rs on top

        let paths =
            collect_file_paths_opts(&[".".to_string()], &global, false, Some(root)).unwrap();

        // .blineignore skips target/ and *.md; then exclude *.rs skips the rs files.
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
}
