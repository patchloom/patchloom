#!/usr/bin/env python3
"""Check that a hyperfine benchmark result is within a threshold.

Usage: check-threshold.py <hyperfine-json> <name> [threshold-seconds]

Exits non-zero if the mean exceeds the threshold (default 0.05s / 50ms).
"""
import json
import sys


def main() -> None:
    if len(sys.argv) < 3:
        raise SystemExit(f"usage: {sys.argv[0]} <hyperfine-json> <name> [threshold-seconds]")

    bench_json = sys.argv[1]
    name = sys.argv[2]
    threshold = float(sys.argv[3]) if len(sys.argv) > 3 else 0.05

    with open(bench_json, encoding="utf-8") as f:
        mean = json.load(f)["results"][0]["mean"]

    if mean > threshold:
        raise SystemExit(
            f"{name} benchmark exceeded threshold: "
            f"{mean * 1000:.2f}ms > {threshold * 1000:.0f}ms"
        )
    print(f"{name} benchmark within threshold: {mean * 1000:.2f}ms <= {threshold * 1000:.0f}ms")


if __name__ == "__main__":
    main()
