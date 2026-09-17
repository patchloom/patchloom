#![no_main]

use libfuzzer_sys::fuzz_target;
use patchloom::ops::shell_token::{
    find_command_position_matches, is_command_position, replace_command_position,
};

fuzz_target!(|input: (&str, usize, usize)| {
    let (content, start, end) = input;
    // Arbitrary content plus start/end must never panic (#2489).
    let _ = is_command_position(content, start, end);
    let _ = find_command_position_matches(content, content);
    let _ = replace_command_position(content, "pip", "uv");
});
