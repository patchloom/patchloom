# Text I/O honesty layer (#1894)

**Status:** implemented on branch `fix/text-io-honesty-layer-20260721`.

## Problem

CLI, MCP/tx, and `api::*` used different loaders. Sole-path `read_to_string`
accepted NUL (valid UTF-8) and rewrote binaries; walks used soft-skip.

## Contract

| Policy | API | Binary / invalid UTF-8 |
|--------|-----|------------------------|
| **Strict** (sole path) | `files::load_text_strict` | hard `InvalidInputError` |
| **SoftSkip** (walk / multi) | `files::read_text_file` / `tx::read_and_probe` | skip |

Byte rule: `files::classify_text_bytes` (8 KiB NUL probe + UTF-8).

## Wiring

- `tx::read_file_content` → Strict
- `tx::read_and_probe` → SoftSkip via `classify_text_bytes`
- Sole-path `api::{replace,file,doc,md,read,tidy,content_edits,ast_write,search}` → Strict
- CLI sole `cmd/read` → Strict

See issue #1894 for acceptance criteria.
