#!/bin/bash
# Patchloom invocation-capture shim.
# Logs every call to a JSONL file, then delegates to the real binary.
# Placeholders __REAL_PATH__ and __LOG_PATH__ are replaced at runtime by conftest.py.

REAL_PATCHLOOM="__REAL_PATH__"
LOG_FILE="__LOG_PATH__"

# Build a JSON array of args using jq
args_json=$(printf '%s\n' "$@" | jq -R . | jq -sc .)

# Run the real binary, capture exit code
"$REAL_PATCHLOOM" "$@"
exit_code=$?

# Append one JSONL record with timestamp, args, and exit code
echo "{\"ts\":$(date +%s),\"args\":${args_json},\"exit_code\":${exit_code}}" >> "$LOG_FILE"

exit $exit_code
