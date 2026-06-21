#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::containment::{AbsolutePathPolicy, PathGuard};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Use a real tempdir because PathGuard performs canonicalization.
        if let Ok(tmp) = tempfile::tempdir() {
            if let Ok(guard) = PathGuard::new(tmp.path().to_path_buf(), AbsolutePathPolicy::Reject) {
                let _ = guard.check_path(s);
            }
        }
    }
});
