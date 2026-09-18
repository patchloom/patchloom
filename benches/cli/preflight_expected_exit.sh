#!/usr/bin/env bash
# Run a command and accept only listed exit codes. Stderr stays visible.
# Usage: preflight_expected_exit.sh 0,2 -- cmd args...
set -u
if [ "$#" -lt 3 ]; then
    echo "Usage: $0 ALLOWED_EXITS -- command [args...]" >&2
    exit 1
fi
allowed="$1"
shift
if [ "${1:-}" != "--" ]; then
    echo "ERROR: expected -- before command" >&2
    exit 1
fi
shift
if [ "$#" -eq 0 ]; then
    echo "ERROR: missing command after --" >&2
    exit 1
fi

set +e
"$@"
rc=$?
set -e

IFS=',' read -r -a ok <<< "$allowed"
for c in "${ok[@]}"; do
    if [ "$rc" -eq "$c" ]; then
        exit 0
    fi
done
echo "ERROR: command failed with unexpected exit $rc (allowed: $allowed): $*" >&2
exit 1
