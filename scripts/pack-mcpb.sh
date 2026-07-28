#!/usr/bin/env bash
# Stamp versions into a staging tree and pack to target/mcpb/patchloom-<ver>.mcpb.
# Does not dirty the committed mcpb/ tree.
#
# Version resolution (same idea as scripts/publish-smithery.sh):
#   1. $VERSION if set and non-empty (CI / workflow_dispatch of older tags)
#   2. else version field in Cargo.toml
#
# Packing: MCPB is a zip archive with manifest.json (Anthropic mcpb format).
# Default path uses zip + lightweight JSON checks (no npm / mcpb CLI), so CI
# stays free of Scorecard Pinned-Dependencies npmCommand findings (#49/#50).
# Optional: set PACK_MCPB_USE_CLI=1 and install mcpb for official validate/pack.
#
# Requires: zip, python3 (and mcpb only if PACK_MCPB_USE_CLI=1).
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

if ! command -v zip >/dev/null 2>&1; then
  echo "zip is required to pack MCPB archives" >&2
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

python3 - "$stage" "$version" "$npm_spec" <<'PY'
import json
import pathlib
import sys

stage = pathlib.Path(sys.argv[1])
version = sys.argv[2]
npm_spec = sys.argv[3]
root = pathlib.Path("mcpb")
manifest = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
manifest["version"] = version
manifest["server"]["mcp_config"]["args"] = ["-y", npm_spec, "mcp-server"]
manifest["server"]["mcp_config"]["platform_overrides"]["win32"]["args"] = [
    "-y",
    npm_spec,
    "mcp-server",
]
(stage / "manifest.json").write_text(
    json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
)
package = json.loads((root / "package.json").read_text(encoding="utf-8"))
package["version"] = version
(stage / "package.json").write_text(
    json.dumps(package, indent=2) + "\n", encoding="utf-8"
)
required = ("manifest_version", "name", "version", "server")
missing = [k for k in required if k not in manifest]
if missing:
    raise SystemExit(f"manifest missing required keys: {missing}")
if "mcp_config" not in manifest.get("server", {}):
    raise SystemExit("manifest.server.mcp_config is required")
print(f"manifest ok: name={manifest.get('name')} version={manifest['version']}")
PY

out_dir="${ROOT}/target/mcpb"
mkdir -p "$out_dir"
out_file="${out_dir}/patchloom-${version}.mcpb"
rm -f "$out_file"

if [ "${PACK_MCPB_USE_CLI:-}" = "1" ]; then
  if ! command -v mcpb >/dev/null 2>&1; then
    echo "PACK_MCPB_USE_CLI=1 but mcpb not found" >&2
    exit 1
  fi
  mcpb validate "$stage/manifest.json"
  mcpb pack "$stage" "$out_file"
  mcpb info "$out_file"
else
  # Official format is a zip of the stage tree (see @anthropic-ai/mcpb README).
  (
    cd "$stage"
    zip -X -r "$out_file" . -x '*.DS_Store' -x '**/.DS_Store'
  )
  python3 - "$out_file" <<'PY'
import json
import sys
import zipfile

path = sys.argv[1]
with zipfile.ZipFile(path) as zf:
    names = set(zf.namelist())
    for need in ("manifest.json", "package.json", "server/run.mjs"):
        if need not in names:
            raise SystemExit(f"packed archive missing {need}: {sorted(names)[:20]}")
    manifest = json.loads(zf.read("manifest.json"))
print(
    f"mcpb archive ok: {path} version={manifest.get('version')} "
    f"entries={len(names)}"
)
PY
fi

ls -lh "$out_file"
echo "PACKED=${out_file}"
