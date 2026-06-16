#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::ops::doc::{FileFormat, parse_doc};

fuzz_target!(|data: &str| {
    // parse_doc must never panic on any input, regardless of format.
    // It should return Ok or Err, never crash.
    let _ = parse_doc(data, &FileFormat::Json);
    let _ = parse_doc(data, &FileFormat::Yaml);
    let _ = parse_doc(data, &FileFormat::Toml);
});