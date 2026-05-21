#!/usr/bin/env python3
"""Generate a human-readable comparison report from agent benchmark results.

Usage:
    python3 report.py                                # most recent result
    python3 report.py results/model_timestamp.json   # specific file
    python3 report.py results/*.json                 # compare multiple models
"""

from __future__ import annotations

import json
import sys
from pathlib import Path


def load_result(path: Path) -> dict:
    return json.loads(path.read_text())


def format_table(data: dict) -> str:
    """Format a single results file as a comparison table."""
    model = data["model"]
    n_runs = data.get("n_runs", 1)
    modes = [m for m in ["patchloom", "mcp", "native"] if m in data]
    if not modes:
        return f"No mode data found in {model}\n"

    lines = []
    lines.append(f"## {model}")
    lines.append(f"")
    stat = "median" if n_runs > 1 else "single run"
    lines.append(f"Runs: {n_runs} ({stat})")
    lines.append(f"")

    # Header
    header = "| Task |"
    sep = "|------|"
    for m in modes:
        label = {"patchloom": "PL-CLI", "mcp": "MCP", "native": "Native"}[m]
        header += f" {label} (s) | OK |"
        sep += "-------:|:--:|"
    header += " Winner |"
    sep += "--------|"
    lines.append(header)
    lines.append(sep)

    # Gather task names from first mode
    first_agg = data[modes[0]]["aggregate"]
    task_names = list(first_agg["tasks"].keys())

    mode_totals = {m: data[m]["aggregate"]["median_total"] for m in modes}

    for name in task_names:
        row = f"| {name} |"
        times = {}
        for m in modes:
            ts = data[m]["aggregate"]["tasks"].get(name, {})
            t = ts.get("median", 0.0)
            sr = ts.get("success_rate", 0.0)
            times[m] = t
            ok = "Y" if sr >= 1.0 else f"{sr:.0%}"
            row += f" {t:>6.1f} | {ok} |"

        # Winner
        sorted_m = sorted(modes, key=lambda m: times[m])
        best = times[sorted_m[0]]
        second = times[sorted_m[1]]
        label_map = {"patchloom": "PL-CLI", "mcp": "MCP", "native": "Native"}
        winner = label_map[sorted_m[0]] if (second - best) > 1.5 else "~same"
        row += f" {winner} |"
        lines.append(row)

    # Total row
    row = "| **TOTAL** |"
    for m in modes:
        t = mode_totals[m]
        row += f" **{t:>5.1f}** | |"
    sorted_t = sorted(modes, key=lambda m: mode_totals[m])
    label_map = {"patchloom": "PL-CLI", "mcp": "MCP", "native": "Native"}
    row += f" **{label_map[sorted_t[0]]}** |"
    lines.append(row)

    # Success summary
    lines.append("")
    for m in modes:
        tasks = data[m]["aggregate"]["tasks"]
        passed = sum(1 for t in tasks.values() if t.get("success_rate", 0) >= 1.0)
        total = len(tasks)
        label = {"patchloom": "PL-CLI", "mcp": "MCP", "native": "Native"}[m]
        lines.append(f"- **{label}**: {passed}/{total} tasks passed")

    # Prompts (if available)
    if "prompts" in data:
        lines.append("")
        lines.append("### Prompts")
        lines.append("")
        for task_name, mode_prompts in data["prompts"].items():
            lines.append(f"**{task_name}**:")
            # Show default prompt, note if mode-specific variants exist
            default = mode_prompts.get("patchloom", mode_prompts.get("native", ""))
            lines.append(f"> {default}")
            variants = [m for m in mode_prompts if mode_prompts[m] != default]
            if variants:
                for m in variants:
                    lines.append(f"> *({m}):* {mode_prompts[m]}")
            lines.append("")

    return "\n".join(lines)


def main():
    results_dir = Path(__file__).parent / "results"

    if len(sys.argv) > 1:
        paths = [Path(p) for p in sys.argv[1:]]
    else:
        # Find most recent result
        jsons = sorted(results_dir.glob("*.json"), key=lambda p: p.stat().st_mtime)
        if not jsons:
            print("No results found. Run: make bench-agent", file=sys.stderr)
            sys.exit(1)
        paths = [jsons[-1]]

    for path in paths:
        if not path.exists():
            print(f"File not found: {path}", file=sys.stderr)
            continue
        data = load_result(path)
        print(f"# Agent Benchmark Report")
        print(f"")
        print(f"Source: `{path.name}`")
        print(f"")
        print(format_table(data))
        print()


if __name__ == "__main__":
    main()
