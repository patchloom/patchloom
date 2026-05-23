"""Check a hyperfine benchmark result against a wall-clock threshold.

Usage: python3 check_threshold.py <results.json> <name> [threshold_secs]

Exit 1 if the mean exceeds the threshold (default 0.05s / 50ms).
"""

import json
import sys


def main() -> None:
    if len(sys.argv) < 3:
        raise SystemExit(f"Usage: {sys.argv[0]} <results.json> <name> [threshold_secs]")

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
    print(
        f"{name} benchmark within threshold: "
        f"{mean * 1000:.2f}ms <= {threshold * 1000:.0f}ms"
    )


if __name__ == "__main__":
    main()
