#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: (&str, &str, Option<&str>, Option<&str>)| {
    let (content, target, before, after) = data;
    // resolve_with_fallback must never panic on arbitrary inputs.
    let _ = patchloom::fallback::resolve_with_fallback(content, target, before, after, false);
});
