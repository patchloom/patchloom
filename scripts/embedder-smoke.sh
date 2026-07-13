#!/usr/bin/env bash
# Pre-release embedder smoke: contracts hosts actually hit (not full CI).
# Usage: scripts/embedder-smoke.sh [path-to-patchloom-binary]
set -euo pipefail

BIN="${1:-target/debug/patchloom}"
if [[ ! -x "$BIN" ]]; then
  echo "error: binary not executable: $BIN (run make build first)" >&2
  exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "OK: $*"; }

# --- #1694: bare identifier typo must not nuke surrounding syntax ---
printf 'const CONFIGURATION_VALUE_PRIMARY: i32 = 1;\nfn use_it() -> i32 { CONFIGURATION_VALUE_PRIMARY }\n' \
  >"$tmpdir/f.rs"
"$BIN" replace CONFIGURATION_VALUE_PRIMRY \
  --new CONFIGURATION_VALUE_SECONDARY --fuzzy --apply "$tmpdir/f.rs" >/dev/null
got=$(cat "$tmpdir/f.rs")
echo "$got" | grep -q 'const CONFIGURATION_VALUE_SECONDARY: i32 = 1;' \
  || fail "fuzzy typo lost const/type syntax: $got"
echo "$got" | grep -qx 'CONFIGURATION_VALUE_SECONDARY' \
  && fail "fuzzy typo bare-line replaced: $got"
pass "fuzzy identifier typo preserves syntax (#1694)"

# --- #1695: nested monorepo undo --list sees crate-local sessions ---
mkdir -p "$tmpdir/ws/crates/foo"
printf 'old\n' >"$tmpdir/ws/crates/foo/lib.txt"
"$BIN" --cwd "$tmpdir/ws/crates/foo" replace old --new new --apply lib.txt >/dev/null
list_out=$("$BIN" --cwd "$tmpdir/ws" undo --list 2>/dev/null || true)
echo "$list_out" | grep -qE '[0-9]{10,}' \
  || fail "undo --list from workspace missed nested session: $list_out"
pass "undo --list finds nested monorepo sessions (#1695)"

# --- plan accepts key alias (registry MCP covered by unit/integration) ---
printf '{"server":{"port":8080}}\n' >"$tmpdir/ws/config.json"
cat >"$tmpdir/ws/plan.json" <<'JSON'
{"ops":[{"op":"doc.set","path":"config.json","key":"server.port","value":9090}]}
JSON
"$BIN" --cwd "$tmpdir/ws" tx plan.json --apply >/dev/null
grep -q '9090' "$tmpdir/ws/config.json" || fail "plan key alias did not apply"
pass "plan doc.set accepts key alias (#1696/#1435)"

echo "embedder-smoke: all checks passed"
