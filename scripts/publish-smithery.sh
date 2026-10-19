#!/usr/bin/env bash
# Publish target/mcpb/patchloom-<ver>.mcpb to Smithery (stdio MCPB).
# Requires: SMITHERY_API_KEY, jq, curl. Prefer this over `smithery mcp publish`
# for CI: CLI 1.2.0 can return 400 "No values to set" while the REST API works.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ -z "${SMITHERY_API_KEY:-}" ]; then
  echo "SMITHERY_API_KEY is required" >&2
  exit 1
fi

version="$(
  python3 - <<'PY'
from pathlib import Path
import re
text = Path("Cargo.toml").read_text()
m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
if not m:
    raise SystemExit("could not parse version from Cargo.toml")
print(m.group(1))
PY
)"
if [ -n "${VERSION:-}" ]; then
  version="$VERSION"
fi

qualified_name="${SMITHERY_SERVER:-patchloom/patchloom}"
bundle="${ROOT}/target/mcpb/patchloom-${version}.mcpb"
if [ ! -f "$bundle" ]; then
  echo "Missing ${bundle}; run scripts/pack-mcpb.sh first" >&2
  exit 1
fi

# Payload shape that Smithery accepts for stdio MCPB uploads. Empty tool lists
# are required (omitting them can yield 400 "No values to set").
payload="$(
  jq -n \
    --arg name "patchloom" \
    --arg ver "$version" \
    --arg title "Patchloom" \
    --arg desc "Agent-safe structured edits: JSON/YAML/TOML/md/AST, dry-run, batch/tx. Not a filesystem MCP." \
    '{
      type: "stdio",
      runtime: "node",
      displayName: $title,
      description: $desc,
      serverCard: {
        serverInfo: { name: $name, version: $ver },
        tools: [],
        prompts: [],
        resources: []
      }
    }'
)"

enc_name="$(python3 -c "import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$qualified_name")"

# Ensure server exists (idempotent).
curl -fsS -X PUT "https://api.smithery.ai/servers/${enc_name}" \
  -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "{}" >/dev/null 2>&1 || true

echo "Uploading ${bundle} as ${qualified_name}..."
# Do not use curl -f here. A 403 body is the reason the upload failed
# (for example "Missing required permission: servers:write"). -f drops
# that body and the log only shows "curl: (22)".
body_file="$(mktemp)"
http_code="$(
  curl -sS -o "$body_file" -w '%{http_code}' -X PUT \
    "https://api.smithery.ai/servers/${enc_name}/releases" \
    -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
    -H "Accept: application/json" \
    -F "payload=${payload}" \
    -F "bundle=@${bundle};type=application/octet-stream"
)" || {
  echo "Smithery release upload failed before a response" >&2
  rm -f "$body_file"
  exit 1
}
if [ "$http_code" -lt 200 ] || [ "$http_code" -ge 300 ]; then
  echo "Smithery release upload failed: HTTP ${http_code}" >&2
  python3 - "$body_file" <<'PY'
import re
import sys

text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
text = re.sub(r"smry_[A-Za-z0-9+/=._-]+", "[redacted]", text)
print(text[:2000], file=sys.stderr)
PY
  rm -f "$body_file"
  exit 1
fi
resp="$(cat "$body_file")"
rm -f "$body_file"
echo "$resp" | jq .

status="$(echo "$resp" | jq -r '.status // empty')"
if [ "$status" != "SUCCESS" ] && [ "$status" != "WORKING" ] && [ "$status" != "PENDING" ]; then
  # 202 responses use status SUCCESS for stdio uploads that finish immediately.
  echo "Unexpected release status: ${status:-unknown}" >&2
  exit 1
fi

# Best-effort metadata (homepage, description, etc.).
curl -fsS -X PATCH "https://api.smithery.ai/servers/${enc_name}" \
  -H "Authorization: Bearer ${SMITHERY_API_KEY}" \
  -H "Content-Type: application/json" \
  -d "$(jq -n \
    --arg desc "Agent-safe structured edits: JSON/YAML/TOML/md/AST, dry-run, batch/tx. Not a filesystem MCP." \
    '{
      displayName: "Patchloom",
      description: $desc,
      homepage: "https://patchloom.github.io/patchloom/",
      repositoryUrl: "https://github.com/patchloom/patchloom",
      license: "MIT",
      unlisted: false
    }')" >/dev/null || true

echo "Published ${qualified_name}"
echo "Server page: https://smithery.ai/servers/${qualified_name}"
