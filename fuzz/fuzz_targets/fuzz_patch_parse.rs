#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // parse_patch must never panic on any input.
    let _ = patchloom::ops::patch::parse_patch(data);
});
