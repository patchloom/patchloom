#!/usr/bin/env bash
# Stamp versions into a staging tree and pack to target/mcpb/patchloom-<ver>.mcpb.
# Does not dirty the committed mcpb/ tree.
#
# Version resolution (same idea as scripts/publish-smithery.sh):
#   1. $VERSION if set and non-empty (CI / workflow_dispatch of older tags)
#   2. else version field in Cargo.toml
#
# Requires: mcpb CLI and jq (unless PACK_MCPB_PRINT_VERSION_ONLY=1).
# Install: npm install -g @anthropic-ai/mcpb@2.1.2
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

version_from_cargo() {
  python3 - <<'PY'
from pathlib import Path
import re
text = Path("Cargo.toml").read_text()
m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
if not m:
    raise SystemExit("could not parse version from Cargo.toml")
print(m.group(1))
PY
}

if [ -n "${VERSION:-}" ]; then
  version="$VERSION"
else
  version="$(version_from_cargo)"
fi

if [ "${PACK_MCPB_PRINT_VERSION_ONLY:-}" = "1" ]; then
  printf '%s\n' "$version"
  exit 0
fi

if ! command -v mcpb >/dev/null 2>&1; then
  echo "mcpb CLI not found. Install: npm install -g @anthropic-ai/mcpb@2.1.2" >&2
  exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

echo "Packing patchloom MCPB version ${version}"
npm_spec="patchloom@${version}"

stage="$(mktemp -d "${TMPDIR:-/tmp}/patchloom-mcpb.XXXXXX")"
cleanup() { rm -rf "$stage"; }
trap cleanup EXIT

mkdir -p "$stage/server"
cp mcpb/server/run.mjs "$stage/server/run.mjs"
cp assets/logo-512.png "$stage/icon.png"
printf '%s\n' 'README.md' '.DS_Store' '**/.DS_Store' > "$stage/.mcpbignore"

jq --arg v "$version" --arg npm "$npm_spec" '
  .version = $v
  | .server.mcp_config.args = ["-y", $npm, "mcp-server"]
  | .server.mcp_config.platform_overrides.win32.args = ["-y", $npm, "mcp-server"]
' mcpb/manifest.json > "$stage/manifest.json"

jq --arg v "$version" '.version = $v' mcpb/package.json > "$stage/package.json"

mcpb validate "$stage/manifest.json"

out_dir="${ROOT}/target/mcpb"
mkdir -p "$out_dir"
out_file="${out_dir}/patchloom-${version}.mcpb"
rm -f "$out_file"

mcpb pack "$stage" "$out_file"
mcpb info "$out_file"
ls -lh "$out_file"
echo "PACKED=${out_file}"
