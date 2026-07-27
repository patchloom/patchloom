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
# Fail-closed fuzzy requires allow_absent_old when old is a typo of the live
# identifier (#1736 / default deny). Opt-in restores host-facing fuzzy apply.
"$BIN" replace CONFIGURATION_VALUE_PRIMRY \
  --new CONFIGURATION_VALUE_SECONDARY --fuzzy --allow-absent-old --apply \
  "$tmpdir/f.rs" >/dev/null
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

# --- #1935: --contain path escape JSON error_kind is guard_rejected ---
printf 'x\n' >"$tmpdir/ws/in.txt"
contain_json=$("$BIN" --json --cwd "$tmpdir/ws" --contain read /etc/passwd 2>/dev/null || true)
echo "$contain_json" | grep -q '"error_kind": "guard_rejected"' \
  || echo "$contain_json" | grep -q '"error_kind":"guard_rejected"' \
  || fail "contain escape must set error_kind guard_rejected: $contain_json"
pass "CLI --contain JSON error_kind is guard_rejected (#1935)"

# --- #1947: create without force peels already_exists (not invalid_input) ---
printf 'existing\n' >"$tmpdir/ws/taken.txt"
create_json=$("$BIN" --json --cwd "$tmpdir/ws" create taken.txt --content 'new' 2>/dev/null || true)
echo "$create_json" | grep -q '"error_kind": "already_exists"' \
  || echo "$create_json" | grep -q '"error_kind":"already_exists"' \
  || fail "create dest-exists must set error_kind already_exists: $create_json"
echo "$create_json" | grep -q '"applied": false\|"applied":false' \
  || fail "create dest-exists must set applied:false: $create_json"
# Must not claim invalid_input for dest-exists (hosts branch on kind for force).
echo "$create_json" | grep -q '"error_kind": "invalid_input"' \
  && fail "create dest-exists must not be invalid_input: $create_json"
echo "$create_json" | grep -q '"error_kind":"invalid_input"' \
  && fail "create dest-exists must not be invalid_input: $create_json"
pass "CLI create dest-exists JSON error_kind is already_exists (#1947)"

# --- rename dest-exists peels already_exists (sibling of create) ---
printf 'src\n' >"$tmpdir/ws/rename-src.txt"
printf 'dst\n' >"$tmpdir/ws/rename-dst.txt"
rename_json=$("$BIN" --json --cwd "$tmpdir/ws" rename rename-src.txt rename-dst.txt 2>/dev/null || true)
echo "$rename_json" | grep -q '"error_kind": "already_exists"' \
  || echo "$rename_json" | grep -q '"error_kind":"already_exists"' \
  || fail "rename dest-exists must set error_kind already_exists: $rename_json"
echo "$rename_json" | grep -q '"error_kind": "invalid_input"' \
  && fail "rename dest-exists must not be invalid_input: $rename_json"
echo "$rename_json" | grep -q '"error_kind":"invalid_input"' \
  && fail "rename dest-exists must not be invalid_input: $rename_json"
pass "CLI rename dest-exists JSON error_kind is already_exists"

# --- delete missing peels not_found (host recovery distinct from operation_failed) ---
delete_json=$("$BIN" --json --cwd "$tmpdir/ws" delete no-such-file.txt 2>/dev/null || true)
echo "$delete_json" | grep -q '"error_kind": "not_found"' \
  || echo "$delete_json" | grep -q '"error_kind":"not_found"' \
  || fail "delete missing must set error_kind not_found: $delete_json"
echo "$delete_json" | grep -q '"error_kind": "operation_failed"' \
  && fail "delete missing must not be operation_failed: $delete_json"
echo "$delete_json" | grep -q '"error_kind":"operation_failed"' \
  && fail "delete missing must not be operation_failed: $delete_json"
pass "CLI delete missing JSON error_kind is not_found"

# --- #1963 / #1972: sole binary peels binary (not no_matches / operation_failed) ---
printf 'x\0y' >"$tmpdir/ws/blob.md"
printf 'x\0y' >"$tmpdir/ws/blob.txt"
bin_json=$("$BIN" --json --cwd "$tmpdir/ws" read blob.txt 2>/dev/null || true)
echo "$bin_json" | grep -q '"error_kind": "binary"' \
  || echo "$bin_json" | grep -q '"error_kind":"binary"' \
  || fail "read sole binary must set error_kind binary: $bin_json"
echo "$bin_json" | grep -q '"error_kind": "no_matches"' \
  && fail "read sole binary must not be no_matches: $bin_json"
echo "$bin_json" | grep -q '"error_kind":"no_matches"' \
  && fail "read sole binary must not be no_matches: $bin_json"
md_bin=$("$BIN" --json --cwd "$tmpdir/ws" md dedupe-headings blob.md 2>/dev/null || true)
echo "$md_bin" | grep -q '"error_kind": "binary"' \
  || echo "$md_bin" | grep -q '"error_kind":"binary"' \
  || fail "md sole binary must set error_kind binary: $md_bin"
pass "CLI sole binary JSON error_kind is binary (#1963/#1972)"

# --- #1963: sole invalid UTF-8 peels invalid_encoding ---
printf 'hello \xff world' >"$tmpdir/ws/bad.txt"
enc_json=$("$BIN" --json --cwd "$tmpdir/ws" read bad.txt 2>/dev/null || true)
echo "$enc_json" | grep -q '"error_kind": "invalid_encoding"' \
  || echo "$enc_json" | grep -q '"error_kind":"invalid_encoding"' \
  || fail "read invalid UTF-8 must set invalid_encoding: $enc_json"
echo "$enc_json" | grep -q '"error_kind": "binary"' \
  && fail "invalid UTF-8 must not be binary: $enc_json"
echo "$enc_json" | grep -q '"error_kind":"binary"' \
  && fail "invalid UTF-8 must not be binary: $enc_json"
pass "CLI sole invalid UTF-8 JSON error_kind is invalid_encoding (#1963)"

# --- nested undo still works after contain smoke (sessions already created) ---
# Library-only find_backup_roots is unit-tested; CLI hosts use undo --list (#1695).

# --- #1981: library host refuse helper (CLI cannot exercise this surface) ---
# Derive repo root from this script so `make embedder-smoke` stays one gate.
# Use cargo name filters (substring), not --exact (needs full module path).
# Fail if a filter runs 0 tests (cargo exits 0 on empty filters).
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ ! -f "$REPO_ROOT/Cargo.toml" ]]; then
  fail "could not locate Cargo.toml for library host tests (REPO_ROOT=$REPO_ROOT)"
fi
run_lib_filter() {
  local filter="$1"
  local out
  out=$(cd "$REPO_ROOT" && cargo test --lib --all-features "$filter" -- --test-threads=2 2>&1) || {
    echo "$out" >&2
    fail "library test filter '$filter' failed (#1981)"
  }
  if ! echo "$out" | grep -qE 'test result: ok\. [1-9][0-9]* passed'; then
    echo "$out" >&2
    fail "library test filter '$filter' ran 0 tests (#1981)"
  fi
}
run_lib_filter fuzzy_span_suspicious_default_policy
run_lib_filter replace_in_content_fuzzy_host_refuses
run_lib_filter replace_in_content_fuzzy_near_collision_reports_matched_text
pass "library fuzzy_span_suspicious host contracts (#1981)"

echo "embedder-smoke: all checks passed"
