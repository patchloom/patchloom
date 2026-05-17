#!/bin/bash
# CLI benchmark runner: patchloom vs native tools using hyperfine.
# Usage: bash run.sh [small|medium|large]
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

# Generate corpora if missing
if [ ! -d "$CORPUS_DIR" ]; then
    echo "Generating benchmark corpora..."
    python3 "$SCRIPT_DIR/generate_corpus.py"
fi

SCALES="${@:-small medium}"
mkdir -p "$RESULTS_DIR"

for SCALE in $SCALES; do
    DIR="$CORPUS_DIR/$SCALE"
    if [ ! -d "$DIR" ]; then
        echo "Corpus $SCALE not found, generating..."
        python3 "$SCRIPT_DIR/generate_corpus.py" "$SCALE"
    fi

    echo ""
    echo "================================================================"
    echo "  Benchmarks: $SCALE corpus ($(find "$DIR" -type f | wc -l) files)"
    echo "================================================================"

    OUTFILE="$RESULTS_DIR/${SCALE}.md"
    echo "# CLI Benchmarks: $SCALE corpus" > "$OUTFILE"
    echo "" >> "$OUTFILE"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$OUTFILE"
    echo "" >> "$OUTFILE"

    # --- Search (literal) ---
    echo ""
    echo "--- Search (literal): TODO ---"
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom search" "$PATCHLOOM search TODO $DIR" \
        -n "grep -r" "grep -r TODO $DIR" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Search (regex) ---
    echo ""
    echo "--- Search (regex): def \\w+\\( ---"
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom search --regex" "$PATCHLOOM search --regex 'def \w+\(' $DIR" \
        -n "grep -rE" "grep -rE 'def \w+\(' $DIR" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Search (count) ---
    echo ""
    echo "--- Search (count) ---"
    hyperfine --warmup 2 --min-runs 10 \
        --export-markdown /dev/stdout \
        -n "patchloom search --count" "$PATCHLOOM search TODO $DIR --count" \
        -n "grep -rc" "grep -rc TODO $DIR" \
        2>/dev/null | tee -a "$OUTFILE"

    # --- Read (single file) ---
    SINGLE_FILE=$(find "$DIR" -name '*.py' -type f | head -1)
    if [ -n "$SINGLE_FILE" ]; then
        echo ""
        echo "--- Read (single file) ---"
        hyperfine --warmup 2 --min-runs 50 \
            --export-markdown /dev/stdout \
            -n "patchloom read" "$PATCHLOOM read $SINGLE_FILE" \
            -n "cat" "cat $SINGLE_FILE" \
            2>/dev/null | tee -a "$OUTFILE"
    fi

    # --- Read (multiple files) ---
    MULTI_FILES=$(find "$DIR" -name '*.py' -type f | head -5 | tr '\n' ' ')
    if [ -n "$MULTI_FILES" ]; then
        echo ""
        echo "--- Read (5 files) ---"
        hyperfine --warmup 2 --min-runs 20 \
            --export-markdown /dev/stdout \
            -n "patchloom read" "$PATCHLOOM read $MULTI_FILES" \
            -n "cat" "cat $MULTI_FILES" \
            2>/dev/null | tee -a "$OUTFILE"
    fi

    # --- Doc set (JSON) ---
    CONFIG="$DIR/config.json"
    if [ -f "$CONFIG" ]; then
        echo ""
        echo "--- Doc set (JSON key) ---"
        # Reset config before each run
        hyperfine --warmup 2 --min-runs 20 \
            --prepare "echo '{\"name\":\"bench\",\"version\":\"v1.0.0\",\"debug\":false}' > $CONFIG" \
            --export-markdown /dev/stdout \
            -n "patchloom doc set" "$PATCHLOOM doc set $CONFIG version v2.0.0 --apply" \
            -n "jq + mv" "jq '.version = \"v2.0.0\"' $CONFIG > ${CONFIG}.tmp && mv ${CONFIG}.tmp $CONFIG" \
            2>/dev/null | tee -a "$OUTFILE"
    fi

    # --- Replace (multi-file, needs cleanup between runs) ---
    echo ""
    echo "--- Replace (multi-file) ---"
    # We use --prepare to reset the corpus between runs
    hyperfine --warmup 1 --min-runs 5 \
        --prepare "cd $DIR && grep -rl 'v2.0.0' . 2>/dev/null | xargs -r sed -i 's/v2.0.0/v1.0.0/g' || true" \
        --export-markdown /dev/stdout \
        -n "patchloom replace" "$PATCHLOOM replace --from v1.0.0 --to v2.0.0 $DIR --apply" \
        -n "sed + find" "find $DIR -type f -exec sed -i 's/v1.0.0/v2.0.0/g' {} +" \
        2>/dev/null | tee -a "$OUTFILE"

    echo ""
    echo "Results saved to $OUTFILE"
done

echo ""
echo "All benchmarks complete. Results in $RESULTS_DIR/"
