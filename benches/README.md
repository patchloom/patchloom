# Benchmarks

Patchloom ships two benchmark suites: **CLI benchmarks** (patchloom vs native
tools) and **agent benchmarks** (AI agent task completion with and without
patchloom).

## Quick start

```bash
# CLI benchmarks (requires hyperfine, jq, yq)
make bench-cli

# Agent benchmarks (requires grok CLI + API key)
make bench-agent MODEL=sxs-claude-opus-4-6

# Agent benchmarks with variance reduction (3 runs per mode)
make bench-agent MODEL=sxs-claude-opus-4-6 RUNS=3

# Preview agent benchmark prompts without calling any API
make bench-agent-dry-run

# Generate a comparison report from saved results
make bench-agent-report
```

## CLI benchmarks

**Location:** `benches/cli/`

Compares patchloom CLI commands against equivalent native tools (grep, sed,
jq, yq) using [hyperfine](https://github.com/sharkdp/hyperfine) for
statistical rigor.

### What is measured

| Category | What it tests | Why it matters |
|----------|--------------|----------------|
| Single-operation | One patchloom command vs one native tool | Baseline latency comparison |
| Batched-operation | One `patchloom batch` call vs N separate native calls | Agent tool-call reduction |

### Benchmark list

**Single-operation:**
- Search (literal): `patchloom search` vs `grep -r`
- Search (regex): `patchloom search --regex` vs `grep -rE`
- Doc set (JSON): `patchloom doc set` vs `jq + mv`
- Doc set (YAML): `patchloom doc set` (comment-preserving) vs `yq eval`
- Replace (multi-file): `patchloom replace` vs `find + sed`
- Tidy check: `patchloom tidy check` vs shell script (find + grep)

**Batched-operation:**
- 6-file version bump: `patchloom batch` (1 call) vs jq + yq + sed (6 calls)
- Atomic 4-file edit: `patchloom tx` (1 call) vs jq + yq + sed (4 calls)

### Corpus sizes

| Scale | Files | Lines per file | Total lines |
|-------|------:|---------------:|------------:|
| small | 50 | 100 | ~5,000 |
| medium | 500 | 100 | ~50,000 |
| large | 5,000 | 100 | ~500,000 |

Corpora are generated deterministically by `generate_corpus.py` from a fixed
seed of code-like content (Python, Rust, JS, Go, Markdown, plain text).

### Prerequisites

- `hyperfine` (>= 1.18)
- `jq`, `yq` (for native-tool comparison)
- patchloom release binary (built automatically by `make bench-cli`)

### Running

```bash
# Default: small + medium
make bench-cli

# Specific scales
cd benches/cli && bash run.sh small medium large
```

Results are written to `benches/cli/results/<scale>.md` as hyperfine markdown
tables.

### Methodology

- Each benchmark runs with 2 warmup iterations (except multi-file replace: 1).
- Minimum 5-20 timed runs depending on the benchmark.
- The `--prepare` flag resets file state between runs to avoid cumulative drift.
- hyperfine uses wall-clock time including process startup.
- patchloom is built in release mode (`--release`) for fair comparison.

### Interpreting results

patchloom is **expected to lose** on pure search (grep is a highly optimized
C program doing one thing). The value proposition is:

1. **Structured editing** (doc set, tidy): patchloom wins because native tools
   either lose comments (jq) or require complex pipelines.
2. **Batched operations**: patchloom wins dramatically (6-7x) because one
   process invocation replaces N tool spawns. For AI agents, this means one
   tool call instead of N round-trips.

---

## Agent benchmarks

**Location:** `tests/agent/test_bench.py`

Measures how an AI agent performs 11 real tasks in three modes:

| Mode | Description |
|------|-------------|
| `patchloom` | Workspace has `AGENTS.md` with patchloom CLI instructions |
| `mcp` | Workspace has patchloom configured as an MCP server |
| `native` | No AGENTS.md, agent uses only its built-in tools |

### Task list

| # | Task | What it tests |
|---|------|--------------|
| 1 | search | Find TODO comments across source files |
| 2 | replace | Rename a function across multiple files |
| 3 | doc_set | Set a JSON key value |
| 4 | md_table | Append a row to a markdown table |
| 5 | tx_multi_file | Create a file + update JSON atomically |
| 6 | batch_6_files | Update version across 6 files (JSON, YAML, TOML, txt, md) |
| 7 | batch_mixed_ops | Atomic: doc set + replace + md bullet insert |
| 8 | yaml_comment_preserve | Set a YAML key while preserving comments |
| 9 | md_insert | Insert text after a heading without replacing content |
| 10 | file_ops | Create a file + rename another file |
| 11 | tidy | Detect and fix whitespace/newline issues |

### How it works

1. A fresh git-initialized workspace is created per mode.
2. For `patchloom` mode, `AGENTS.md` is generated from `patchloom agent-rules --mode cli --platform linux`.
3. For `mcp` mode, a `.grok/config.toml` configures patchloom as an MCP server.
4. Tasks run sequentially in a single agent session (session ID is reused).
5. A shim script wraps patchloom to log every invocation (args, duration).
6. Each task has a `check` function that verifies the workspace state.

### Prerequisites

- Grok Build CLI (`grok`) installed and configured with an API key
- `GROK_CODE_XAI_API_KEY` environment variable set
- patchloom binary built with MCP support: `cargo build --all-features`

### Running

```bash
# Single run with default model
make bench-agent

# Specific model
make bench-agent MODEL=sxs-claude-opus-4-6

# Multiple runs for variance reduction
make bench-agent MODEL=sxs-claude-opus-4-6 RUNS=3

# Preview prompts without calling the API
make bench-agent-dry-run
```

### Dry-run mode

`make bench-agent-dry-run` prints all 11 task prompts (for each mode) without
calling any LLM API. Use this to:

- Review exactly what the agent will be asked
- Verify prompts are fair across modes
- Audit the benchmark methodology

### Results format

Results are saved as JSON to `benches/agent/results/<model>_<timestamp>.json`.

Each file contains:
- **`timestamp`**: When the benchmark ran
- **`model`**: Model identifier
- **`n_runs`**: Number of runs per mode
- **`prompts`**: The exact prompts sent to the agent for each task and mode
- Per mode (`patchloom`, `mcp`, `native`):
  - **`aggregate`**: Median/min/max per task and total, plus success rate
  - **`runs`**: Raw data for each run (duration, patchloom calls, success)

### Generating reports

```bash
# Compare most recent results
make bench-agent-report

# Compare a specific file
python3 benches/agent/report.py benches/agent/results/sxs-claude-opus-4-6_20260521_123522.json
```

The report shows a side-by-side comparison table with winner determination
and success rates.

### Methodology

- Tasks run in a **single persistent session** per mode to amortize AGENTS.md
  ingestion overhead (matches real-world usage).
- First-task latency is reported separately from subsequent-task averages.
- The `check` function for each task verifies actual workspace state (not just
  exit code), catching cases where the agent reports success but didn't make
  the correct changes.
- patchloom CLI overhead is measured via the shim (process spawn + execution
  time), typically under 50ms total across all tasks.
- With `RUNS=N` (N >= 3), statistics use the median to reduce outlier impact.

### Interpreting results

- **"PL exec" column**: Total time spent inside patchloom itself (ms). This is
  negligible compared to LLM inference time, proving patchloom adds near-zero
  overhead.
- **Winner determination**: A mode wins a task if its median is > 1.5s faster
  than the runner-up. Otherwise marked "~same".
- **Success rate < 1.0**: The agent failed the task's check function in at
  least one run. A star (*) marks these in the comparison table.
- **AGENTS.md ingestion overhead**: Estimated from `first_task_latency(patchloom) - first_task_latency(native) - (subsequent_avg(patchloom) - subsequent_avg(native))`.

---

## Existing results

Results are stored in `benches/agent/results/` and `benches/cli/results/`.
These are committed to the repo for historical comparison. Run
`make bench-agent-report` to view them.
