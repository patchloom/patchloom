#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // selector::parse must never panic on any input.
    let _ = patchloom::selector::parse(data);
});
