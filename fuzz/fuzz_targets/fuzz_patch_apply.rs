#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use patchloom::ops::patch::{Hunk, PatchLine, apply_hunks};

#[derive(Debug, Arbitrary)]
struct FuzzInput {
    original: String,
    hunks: Vec<FuzzHunk>,
}

#[derive(Debug, Arbitrary)]
struct FuzzHunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<FuzzLine>,
}

#[derive(Debug, Arbitrary)]
enum FuzzLine {
    Context(String),
    Remove(String),
    Add(String),
}

fuzz_target!(|input: FuzzInput| {
    let hunks: Vec<Hunk> = input
        .hunks
        .into_iter()
        .map(|h| Hunk {
            old_start: h.old_start,
            old_count: h.old_count,
            new_start: h.new_start,
            new_count: h.new_count,
            lines: h
                .lines
                .into_iter()
                .map(|l| match l {
                    FuzzLine::Context(s) => PatchLine::Context(s),
                    FuzzLine::Remove(s) => PatchLine::Remove(s),
                    FuzzLine::Add(s) => PatchLine::Add(s),
                })
                .collect(),
        })
        .collect();

    // apply_hunks must never panic, only return Ok or Err.
    let _ = apply_hunks(&input.original, &hunks);
});
