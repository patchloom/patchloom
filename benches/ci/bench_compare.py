"""Compare benchmark results against a baseline to detect regressions.

Usage: python3 bench_compare.py <baseline_dir> <current_dir> [threshold_pct] [min_abs_ms]

Scans for bench-*.json files (hyperfine output) in both directories,
matches them by name, and reports percentage changes. Catches gradual
regressions that slip under a static absolute threshold.

Arguments:
  baseline_dir   Directory containing baseline bench-*.json files
  current_dir    Directory containing current bench-*.json files
  threshold_pct  Regression threshold in percent (default: 20)
  min_abs_ms     Minimum absolute change in ms to count (default: 2)

Exit codes:
  0 - No regressions detected (or no baseline available)
  1 - At least one benchmark regressed beyond the threshold
"""

import json
import sys
from pathlib import Path


def load_results(directory: Path) -> dict[str, dict]:
    """Load all bench-*.json files from a directory."""
    results = {}
    for f in sorted(directory.glob("bench-*.json")):
        name = f.stem.removeprefix("bench-")
        with open(f, encoding="utf-8") as fh:
            data = json.load(fh)["results"][0]
        results[name] = data
    return results


def main() -> None:
    if len(sys.argv) < 3:
        raise SystemExit(
            f"Usage: {sys.argv[0]} <baseline_dir> <current_dir> "
            "[threshold_pct] [min_abs_ms]"
        )

    baseline_dir = Path(sys.argv[1])
    current_dir = Path(sys.argv[2])
    threshold_pct = float(sys.argv[3]) if len(sys.argv) > 3 else 20.0
    min_abs_ms = float(sys.argv[4]) if len(sys.argv) > 4 else 2.0

    baseline = load_results(baseline_dir)
    current = load_results(current_dir)

    if not baseline:
        print("No baseline results found; skipping comparison.")
        return

    if not current:
        print("No current results found; skipping comparison.")
        return

    common = sorted(set(baseline) & set(current))
    if not common:
        print("No matching benchmarks between baseline and current.")
        return

    print("## Benchmark Comparison (vs. baseline)\n")
    print("| Command | Baseline | Current | Change | Status |")
    print("|---------|----------|---------|--------|--------|")

    regressions = []
    for name in common:
        b_mean = baseline[name]["mean"] * 1000
        c_mean = current[name]["mean"] * 1000
        abs_diff = c_mean - b_mean

        if b_mean > 0:
            pct = (c_mean - b_mean) / b_mean * 100
        else:
            pct = 0.0

        is_regression = pct > threshold_pct and abs_diff > min_abs_ms
        if is_regression:
            status = "REGRESSION"
            regressions.append(name)
        elif pct < -5:
            status = "faster"
        else:
            status = "ok"

        sign = "+" if pct >= 0 else ""
        print(
            f"| {name} | {b_mean:.2f}ms | {c_mean:.2f}ms "
            f"| {sign}{pct:.1f}% | {status} |"
        )

    new_benches = sorted(set(current) - set(baseline))
    if new_benches:
        print(f"\nNew benchmarks (no baseline): {', '.join(new_benches)}")

    removed = sorted(set(baseline) - set(current))
    if removed:
        print(f"\nRemoved benchmarks: {', '.join(removed)}")

    print()

    if regressions:
        names = ", ".join(regressions)
        print(
            f"**{len(regressions)} regression(s) detected** "
            f"(>{threshold_pct}% slower and >{min_abs_ms}ms absolute change): "
            f"{names}"
        )
        sys.exit(1)
    else:
        print(f"All benchmarks within {threshold_pct}% of baseline.")


if __name__ == "__main__":
    main()
