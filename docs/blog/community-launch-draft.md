# Community launch pack (draft — do not post until a release ships)

> **Status:** draft for maintainers. Post only after an explicit release tag / user-approved release PR merge. Do not publish stale version claims.

## Demo script (temp dir)

```bash
REPO=$(git rev-parse --show-toplevel)
BIN="${REPO}/target/release/patchloom"
# or: BIN=$(which patchloom)
S=$(mktemp -d /tmp/patchloom-demo-XXXXXX)
cd "$S"
printf 'port: 1\n# keep me\n' > app.yaml
printf 'name: a\n---\nname: b\n' > stream.yaml

echo "=== dry-run (expect changes, no write) ==="
"$BIN" doc set app.yaml port 5432   # exit 2 typical without --apply
cat app.yaml

echo "=== apply structured YAML ==="
"$BIN" doc set app.yaml port 5432 --apply
cat app.yaml

echo "=== multi-doc selector ==="
"$BIN" doc set stream.yaml 0.name A --apply
"$BIN" doc get stream.yaml 0.name

echo "=== fail-closed fuzzy (exact old absent) ==="
printf 'const LIVE_NAME: i32 = 1;\n' > f.rs
"$BIN" --json replace LIVE_NAAME --new X --fuzzy --apply f.rs || true
grep LIVE_NAME f.rs

echo "=== agent-rules snippet ==="
"$BIN" agent-rules --mode mcp | head -40
```

## Post draft (Show HN / r/mcp)

Title options:

- Show HN: Patchloom – structured file edits for AI agents (not another filesystem MCP)
- Patchloom: dry-run, peels, and parser-backed JSON/YAML/TOML for agent tool loops

Body (plain text; no em dashes):

```
Patchloom is a single binary (CLI + MCP + Rust library) for agent-safe file edits.

Why not generic filesystem MCP / sed / yq alone?
- Dry-run by default (preview / exit 2 when changes would apply)
- Parser-backed JSON, YAML, TOML (comments, multi-doc honesty)
- Markdown section ops and tree-sitter AST renames
- batch/tx with undo; stable error_kind for hosts (binary, already_exists, …)
- Library hosts: ReplaceOptions::for_agent and fuzzy_span_suspicious

Install: https://patchloom.github.io/patchloom/
MCP Registry: io.github.patchloom/patchloom
Repo: https://github.com/patchloom/patchloom

Would love feedback from people wiring agents to config-heavy repos.
```

## Channels (after release)

- [ ] Hacker News Show HN
- [ ] r/mcp
- [ ] Optional: r/ClaudeAI / r/ClaudeCode if rules allow tooling posts
- [ ] Update Glama listing description if manual form needed

## Related issues

- Comparison docs, README positioning, directory audit (competitive research batch)
