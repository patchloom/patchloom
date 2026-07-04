#!/usr/bin/env bash
# Example 11: AST-aware operations
#
# These commands use tree-sitter to understand code structure (20 languages).
# Unlike text-based tools, AST operations skip strings and comments and
# understand scope boundaries.
#
# Prerequisites: patchloom built with the `ast` feature (enabled by default).
# All commands below are read-only except `ast rename` and `ast replace`,
# which support --diff (default), --check, and --apply modes.

set -euo pipefail

# ---- ast list: list symbol definitions in a file --------------------------
# Shows functions, structs, enums, traits, etc. with line numbers.
patchloom ast list src/lib.rs

# Filter by kind (function, struct, enum, trait, impl, const, type, ...):
patchloom ast list src/lib.rs --kind function,struct

# Compact mode for token-efficient output (names only):
patchloom ast list src/ --compact

# ---- ast read: read a specific symbol's source code ----------------------
# Extract the full body of a symbol by name.
patchloom ast read src/lib.rs run

# With context lines before/after:
patchloom ast read src/lib.rs run --context 3

# ---- ast rename: rename identifiers (AST-aware) --------------------------
# Renames only true identifiers, skipping occurrences in strings and comments.
patchloom ast rename src/lib.rs --old OldName --new NewName --diff    # preview
patchloom ast rename src/ --old OldName --new NewName --apply          # apply across directory

# ---- ast validate: check syntax of source files --------------------------
# Reports parse errors without modifying files. Useful as a CI gate.
patchloom ast validate src/lib.rs
patchloom ast validate src/                                # validate entire directory

# ---- ast search: structural search using tree-sitter queries --------------
# Use tree-sitter S-expression queries for precise structural matching.
patchloom ast search '(function_item name: (identifier) @name)' src/

# Or use code pattern mode with meta-variables ($VAR, $$$MULTI):
patchloom ast search 'fn $NAME($$$PARAMS) -> Result<$RET>' src/ --pattern --lang rust

# ---- ast refs: find all references to a symbol ----------------------------
# Locates every usage of a symbol across files.
patchloom ast refs my_function src/
patchloom ast refs my_function src/ --include-def          # include the definition site

# ---- ast deps: extract import/dependency statements ----------------------
# Shows what a file imports.
patchloom ast deps src/lib.rs

# Show reverse dependencies (what imports this file):
patchloom ast deps src/lib.rs --reverse

# ---- ast map: generate a ranked repository map (PageRank) -----------------
# Produces a token-efficient summary of the repo's key symbols.
patchloom ast map src/
patchloom ast map src/ --max-tokens 2048 --focus src/lib.rs --boost dispatch

# ---- ast diff: structural diff between file versions ----------------------
# Shows symbol-level changes (added, removed, modified) instead of line diffs.
patchloom ast diff src/lib.rs                              # HEAD vs working tree
patchloom ast diff src/lib.rs --old main                  # branch comparison
patchloom ast diff src/lib.rs --old v0.4.0 --new v0.5.0   # tag comparison

# ---- ast impact: transitive impact analysis -------------------------------
# Shows what other symbols are affected by changing a given symbol.
patchloom ast impact my_function src/
patchloom ast impact my_function src/ --depth 2            # limit traversal depth

# ---- ast replace: scoped text replacement within a symbol -----------------
# Replaces text only inside a specific symbol's body, leaving the rest untouched.
patchloom ast replace src/config.rs default_timeout --old 30 --new 60 --diff
patchloom ast replace src/config.rs default_timeout --old 30 --new 60 --apply

# Regex mode:
patchloom ast replace src/config.rs my_function --old 'TODO.*' --new 'DONE' --regex --apply
