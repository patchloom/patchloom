"""Print a summary table of all benchmark results in a directory.

Usage: python3 bench_summary.py <directory>

Scans for bench-*.json files (hyperfine output) and prints a
markdown-style table with mean, median, min, max, and stddev.
"""

import json
import os
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) < 2:
        raise SystemExit(f"Usage: {sys.argv[0]} <directory>")

    bench_dir = Path(sys.argv[1])
    files = sorted(bench_dir.glob("bench-*.json"))

    if not files:
        print("No benchmark result files found.")
        return

    rows: list[tuple[str, float, float, float, float, float]] = []
    for f in files:
        name = f.stem.removeprefix("bench-")
        with open(f, encoding="utf-8") as fh:
            result = json.load(fh)["results"][0]
        rows.append((
            name,
            result["mean"] * 1000,
            result["median"] * 1000,
            result["min"] * 1000,
            result["max"] * 1000,
            result["stddev"] * 1000,
        ))

    print("\n=== Benchmark Summary ===")
    print(f"{'Command':<20} {'Mean':>8} {'Median':>8} {'Min':>8} {'Max':>8} {'StdDev':>8}")
    print("-" * 72)
    for name, mean, median, mn, mx, sd in rows:
        print(f"{name:<20} {mean:>7.2f}ms {median:>7.2f}ms {mn:>7.2f}ms {mx:>7.2f}ms {sd:>7.2f}ms")
    print()


if __name__ == "__main__":
    main()
