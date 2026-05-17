#!/bin/bash
# Patchloom invocation-capture shim.
# Logs every call to a JSONL file, then delegates to the real binary.
# Placeholders __REAL_PATH__ and __LOG_PATH__ are replaced at runtime by conftest.py.

REAL_PATCHLOOM="__REAL_PATH__"
LOG_FILE="__LOG_PATH__"

# Build a JSON array of args using jq
args_json=$(printf '%s\n' "$@" | jq -R . | jq -sc .)

# Capture start time (nanoseconds for duration calculation)
start_ns=$(date +%s%N 2>/dev/null || echo 0)

# Run the real binary, capture exit code
"$REAL_PATCHLOOM" "$@"
exit_code=$?

# Capture end time and calculate duration
end_ns=$(date +%s%N 2>/dev/null || echo 0)
if [ "$start_ns" != "0" ] && [ "$end_ns" != "0" ]; then
    duration_ms=$(( (end_ns - start_ns) / 1000000 ))
else
    duration_ms=-1
fi

# Append one JSONL record with timestamp, args, exit code, and duration
echo "{\"ts\":$(date +%s),\"args\":${args_json},\"exit_code\":${exit_code},\"duration_ms\":${duration_ms}}" >> "$LOG_FILE"

exit $exit_code
