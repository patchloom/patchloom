#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::ops::replace::compile_replace_regex;

fuzz_target!(|data: &str| {
    // Regex compilation must never panic on any pattern string.
    // It may return Err for invalid patterns, but must not crash.
    let _ = compile_replace_regex(data, true, false, false, false);
    let _ = compile_replace_regex(data, true, true, false, false);
    let _ = compile_replace_regex(data, true, false, true, false);
    let _ = compile_replace_regex(data, false, true, false, true);
    let _ = compile_replace_regex(data, true, true, true, true);
});