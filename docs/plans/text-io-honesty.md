# Text I/O honesty layer (#1894)

**Status:** implemented (see #1895–#1900; patch residual #1896).

## Problem

CLI, MCP/tx, and `api::*` used different loaders. Sole-path `read_to_string`
accepted NUL (valid UTF-8) and rewrote binaries; walks used soft-skip.

## Contract

| Policy | API | Binary / invalid UTF-8 | Unreadable |
|--------|-----|------------------------|------------|
| **Strict** (sole path) | `files::load_text_strict` | hard `InvalidInputError` | hard IO / not-found |
| **Soft content** (walk) | `try_read_text_file` / `SoftTextSkip` | content SoftSkip | `SoftTextSkip::Unreadable` |
| **Tx multi probe** | `tx::read_and_probe` | SoftSkip `Ok(false)` | hard `Err` |

Byte rule: `files::classify_text_bytes` (8 KiB NUL probe + UTF-8).

## Wiring

- `tx::read_file_content` → Strict
- `tx::read_and_probe` → content SoftSkip; unreadable hard
- Sole-path `api::*` / CLI sole paths → Strict
- Multi-path `refused[]` → `ops::file::explicit_multi_path_non_text_refused`
- Sole multi helper → `ops::file::sole_explicit_non_text`
- CLI `patch` targets + diff input → Strict via `load_patch_target` / `load_text_strict` (#1896)

See issue #1894 for acceptance criteria.
