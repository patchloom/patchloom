#!/usr/bin/env bash
# Required DCO check for pull requests.
# Every non-merge, non-bot commit must have Signed-off-by.
# Co-authored-by trailers that name anthropic.com, and Claude-Session
# trailers, are rejected. Squash merge copies those trailers onto main,
# which is how the claude account became a contributor.
set -euo pipefail

if [[ -z "${BASE_SHA:-}" ]]; then
  echo "::error::BASE_SHA is required"
  exit 1
fi

if ! git rev-parse --verify "${BASE_SHA}^{commit}" >/dev/null 2>&1; then
  echo "::error::BASE_SHA ${BASE_SHA} is not a commit"
  exit 1
fi

for sha in $(git rev-list "${BASE_SHA}"..HEAD); do
  parents=$(git log -1 --format='%P' "$sha")
  if [[ "$parents" == *" "* ]]; then
    continue
  fi
  author=$(git log -1 --format='%an' "$sha")
  if [[ "$author" == *"[bot]" ]]; then
    continue
  fi
  body=$(git log -1 --format='%B' "$sha")
  if ! printf '%s\n' "$body" | grep -qi '^Signed-off-by:'; then
    echo "::error::Commit $sha by $author is missing Signed-off-by"
    exit 1
  fi
  if printf '%s\n' "$body" | grep -qiE '^[[:space:]]*Co-authored-by:.*anthropic\.com'; then
    echo "::error::Commit $sha by $author credits an Anthropic co-author. Remove the Co-authored-by trailer."
    exit 1
  fi
  if printf '%s\n' "$body" | grep -qiE '^[[:space:]]*Claude-Session:'; then
    echo "::error::Commit $sha by $author contains a Claude-Session trailer. Remove it."
    exit 1
  fi
done

echo "All commits have DCO sign-off"
