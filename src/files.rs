use crate::cli::global::GlobalFlags;
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Returns `true` if the buffer looks like binary content (contains a NUL byte
/// in the first 8 KiB, the same heuristic Git uses).
pub(crate) fn is_binary(data: &[u8]) -> bool {
    let check_len = data.len().min(8192);
    data[..check_len].contains(&0)
}

/// Collect file paths from either `--files-from`, or by walking `paths` with
/// `ignore::WalkBuilder` (respects `.gitignore`).  When `root` is `Some`,
/// paths are joined with it before walking.  Hygiene commands set
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
    builder.build_parallel().run(|| {
        let collected = &collected;
        Box::new(move |result| {
            if let Ok(entry) = result {
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    collected.lock().unwrap().push(entry.into_path());
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

/// Check whether `path` matches any of the globs (always true if no globs).
pub(crate) fn matches_glob(path: &Path, matcher: Option<&GlobSet>) -> bool {
    match matcher {
        None => true,
        Some(m) => m.is_match(path) || path.file_name().is_some_and(|n| m.is_match(n)),
    }
}

/// Read a file as UTF-8 text, skipping binary files and logging errors.
/// Returns `None` for binary, empty, unreadable, or non-UTF-8 files.
pub(crate) fn read_text_file(path: &Path, cmd: &str, quiet: bool) -> Option<String> {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            if !quiet {
                eprintln!("{cmd}: skipping {}: {e}", path.display());
            }
            return None;
        }
    };
    if data.is_empty() || is_binary(&data) {
        return None;
    }
    match String::from_utf8(data) {
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
    f: F,
) -> Vec<T>
where
    T: Send,
    F: Fn(&Path) -> Option<T> + Sync,
{
    fn process_slice<T, F>(paths: &[PathBuf], glob_matcher: Option<&GlobSet>, f: &F) -> Vec<T>
    where
        T: Send,
        F: Fn(&Path) -> Option<T> + Sync,
    {
        paths
            .iter()
            .filter(|p| matches_glob(p, glob_matcher))
            .filter_map(|p| f(p.as_path()))
            .collect()
    }

    let num_splits = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(paths.len());

    if num_splits <= 1 {
        return process_slice(paths, glob_matcher, &f);
    }

    let chunk_size = paths.len().div_ceil(num_splits);
    let chunks: Vec<&[PathBuf]> = paths.chunks(chunk_size).collect();

    std::thread::scope(|s| {
        // Spawn threads for all chunks except the first.
        let handles: Vec<_> = chunks[1..]
            .iter()
            .map(|chunk| s.spawn(|| process_slice(chunk, glob_matcher, &f)))
            .collect();

        // Process the first chunk on the calling thread immediately.
        let mut results = process_slice(chunks[0], glob_matcher, &f);

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

    // ── par_process_files ─────────────────────────────────────────────

    #[test]
    fn par_process_single_file() {
        let paths = vec![PathBuf::from("a.txt")];
        let results = par_process_files(&paths, None, |p| Some(p.to_string_lossy().to_string()));
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
        let results = par_process_files(&paths, Some(&matcher), |p| {
            Some(p.to_string_lossy().to_string())
        });
        assert_eq!(results.len(), 2);
        assert!(results.contains(&"a.rs".to_string()));
        assert!(results.contains(&"c.rs".to_string()));
    }

    #[test]
    fn par_process_empty_paths() {
        let paths: Vec<PathBuf> = vec![];
        let results: Vec<String> =
            par_process_files(&paths, None, |p| Some(p.to_string_lossy().to_string()));
        assert!(results.is_empty());
    }

    #[test]
    fn par_process_closure_can_filter() {
        let paths = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        let results = par_process_files(&paths, None, |p| {
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
}
