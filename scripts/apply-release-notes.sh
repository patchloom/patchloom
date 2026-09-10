#!/usr/bin/env bash
# Apply curated notes to an existing GitHub Release, then delete the
# notes branch. Notes are not on main.
#
# Sources, first match:
#   1. NOTES_FILE (tests)
#   2. RELEASE_NOTES.md on branch release-note-<semver>
#      (tag patchloom-v0.6.0 or v0.6.0 -> release-note-0.6.0)
#   3. Actions vars RELEASE_NOTES + RELEASE_NOTES_TAG (tag must match)
#   4. RELEASE_NOTES.md in the current checkout (legacy main-file path)
#
# Missing source is a no-op (auto changelog stays). After a successful
# apply from the notes branch, that branch is deleted. Variables are
# left in place; the tag pin stops them applying to a later cut.
set -euo pipefail

TAG="${TAG:-}"
REPO="${GH_REPO:-${GITHUB_REPOSITORY:-}}"
DRY_RUN="${DRY_RUN:-0}"
DELETE_BRANCH="${DELETE_BRANCH:-1}"
NOTES_FILE="${NOTES_FILE:-}"
RELEASE_NOTES="${RELEASE_NOTES:-}"
RELEASE_NOTES_TAG="${RELEASE_NOTES_TAG:-}"

semver=""
if [[ "${TAG}" =~ ^patchloom-v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  semver="${TAG#patchloom-v}"
elif [[ "${TAG}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  semver="${TAG#v}"
else
  echo "TAG must look like patchloom-vX.Y.Z or vX.Y.Z: ${TAG}" >&2
  exit 1
fi

if [ -z "${REPO}" ]; then
  echo "GH_REPO or GITHUB_REPOSITORY required" >&2
  exit 1
fi

branch="${NOTES_BRANCH:-release-note-${semver}}"
loaded_from_branch=0
source_label=""

tmp="$(mktemp)"
cleanup() { rm -f "${tmp}"; }
trap cleanup EXIT

tag_matches_pin() {
  local pin="$1"
  if [ -z "${pin}" ]; then
    return 1
  fi
  if [ "${pin}" = "${TAG}" ] || [ "${pin}" = "${semver}" ] \
    || [ "${pin}" = "v${semver}" ] || [ "${pin}" = "patchloom-v${semver}" ]; then
    return 0
  fi
  return 1
}

if [ -n "${NOTES_FILE}" ] && [ -f "${NOTES_FILE}" ]; then
  echo "PLAN: file ${NOTES_FILE}"
  cp "${NOTES_FILE}" "${tmp}"
  source_label="file:${NOTES_FILE}"
elif [ -n "${GH_TOKEN:-}" ]; then
  echo "PLAN: fetch RELEASE_NOTES.md from ${branch}"
  if gh api "repos/${REPO}/contents/RELEASE_NOTES.md?ref=${branch}" \
    -H "Accept: application/vnd.github.raw" >"${tmp}"; then
    loaded_from_branch=1
    source_label="branch:${branch}"
  else
    : >"${tmp}"
    echo "PLAN: no notes branch ${branch}"
  fi
fi

if [ ! -s "${tmp}" ] && [ -n "${RELEASE_NOTES}" ] \
  && tag_matches_pin "${RELEASE_NOTES_TAG}"; then
  echo "PLAN: Actions variable RELEASE_NOTES (pin ${RELEASE_NOTES_TAG})"
  printf '%s\n' "${RELEASE_NOTES}" >"${tmp}"
  source_label="variable"
fi

if [ ! -s "${tmp}" ] && [ -f RELEASE_NOTES.md ]; then
  echo "PLAN: legacy RELEASE_NOTES.md in checkout"
  cp RELEASE_NOTES.md "${tmp}"
  source_label="file:RELEASE_NOTES.md"
fi

if [ ! -s "${tmp}" ]; then
  echo "OK: no curated notes for ${TAG}; leaving auto notes"
  exit 0
fi

if [ "${DRY_RUN}" = "1" ]; then
  echo "DRY_RUN: would apply ${source_label} to ${TAG}"
  echo "BYTES: $(wc -c <"${tmp}" | tr -d ' ')"
  if [ "${loaded_from_branch}" = "1" ] && [ "${DELETE_BRANCH}" = "1" ]; then
    echo "DRY_RUN: would delete branch ${branch}"
  fi
  exit 0
fi

if ! gh release view "${TAG}" --repo "${REPO}" >/dev/null 2>&1; then
  echo "FAIL: release ${TAG} does not exist" >&2
  exit 1
fi

echo "DO: gh release edit ${TAG} from ${source_label}"
gh release edit "${TAG}" --repo "${REPO}" --notes-file "${tmp}"
echo "OK: applied notes to ${TAG}"

if [ "${loaded_from_branch}" = "1" ] && [ "${DELETE_BRANCH}" = "1" ]; then
  echo "DO: delete ${branch}"
  if gh api -X DELETE "repos/${REPO}/git/refs/heads/${branch}"; then
    echo "DONE: deleted ${branch}"
  else
    echo "WARN: could not delete ${branch}; delete it by hand" >&2
  fi
fi
