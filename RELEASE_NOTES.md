# Patchloom 0.15.2

Clearer JSON for agents and multi-op plans: doc reads wrap values you can
parse, batch and tx report whether a write landed, and tidy.fix defaults
match the CLI. Seven product PRs since 0.15.1.

## Highlights

Doc query commands under `--json` / `--jsonl` now return a small success
object with `ok`, `value`, and `path` (and `selector` when it applies). Plain
text mode is unchanged so human and jq-style pipelines still see bare values.
MCP tools peel that envelope and keep returning the bare value for existing
prompts.

Plan and batch JSON now include `applied`, the same field single-file write
commands already use. Preview and `--check` set `applied: false` with
`status: changes_detected`; a successful `--apply` sets `applied: true` with
`status: success`. You no longer need to infer apply mode only from `status`
or exit code.

`doc has` treats a missing key as a normal answer: exit 0 and `false` (JSON
`value: false`), not exit 3. Plan and batch `tidy.fix` without write-policy
fields match CLI `tidy fix` (trim trailing whitespace and ensure a final
newline). Op fields still override plan `write_policy`, including through
commit.

## Bug fixes

### JSON and CLI fields agents can use

- **Doc get / has / keys / len / select / flatten success under `--json` is an
  envelope.** Shape is `{"ok":true,"value":...,"path":...,"selector":...}`
  (no `selector` on flatten). Text output stays bare. MCP peels the envelope so
  tool results stay stable. (#1846, #1848)
- **`doc has` missing key exits 0 with `false`.** Missing is not
  `no_matches` / exit 3. (#1846)
- **Plan, batch, and `tx` JSON include `applied`.** Same meaning as other write
  commands: true after a successful apply (and post-commit lifecycle error
  reports where bytes already landed), false for preview and pure failures.
  (#1851)
- **Undo dry-run sets `applied: false`; restore sets `applied: true`.** (#1836)
- **`format_failed` after a successful write sets `applied: true`.**
  `write_applied` remains as a deprecated alias. (#1836)
- **Replace `no_matches` and `ambiguous` JSON set `applied: false`.** (#1845)
- **Replace missing-mode errors name CLI flags** (`--new`, `--insert-before`,
  `--insert-after`) and show `replace OLD --new NEW path`, not an internal
  field name. (#1836)

### tidy.fix and plan write policy

- **Bare `tidy.fix` matches CLI `tidy fix` defaults** (trim trailing
  whitespace + ensure final newline; `normalize_eol` stays keep unless set).
  (#1846)
- **Plan `write_policy` is applied at stage** with precedence defaults → plan
  fields → op fields. (#1848)
- **Op-level tidy fields are not undone at commit** by re-applying plan
  `write_policy`. CLI and EditorConfig policy still apply so
  `tidy fix --respect-editorconfig` keeps working. (#1849)

### Docs, sandboxes, and embedders

- **`--contain` is relative to effective `--cwd`.** Hosts that sandbox agents
  should pin `--cwd` to the project and not forward model-chosen `--cwd` or
  `--contain`. (#1845)
- **`init --json` / `--jsonl` writes `AGENTS.md` without `--yes`.** No more
  silent `agent_rules: skipped` with `ok: true`. (#1845)
- **Canonical names table:** CLI replace uses positional `OLD` (not `--old`);
  AST rename/replace still use `--old` / `--new`. (#1845)
- **`md upsert-bullet --content` aliases `--bullet`.** (#1846)
- **AST agent-rules examples match real clap** (path-first replace/read;
  symbol-first refs/impact; no `--symbol` / `--name` flags). (#1846)
- **Plan-level `for_each: { "glob": "..." }` example** in agent-rules and
  docs. (#1846)
- **`merge_match_modes` is public** for Rust embedders that roll up match
  honesty. (#1846)
- **Schema and agent-rules document tidy.fix defaults and precedence.**
  (#1850)
- **`explain` lists explicit tidy.fix write-policy fields** when set. (#1849)

## Numbers

| Metric | 0.15.1 | 0.15.2 |
|--------|--------|--------|
| Tests (rounded) | 3600+ | 3700+ |
| Tests (exact list) | (3600+ badge) | 3709 |
| Commands | 23 | 23 |
| Product PRs since 0.15.1 |  | 7 |

## Upgrading

```bash
# crates.io
cargo install patchloom --locked

# or upgrade an existing install
cargo install patchloom --locked --force
```

### For agent and script authors

1. **Doc query JSON:** under `--json` / `--jsonl`, read `value` (and check
   `ok`), not the whole object as the document fragment. Text mode is still
   bare. MCP tool results remain bare values.
2. **Plan and batch:** use `applied` the same way as single-file writes.
   Prefer `applied: true` (or `status: success` with `applied: true`) over
   `ok: true` alone when deciding that an edit landed.
3. **`doc has`:** exit 0 with `false` means the key is absent; do not treat
   that as a hard miss that needs retry with a different command.
4. **Bare `tidy.fix`:** expect trim + final newline unless you set op or plan
   write-policy fields. To keep trailing spaces, set
   `"trim_trailing_whitespace": false` on the op (or plan policy) explicitly.

Library embedders that construct `TxOutput` by hand should use Patchloom
constructors or update struct literals for the new `applied` field
(`#[serde(default)]` accepts older JSON without it).
