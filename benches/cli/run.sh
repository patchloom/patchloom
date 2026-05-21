#!/bin/bash
# CLI benchmark runner: patchloom vs native tools using hyperfine.
# Usage: bash run.sh [small|medium|large]
#
# Benchmarks are grouped into two categories:
#   1. Single-operation: patchloom vs equivalent native tool (grep, cat, jq, sed)
#   2. Batched operations: patchloom batch/tx (1 call) vs N separate native calls
#
# Category 2 is where patchloom's value shows: one process invocation doing N
# edits across different file types costs less than spawning N separate tools.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CORPUS_DIR="$SCRIPT_DIR/corpus"
RESULTS_DIR="$SCRIPT_DIR/results"
PATCHLOOM="$REPO_ROOT/target/release/patchloom"

# Build release binary for fair comparison
echo "Building patchloom (release)..."
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml" --quiet

if [ ! -f "$PATCHLOOM" ]; then
    echo "ERROR: release binary not found at $PATCHLOOM"
    exit 1
fi

# Regenerate corpora (always, to pick up schema changes)
echo "Generating benchmark corpora..."
rm -rf "$CORPUS_DIR"
python3 "$SCRIPT_DIR/generate_corpus.py"

SCALES="${@:-small medium}"
mkdir -p "$RESULTS_DIR"

for SCALE in $SCALES; do
    DIR="$CORPUS_DIR/$SCALE"
    if [ ! -d "$DIR" ]; then
        echo "Corpus $SCALE not found, skipping."
        continue
    fi

    FILE_COUNT=$(find "$DIR" -type f | wc -l)
    echo ""
    echo "================================================================"
    echo "  Benchmarks: $SCALE corpus ($FILE_COUNT files)"
    echo "================================================================"

    OUTFILE="$RESULTS_DIR/${SCALE}.md"
    cat > "$OUTFILE" <<EOF
# CLI Benchmarks: $SCALE corpus

Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)

## Single-operation benchmarks

These compare one patchloom command against the equivalent native tool.
patchloom is expected to lose on search/read (grep and cat are highly
optimized C programs) and win on structured editing (doc set, tidy).

EOF

    # ── Single-operation benchmarks ──────────────────────────────────

    # --- Search (literal) ---
    echo ""
    echo "--- Search (literal): TODO ---"
    echo "### Search (literal)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom search" "$PATCHLOOM search TODO $DIR" \
        -n "grep -r" "grep -r TODO $DIR" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Search (regex) ---
    echo ""
    echo "--- Search (regex): def \\w+\\( ---"
    echo "" >> "$OUTFILE"
    echo "### Search (regex)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom search --regex" "$PATCHLOOM search --regex 'def \w+\(' $DIR" \
        -n "grep -rE" "grep -rE 'def \w+\(' $DIR" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Doc set (JSON) ---
    CONFIG_JSON="$DIR/config.json"
    echo ""
    echo "--- Doc set (JSON key) ---"
    echo "" >> "$OUTFILE"
    echo "### Doc set (JSON)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    hyperfine --warmup 2 --min-runs 20 \
        --prepare "echo '{\"name\":\"bench\",\"version\":\"v1.0.0\",\"debug\":false}' > $CONFIG_JSON" \
        --export-markdown /dev/stdout \
        -n "patchloom doc set" "$PATCHLOOM doc set $CONFIG_JSON version v2.0.0 --apply" \
        -n "jq + mv" "jq '.version = \"v2.0.0\"' $CONFIG_JSON > ${CONFIG_JSON}.tmp && mv ${CONFIG_JSON}.tmp $CONFIG_JSON" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Doc set (YAML with comments) ---
    CONFIG_YAML="$DIR/config.yaml"
    echo ""
    echo "--- Doc set (YAML with comments) ---"
    echo "" >> "$OUTFILE"
    echo "### Doc set (YAML, comment-preserving)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    YAML_RESET="printf '# Application configuration\napp:\n  name: bench  # project name\n  version: v1.0.0  # current release\n  debug: false\n' > $CONFIG_YAML"
    hyperfine --warmup 2 --min-runs 20 \
        --prepare "$YAML_RESET" \
        --export-markdown /dev/stdout \
        -n "patchloom doc set" "$PATCHLOOM doc set $CONFIG_YAML app.version v2.0.0 --apply" \
        -n "yq eval" "yq eval '.app.version = \"v2.0.0\"' -i $CONFIG_YAML" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Replace (multi-file) ---
    echo ""
    echo "--- Replace (multi-file) ---"
    echo "" >> "$OUTFILE"
    echo "### Replace (multi-file)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    hyperfine --warmup 1 --min-runs 5 \
        --prepare "cd $DIR && grep -rl 'v2.0.0' . 2>/dev/null | xargs -r sed -i 's/v2.0.0/v1.0.0/g' || true" \
        --export-markdown /dev/stdout \
        -n "patchloom replace" "$PATCHLOOM replace v1.0.0 --to v2.0.0 $DIR --apply" \
        -n "find + sed" "find $DIR -type f -exec sed -i 's/v1.0.0/v2.0.0/g' {} +" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Tidy check ---
    echo ""
    echo "--- Tidy check ---"
    echo "" >> "$OUTFILE"
    echo "### Tidy check" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    # Create some files with tidy issues for the check
    TIDY_DIR="$DIR/_tidy_test"
    mkdir -p "$TIDY_DIR"
    for i in $(seq 1 20); do
        printf "line one   \nline two\nno final newline" > "$TIDY_DIR/dirty_$i.txt"
    done
    # Both commands return non-zero when issues found; wrap to exit 0
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom tidy check" "bash -c '$PATCHLOOM tidy check $TIDY_DIR >/dev/null; true'" \
        -n "shell (find+grep)" "bash -c 'find $TIDY_DIR -type f -name \"*.txt\" | while read f; do test -n \"\$(tail -c 1 \"\$f\")\" && echo missing; grep -cP \"\\s+\$\" \"\$f\" || true; done >/dev/null; true'" \
        2>/dev/null | tee -a "$OUTFILE"
    rm -rf "$TIDY_DIR"

    # ── Batched-operation benchmarks ─────────────────────────────────

    cat >> "$OUTFILE" <<'EOF'

## Batched-operation benchmarks

These compare patchloom batch/tx (1 process invocation) against running
N separate native commands. This is where patchloom's value shows for
AI agents: one tool call instead of N round-trips.

EOF

    echo ""
    echo "--- Batch: 6-file version bump (1 call vs 6 native commands) ---"
    echo "### Batch: 6-file version bump" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    echo "One patchloom batch call edits 6 files (JSON, YAML, TOML, text, markdown)" >> "$OUTFILE"
    echo "vs 6 separate native tool invocations (jq, yq, sed, etc.)." >> "$OUTFILE"
    echo "" >> "$OUTFILE"

    # Write a reset script for hyperfine --prepare to use
    RESET_SCRIPT="$DIR/_reset.sh"
    cat > "$RESET_SCRIPT" <<RESETEOF
#!/bin/bash
DIR="$DIR"
echo '{"name":"bench","version":"v1.0.0","debug":false}' > "\$DIR/config.json"
printf '# Application configuration\napp:\n  name: bench  # project name\n  version: '\''v1.0.0'\''  # current release\n  debug: false\n' > "\$DIR/config.yaml"
printf '# Application configuration\n[app]\nname = "bench"  # project name\nversion = "v1.0.0"  # current release\ndebug = false\n' > "\$DIR/config.toml"
echo '{"name":"bench","version":"v1.0.0","main":"index.js"}' > "\$DIR/package.json"
echo "v1.0.0" > "\$DIR/VERSION"
printf '# Bench Project\n\n## Commands\n\n| Command | Description |\n|---------|-------------|\n| build | Build the project |\n| test | Run tests |\n\n## Changelog\n\n- v1.0.0 initial release\n' > "\$DIR/README.md"
RESETEOF
    chmod +x "$RESET_SCRIPT"

    BATCH_INPUT="$DIR/_batch_input.txt"
    cat > "$BATCH_INPUT" <<'BATCHEOF'
doc.set config.json version "v2.0.0"
doc.set config.yaml app.version "v2.0.0"
doc.set config.toml app.version "v2.0.0"
doc.set package.json version "v2.0.0"
replace VERSION "v1.0.0" "v2.0.0"
replace README.md "v1.0.0" "v2.0.0"
BATCHEOF

    hyperfine --warmup 1 --min-runs 10 \
        --prepare "bash $RESET_SCRIPT" \
        --export-markdown /dev/stdout \
        -n "patchloom batch (1 call)" "$PATCHLOOM --cwd $DIR batch $BATCH_INPUT --apply" \
        -n "jq+yq+sed (6 calls)" "bash -c 'jq \".version = \\\"v2.0.0\\\"\" $DIR/config.json > $DIR/config.json.tmp && mv $DIR/config.json.tmp $DIR/config.json && yq eval \".app.version = \\\"v2.0.0\\\"\" -i $DIR/config.yaml && sed -i \"s/v1.0.0/v2.0.0/\" $DIR/config.toml && jq \".version = \\\"v2.0.0\\\"\" $DIR/package.json > $DIR/package.json.tmp && mv $DIR/package.json.tmp $DIR/package.json && sed -i \"s/v1.0.0/v2.0.0/\" $DIR/VERSION && sed -i \"s/v1.0.0/v2.0.0/\" $DIR/README.md'" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- tx: multi-file atomic with format/validate ---
    echo ""
    echo "--- tx: atomic 4-file edit with validation ---"
    echo "" >> "$OUTFILE"
    echo "### tx: atomic 4-file edit" >> "$OUTFILE"
    echo "" >> "$OUTFILE"
    echo "One patchloom tx call atomically edits 4 files with a validation step" >> "$OUTFILE"
    echo "vs 4 separate native commands plus a manual check." >> "$OUTFILE"
    echo "" >> "$OUTFILE"

    TX_PLAN="$DIR/_tx_plan.json"
    cat > "$TX_PLAN" <<TXEOF
{
  "version": "1",
  "operations": [
    {"op": "doc.set", "path": "config.json", "selector": "version", "value": "v2.0.0"},
    {"op": "doc.set", "path": "config.yaml", "selector": "app.version", "value": "v2.0.0"},
    {"op": "replace", "path": "VERSION", "from": "v1.0.0", "to": "v2.0.0"},
    {"op": "md.upsert_bullet", "path": "README.md", "heading": "Changelog", "bullet": "- v2.0.0 release"}
  ]
}
TXEOF

    hyperfine --warmup 1 --min-runs 10 \
        --prepare "bash $RESET_SCRIPT" \
        --export-markdown /dev/stdout \
        -n "patchloom tx (1 call)" "$PATCHLOOM --cwd $DIR tx $TX_PLAN --apply" \
        -n "jq+yq+sed (4 calls)" "bash -c 'jq \".version = \\\"v2.0.0\\\"\" $DIR/config.json > $DIR/config.json.tmp && mv $DIR/config.json.tmp $DIR/config.json && yq eval \".app.version = \\\"v2.0.0\\\"\" -i $DIR/config.yaml && sed -i \"s/v1.0.0/v2.0.0/\" $DIR/VERSION && sed -i \"/## Changelog/a\\\\- v2.0.0 release\" $DIR/README.md'" \
        2>/dev/null | tee -a "$OUTFILE"

    rm -f "$BATCH_INPUT" "$TX_PLAN" "$RESET_SCRIPT"

    echo ""
    echo "Results saved to $OUTFILE"
done

echo ""
echo "All benchmarks complete. Results in $RESULTS_DIR/"
