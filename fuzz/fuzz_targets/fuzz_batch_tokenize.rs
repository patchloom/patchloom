#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &str| {
    // tokenize must never panic on any input.
    let _ = patchloom::cmd::batch::tokenize(data);
});
