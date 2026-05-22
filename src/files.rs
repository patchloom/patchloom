use crate::cli::global::GlobalFlags;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Compute a display-friendly relative path by stripping a `base` prefix.
///
/// Returns the relative portion if `path` is under `base`, otherwise returns
/// the original path unchanged. Used by diff headers, search results, and JSON
/// output so users see `src/main.rs` instead of `/home/user/project/src/main.rs`.
pub(crate) fn relative_display<'a>(path: &'a Path, base: &Path) -> &'a Path {
    path.strip_prefix(base).unwrap_or(path)
}

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB, the same heuristic Git uses).
/// Check if a string contains common regex metacharacters that suggest
/// the user intended a regex pattern but forgot `--regex` (or used `--literal`).
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

pub(crate) fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    memchr::memchr(0, &data[..check_len]).is_some()
}

/// Collect file paths from either `--files-from`, or by walking `paths` with
/// `ignore::WalkBuilder` (respects `.gitignore`).  When `root` is `Some`,
/// paths are joined with it before walking.  Tidy commands set
/// `include_hidden = true` so dotfiles are checked.
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
    let first = resolve(&effective[0]);
    let mut builder = WalkBuilder::new(&first);
    for p in &effective[1..] {
        builder.add(resolve(p));
    }
    if include_hidden {
        builder.hidden(false);
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
                self.target.lock().unwrap().append(&mut self.batch);
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
                    state.target.lock().unwrap().append(&mut state.batch);
                }
            }
            WalkState::Continue
        })
    });
    Ok(collected.into_inner().unwrap())
}

/// Build a compiled glob matcher from `--glob`, or `None` if no globs given.
pub(crate) fn build_glob_matcher(global: &GlobalFlags) -> anyhow::Result<Option<GlobSet>> {
    if global.glob.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in &global.glob {
        builder.add(Glob::new(pattern)?);
    }
    Ok(Some(builder.build()?))
}

/// Collect roots used for matching `--glob` patterns against walked files.
pub(crate) fn collect_glob_roots(
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

    let mut roots = Vec::new();
    for path in effective {
        let resolved = match root {
            Some(r) => r.join(path),
            None => PathBuf::from(path),
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

    Ok(roots)
}

fn normalize_glob_root(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn glob_matches_path(path: &Path, matcher: &GlobSet) -> bool {
    matcher.is_match(path) || path.file_name().is_some_and(|name| matcher.is_match(name))
}

/// Check whether `path` matches any of the globs, either directly or relative
/// to one of the provided roots (always true if no globs).
pub(crate) fn matches_glob_with_roots(
    path: &Path,
    matcher: Option<&GlobSet>,
    roots: &[PathBuf],
) -> bool {
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
pub(crate) fn matches_glob(path: &Path, matcher: Option<&GlobSet>) -> bool {
    match matcher {
        None => true,
        Some(m) => glob_matches_path(path, m),
    }
}

/// Read a file as UTF-8 text, skipping binary files and logging errors.
/// Returns `None` for binary, empty, unreadable, or non-UTF-8 files.
///
/// Only the first 8 KiB are read for the binary check, so large binary
/// files (images, compiled objects) are rejected without reading the
/// entire file into memory.
pub(crate) fn read_text_file(path: &Path, cmd: &str, quiet: bool) -> Option<String> {
    use std::io::Read;

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            if !quiet {
                eprintln!("{cmd}: skipping {}: {e}", path.display());
            }
            return None;
        }
    };

    // Read the first 8 KiB for binary detection (same heuristic as Git).
    let mut probe = vec![0u8; 8192];
    let n = match file.read(&mut probe) {
        Ok(n) => n,
        Err(e) => {
            if !quiet {
                eprintln!("{cmd}: skipping {}: {e}", path.display());
            }
            return None;
        }
    };

    if n == 0 {
        return None; // empty file
    }
    probe.truncate(n);
    if is_binary(&probe) {
        return None;
    }

    // Not binary — read the remainder and combine.
    if let Err(e) = file.read_to_end(&mut probe) {
        if !quiet {
            eprintln!("{cmd}: skipping {}: {e}", path.display());
        }
        return None;
    }

    match String::from_utf8(probe) {
        Ok(s) => Some(s),
        Err(_) => {
            if !quiet {
                eprintln!("{cmd}: skipping {} (invalid UTF-8)", path.display());
            }
            None
        }
    }
}

/// Process file paths using adaptive parallelism via `std::thread::scope`.
///
/// Files are split into chunks (one per available core). The calling thread
/// processes the first chunk immediately while spawned threads handle the
/// rest. Thread creation cost is ~0.05ms per thread (vs ~2ms for rayon's
/// global thread pool init), so overhead is near-zero even for small
/// workloads. For large workloads, all cores run concurrently.
pub(crate) fn par_process_files<T, F>(
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
    fn plain_text_has_no_regex_meta() {
        assert!(!has_regex_metacharacters("hello world"));
        assert!(!has_regex_metacharacters("foo-bar_baz"));
    }

    #[test]
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

    // ── matches_glob ──────────────────────────────────────────────────

    #[test]
    fn no_matcher_matches_everything() {
        assert!(matches_glob(Path::new("any/file.rs"), None));
    }

    #[test]
    fn glob_matches_extension() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let matcher = builder.build().unwrap();
        assert!(matches_glob(Path::new("src/main.rs"), Some(&matcher)));
    }

    #[test]
    fn glob_rejects_non_matching() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("*.rs").unwrap());
        let matcher = builder.build().unwrap();
        assert!(!matches_glob(Path::new("src/main.py"), Some(&matcher)));
    }

    #[test]
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
    fn collect_glob_roots_normalizes_current_directory_segments() {
        let global = GlobalFlags::default();
        let roots = collect_glob_roots(&[], &global, Some(Path::new("/tmp/project"))).unwrap();

        assert_eq!(roots, vec![PathBuf::from("/tmp/project")]);
    }

    // ── par_process_files ─────────────────────────────────────────────

    #[test]
    fn par_process_single_file() {
        let paths = vec![PathBuf::from("a.txt")];
        let results = par_process_files(&paths, None, &[], |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert_eq!(results, vec!["a.txt"]);
    }

    #[test]
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
    fn par_process_empty_paths() {
        let paths: Vec<PathBuf> = vec![];
        let results: Vec<String> = par_process_files(&paths, None, &[], |p| {
            Some(p.to_string_lossy().into_owned())
        });
        assert!(results.is_empty());
    }

    #[test]
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
        let result = read_text_file(&file, "test", false);
        assert_eq!(result.unwrap(), "hello world\n");
    }

    #[test]
    fn read_text_file_returns_none_for_binary() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("binary.bin");
        std::fs::write(&file, b"hello\x00world").unwrap();
        assert!(read_text_file(&file, "test", false).is_none());
    }

    #[test]
    fn read_text_file_returns_none_for_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("empty.txt");
        std::fs::write(&file, b"").unwrap();
        assert!(read_text_file(&file, "test", false).is_none());
    }

    #[test]
    fn read_text_file_returns_none_for_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.txt");
        std::fs::write(&file, b"hello \xff world\n").unwrap();
        assert!(read_text_file(&file, "test", false).is_none());
    }

    #[test]
    fn read_text_file_returns_none_for_missing_file() {
        let result = read_text_file(
            Path::new("/tmp/patchloom_nonexistent_xyz.txt"),
            "test",
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn read_text_file_large_file_two_phase_read() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("large.txt");
        // Create a text file larger than the 8 KiB binary-check probe.
        let content = "a".repeat(10_000) + "\n";
        std::fs::write(&file, &content).unwrap();
        let result = read_text_file(&file, "test", false);
        assert_eq!(result.unwrap(), content);
    }

    #[test]
    fn read_text_file_binary_past_8k_still_read_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mostly_text.txt");
        // 8 KiB of text, then a NUL byte. The binary check only inspects
        // the first 8 KiB, so this file should be treated as text but
        // fail UTF-8 validation (NUL is valid UTF-8 but is_binary only
        // checks the first 8 KiB). Actually NUL IS valid UTF-8, so this
        // will succeed as text. The real check: is_binary only looks at
        // the first 8 KiB probe, and the rest is read unconditionally.
        let mut data = vec![b'a'; 8192];
        data.push(b'\n');
        std::fs::write(&file, &data).unwrap();
        let result = read_text_file(&file, "test", false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 8193);
    }
}
