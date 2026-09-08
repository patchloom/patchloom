# Patchloom 0.33.0

Windows dests, globs, and line endings now follow the same rules as
Linux. Library hosts can set charset on writes.

## Highlights

A dest like `*.txt` matches files in the current directory only.
`sub/*.txt` stays in that directory. Use `**/*.txt` or `--glob '*.txt'`
when you want nested files. On Windows the CLI expands those dest
patterns itself (cmd.exe does not).

`WritePolicyOptions.charset` and EditorConfig `charset = utf-8-bom`
write a leading U+FEFF. `charset = utf-8` does not insert one.
`utf-16le` is `invalid_input`. `api::tidy` applies the same charset
when it is not Keep.

A leading UTF-8 BOM is outside regex `^`. A lone CR is a line end
for search, replace `$`, markdown headings, AST lines, and
`read --lines`.

## Breaking changes

`WritePolicy` and `WritePolicyOptions` are `#[non_exhaustive]` and
have a new `charset` field (`CharsetMode`, default `Keep`). Construct
with `Default` and `..`:

```rust
let opts = WritePolicyOptions {
    charset: CharsetMode::Utf8Bom,
    ..WritePolicyOptions::default()
};
```

`CharsetMode` is re-exported from `patchloom::api` and the crate
root ([#2372](https://github.com/patchloom/patchloom/pull/2372)).

## New features

- **Dest globs on search, replace, and tidy.** `*.txt` is the current
  directory only. `sub/*.txt` is that directory only (not cwd, not
  nested). `**/*.txt` is recursive. `--glob '*.txt'` still walks
  nested files. A miss names dest vs `--glob` and skips the `-i` tip
  when the dest glob was empty or cwd-only. Plan, tx, and MCP `path`
  is one file or directory: do not put `*.txt` there. Use replace
  `"glob"` or plan-level `for_each.glob` ([#2368](https://github.com/patchloom/patchloom/pull/2368), [#2370](https://github.com/patchloom/patchloom/pull/2370), [#2371](https://github.com/patchloom/patchloom/pull/2371)).

- **Charset on library writes.** Set `WritePolicyOptions.charset` to
  `Utf8Bom` or `Utf8`. `api::tidy` honors a non-Keep charset locally
  (plan `TidyFix` has no charset field). Indent then charset, so a
  BOM stays on the first byte. Setting both `dedent` and `indent` is
  `invalid_input` ([#2372](https://github.com/patchloom/patchloom/pull/2372), [#2373](https://github.com/patchloom/patchloom/pull/2373)).

- **EditorConfig `charset`.** With `--respect-editorconfig`,
  `charset = utf-8-bom` writes U+FEFF and `charset = utf-8` does not
  insert one. `utf-16le` / `utf-16be` are `invalid_input` ([#2358](https://github.com/patchloom/patchloom/pull/2358)).

- **Protobuf AST 0.6.** `ast list` / `ast rename` on `.proto` files
  use tree-sitter-proto 0.6 ([#2374](https://github.com/patchloom/patchloom/pull/2374)).

## Bug fixes

- **Windows dests ignored `--cwd` or deleted the file.** `C:foo` and
  `\foo` dests are `invalid_input` (they ignore `--cwd`). A trailing
  slash on a dest no longer treats the file as missing or rolls back
  a create. Drive-relative `--files-from` lines peel the same way.
  Bare `NUL` is `invalid_input`. Illegal dests (`<>"|?*`, `\\.`,
  trailing space or dot) peel before open ([#2360](https://github.com/patchloom/patchloom/pull/2360), [#2362](https://github.com/patchloom/patchloom/pull/2362), [#2364](https://github.com/patchloom/patchloom/pull/2364), [#2366](https://github.com/patchloom/patchloom/pull/2366), [#2354](https://github.com/patchloom/patchloom/pull/2354), [#2307](https://github.com/patchloom/patchloom/pull/2307)).

- **`--glob *.txt` missed `Hit.TXT` on Windows.** User globs and
  `.gitignore` match case-insensitively on Windows ([#2309](https://github.com/patchloom/patchloom/pull/2309), [#2310](https://github.com/patchloom/patchloom/pull/2310)).

- **Writes dropped hardlinks, MOTW, and named streams.** A Windows
  hardlink pair stays linked after Apply. `Zone.Identifier` and
  custom NTFS streams persist through atomic rename. Destinations
  past MAX_PATH persist. An 8.3 dest keeps the long name ([#2303](https://github.com/patchloom/patchloom/pull/2303), [#2312](https://github.com/patchloom/patchloom/pull/2312), [#2356](https://github.com/patchloom/patchloom/pull/2356), [#2313](https://github.com/patchloom/patchloom/pull/2313), [#2306](https://github.com/patchloom/patchloom/pull/2306)).

- **`--contain` rejected local UNC and `//?/` dests.** Local
  admin-share UNC (`\\localhost\C$\...`) maps into the workspace.
  IPv6 loopback UNC is local, not an ADS path. `//?/C:/` dests
  inside the workspace apply, and backup opens them with a
  drive-letter path. `\\.\C:\` dests that name a real file apply ([#2314](https://github.com/patchloom/patchloom/pull/2314), [#2315](https://github.com/patchloom/patchloom/pull/2315), [#2321](https://github.com/patchloom/patchloom/pull/2321), [#2319](https://github.com/patchloom/patchloom/pull/2319), [#2323](https://github.com/patchloom/patchloom/pull/2323)).

- **Notepad UTF-8 BOM and CR-only files missed matches.** JSON,
  markdown, prepend, and tx plans accept a leading BOM. Multi-doc
  YAML strips the BOM before parse. Regex `^` skips a leading BOM.
  A lone CR is a line end for search, markdown, TOML, AST lines,
  `read --lines`, and replace `$` (including mid-file CR). Indent
  and dedent peel a leading BOM so the first line is not treated
  as indented text ([#2311](https://github.com/patchloom/patchloom/pull/2311), [#2316](https://github.com/patchloom/patchloom/pull/2316), [#2317](https://github.com/patchloom/patchloom/pull/2317), [#2330](https://github.com/patchloom/patchloom/pull/2330), [#2331](https://github.com/patchloom/patchloom/pull/2331), [#2335](https://github.com/patchloom/patchloom/pull/2335), [#2336](https://github.com/patchloom/patchloom/pull/2336), [#2337](https://github.com/patchloom/patchloom/pull/2337), [#2343](https://github.com/patchloom/patchloom/pull/2343), [#2347](https://github.com/patchloom/patchloom/pull/2347), [#2326](https://github.com/patchloom/patchloom/pull/2326), [#2348](https://github.com/patchloom/patchloom/pull/2348), [#2374](https://github.com/patchloom/patchloom/pull/2374)).

- **Patch dests with `a\` stayed `not_found`.** Git headers
  `a\file` and `b\file` strip like `a/` / `b/`. Mixed grammar
  (unified headers plus SEARCH/REPLACE) is `invalid_input` ([#2349](https://github.com/patchloom/patchloom/pull/2349), [#2351](https://github.com/patchloom/patchloom/pull/2351)).

- **Quoted YAML `C:\Users` was a unicode escape.** A drive-letter
  `:\\` path in a quoted YAML plan is `invalid_input` and names
  `/` or `\\\\` as the fix ([#2357](https://github.com/patchloom/patchloom/pull/2357)).

- **UTF-16 `--files-from` looked like a missing list.** The list
  peels as `invalid_input` ([#2355](https://github.com/patchloom/patchloom/pull/2355)).

- **Apply reported success after a fail-restore.** A share lock or
  other persist failure is `rollback` with `applied: false`.
  Junctions unlink as junctions. ADS dests refuse ([#2304](https://github.com/patchloom/patchloom/pull/2304), [#2308](https://github.com/patchloom/patchloom/pull/2308), [#2305](https://github.com/patchloom/patchloom/pull/2305)).

- **Empty-hunk delete dests were not peeled.** A missing dest is
  `not_found`. A directory dest is `invalid_input`. A dest outside
  `--contain` is `guard_rejected` before exists ([#2299](https://github.com/patchloom/patchloom/pull/2299), [#2301](https://github.com/patchloom/patchloom/pull/2301)).

- **Undo after a case-only rename kept the new casing.** Restore
  puts the original NTFS casing back ([#2346](https://github.com/patchloom/patchloom/pull/2346)).

## Numbers

| Metric | Notes |
|--------|--------|
| Version | 0.32.0 -> 0.33.0 |
| Focus | Dest globs, charset, Windows dests, BOM and CR line ends |
| Tests | 5000+ (3376 unit + 1617 integration + 10 PTY) |

## Upgrading

- **Agents:** dest `*.txt` is cwd-only. Dest `sub/*.txt` is that
  directory only. Use dest `**/*.txt` or `--glob '*.txt'` for
  nested files. Plan/tx/MCP `path` is not a dest glob. `C:foo`
  and `\foo` dests are `invalid_input`.
- **Library hosts:** construct `WritePolicy` / `WritePolicyOptions`
  with `Default` and `..`. Set `charset` when you want a BOM or
  want one stripped. `api::tidy` is the high-level writer that
  honors charset today. `WritePolicy` now has `charset`; both
  structs are `#[non_exhaustive]`.
- **EditorConfig:** `charset = utf-8-bom` writes U+FEFF.
  `utf-16le` / `utf-16be` fail closed.
- **Windows:** `--glob` and `.gitignore` are case-insensitive.
  Hardlink siblings, MOTW, and named streams survive Apply.
- Install from crates.io, npm, Homebrew, or Scoop after the tag
  ships. On Windows, Scoop is recommended; winget may need
  `winget source update` after Microsoft publishes; Chocolatey can
  lag while moderation runs.

## Full changelog

https://github.com/patchloom/patchloom/compare/patchloom-v0.32.0...patchloom-v0.33.0
