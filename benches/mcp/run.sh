#!/bin/bash
# MCP benchmark runner: measures MCP JSON-RPC per-call latency vs CLI process spawn.
# Usage: bash run.sh [ITERATIONS]
#
# Starts a patchloom MCP server once, sends tool calls over JSON-RPC, and
# compares per-call latency against spawning a new CLI process for each
# operation. Shows the amortized startup advantage of MCP for AI agents.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"
CORPUS_DIR="$SCRIPT_DIR/../cli/corpus"

# Build release binary with MCP support
echo "Building patchloom (release, all features)..."
cargo build --release --all-features --manifest-path "$REPO_ROOT/Cargo.toml" --quiet

# Generate corpus if needed (reuse CLI benchmark corpus)
if [ ! -d "$CORPUS_DIR/small" ]; then
    echo "Generating benchmark corpus..."
    python3 "$SCRIPT_DIR/../cli/generate_corpus.py" small
fi

mkdir -p "$RESULTS_DIR"

ITERS="${1:-50}"
echo ""
echo "Running MCP benchmarks (${ITERS} iterations per operation)..."
echo ""

# Run the Rust benchmark binary
# --test-threads=1 ensures sequential execution (no interleaving output)
cargo test --test bench_mcp --all-features --release -- \
    --nocapture --test-threads=1 \
    2>&1 | tee "$RESULTS_DIR/latest.log"

# Extract just the markdown table from the output
sed -n '/^# MCP Benchmark/,$ p' "$RESULTS_DIR/latest.log" > "$RESULTS_DIR/latest.md" 2>/dev/null || true

echo ""
echo "Results saved to $RESULTS_DIR/"