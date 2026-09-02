#!/usr/bin/env python3
"""Lock CI workflow-sanity job, path filter, and pinned linters (#2279)."""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CI = ROOT / ".github" / "workflows" / "ci.yml"
MAKEFILE = ROOT / "Makefile"
ZIZMOR = ROOT / ".github" / "zizmor.yml"


def fail(msg: str) -> int:
    print(msg, file=sys.stderr)
    return 1


def main() -> int:
    ci = CI.read_text(encoding="utf-8")
    makefile = MAKEFILE.read_text(encoding="utf-8")

    if "workflow-sanity:" not in ci:
        return fail("ci.yml must define a workflow-sanity job")
    if ".github/workflows/**" not in ci:
        return fail("ci.yml path filter must include .github/workflows/**")
    if ".github/actions/**" not in ci:
        return fail("ci.yml path filter must include .github/actions/**")
    if "workflows:" not in ci:
        return fail("ci.yml must expose a workflows paths-filter output")
    if "needs.changes.outputs.workflows == 'true'" not in ci:
        return fail("workflow-sanity must gate on needs.changes.outputs.workflows")
    if "actionlint" not in ci or "zizmor" not in ci:
        return fail("workflow-sanity must run actionlint and zizmor")
    if re.search(r"actionlint@latest|zizmor@latest", ci):
        return fail("do not install actionlint or zizmor at @latest")
    if "zizmor@1.30.0" not in ci:
        return fail("pin zizmor@1.30.0 (not @latest)")
    if "actionlint_1.7.12" not in ci and "actionlint@1.7.12" not in ci:
        return fail("pin actionlint 1.7.12 (release tarball or install-action)")
    if "sha256sum -c" not in ci:
        return fail("workflow-sanity must verify the actionlint tarball with sha256sum -c")
    if "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8" not in ci:
        return fail(
            "pin actionlint 1.7.12 tarball sha256 "
            "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
        )
    if "needs.workflow-sanity.result" not in ci:
        return fail("aggregate ci job must fail when workflow-sanity fails")
    job_match = re.search(
        r"(?ms)^  workflow-sanity:.*?(?=^  [A-Za-z0-9_-]+:|\Z)",
        ci,
    )
    if not job_match:
        return fail("could not isolate the workflow-sanity job")
    job = job_match.group(0)
    if "timeout-minutes:" not in job:
        return fail("workflow-sanity must set timeout-minutes")
    if "persist-credentials: false" not in job:
        return fail("workflow-sanity must set persist-credentials: false")
    if "harden-runner" not in job:
        return fail("workflow-sanity must use harden-runner")
    if not ZIZMOR.is_file():
        return fail(".github/zizmor.yml is required for documented suppressions")
    if "workflow-sanity-test:" not in makefile:
        return fail("Makefile must define workflow-sanity-test")
    check_line = next(
        (line for line in makefile.splitlines() if line.startswith("check:")),
        "",
    )
    if "workflow-sanity-test" not in check_line:
        return fail("make check must include workflow-sanity-test")
    check_fast_line = next(
        (line for line in makefile.splitlines() if line.startswith("check-fast:")),
        "",
    )
    if "workflow-sanity-test" not in check_fast_line:
        return fail("make check-fast must include workflow-sanity-test")
    print("ok: workflow-sanity job, path filter, pinned linters, and lock target")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
