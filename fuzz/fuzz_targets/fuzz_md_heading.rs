#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::ops::md::parse_headings;

fuzz_target!(|data: &str| {
    // Markdown heading parser must never panic on any input,
    // including deeply nested headings, fenced code blocks,
    // and malformed markdown.
    let _ = parse_headings(data);
});