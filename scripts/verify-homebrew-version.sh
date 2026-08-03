#!/usr/bin/env bash
# Verify the installed patchloom CLI version matches a release tag / app version.
#
# Used after publishing the Homebrew formula (#2134). Existing cellars on
# operator machines still need `brew upgrade`; this only proves a fresh
# install (or the current PATH binary) matches the intended version.
#
# Usage:
#   EXPECTED_VERSION=0.26.0 bash scripts/verify-homebrew-version.sh
#   EXPECTED_VERSION=0.26.0 INSTALL=1 bash scripts/verify-homebrew-version.sh
#   # Unit-test helpers (no brew):
#   bash scripts/verify-homebrew-version.sh --check-version-line "patchloom 0.26.0" 0.26.0
#   bash scripts/verify-homebrew-version.sh --normalize-version "patchloom-v0.26.0"
#
# Exit: 0 on match, 1 on hard fail, 2 on bad args / missing tools.
set -euo pipefail

normalize_version() {
  local v="$1"
  v="${v#patchloom-v}"
  v="${v#v}"
  v="${v#patchloom }"
  # Trim whitespace
  v="$(printf '%s' "$v" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  printf '%s' "$v"
}

version_line_matches() {
  local line="$1"
  local expected
  expected="$(normalize_version "$2")"
  # Accept "patchloom 0.26.0" or bare "0.26.0"
  local got
  if [[ "$line" =~ [Pp]atchloom[[:space:]]+([^[:space:]]+) ]]; then
    got="$(normalize_version "${BASH_REMATCH[1]}")"
  else
    got="$(normalize_version "$line")"
  fi
  [[ -n "$expected" && "$got" == "$expected" ]]
}

if [[ "${1:-}" == "--normalize-version" ]]; then
  normalize_version "${2:?version required}"
  printf '\n'
  exit 0
fi

if [[ "${1:-}" == "--check-version-line" ]]; then
  line="${2:?version line required}"
  expected="${3:?expected version required}"
  if version_line_matches "$line" "$expected"; then
    echo "OK: version line matches ${expected}"
    exit 0
  fi
  echo "FAIL: line=${line@Q} expected=${expected@Q}" >&2
  exit 1
fi

EXPECTED_VERSION="${EXPECTED_VERSION:-}"
if [[ -z "$EXPECTED_VERSION" && -n "${PLAN:-}" ]]; then
  EXPECTED_VERSION="$(printf '%s' "$PLAN" | jq -r '.releases[0].app_version // empty')"
fi
if [[ -z "$EXPECTED_VERSION" ]]; then
  echo "FAIL: set EXPECTED_VERSION or PLAN with releases[0].app_version" >&2
  exit 2
fi
EXPECTED_VERSION="$(normalize_version "$EXPECTED_VERSION")"

if [[ "${INSTALL:-0}" == "1" ]]; then
  export PATH="/home/linuxbrew/.linuxbrew/bin:/opt/homebrew/bin:/usr/local/bin:${PATH:-}"
  if ! command -v brew >/dev/null 2>&1; then
    echo "FAIL: brew not found (INSTALL=1 requires Homebrew)" >&2
    exit 2
  fi
  echo "PLAN: brew install patchloom/tap/patchloom (expected ${EXPECTED_VERSION})"
  # Prefer a clean install of the tap formula so we exercise the published rb.
  brew update
  brew install --force patchloom/tap/patchloom
fi

if ! command -v patchloom >/dev/null 2>&1; then
  echo "FAIL: patchloom not on PATH" >&2
  exit 2
fi

line="$(patchloom --version 2>&1 | head -n1)"
echo "DO: patchloom --version -> ${line}"
if version_line_matches "$line" "$EXPECTED_VERSION"; then
  echo "OK: patchloom --version matches ${EXPECTED_VERSION}"
  echo "NOTE: Operators with an older linked cellar still need: brew upgrade patchloom"
  exit 0
fi
echo "FAIL: expected version ${EXPECTED_VERSION}, got: ${line}" >&2
exit 1
