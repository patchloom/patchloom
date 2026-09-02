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
    if "actionlint@" not in ci or "zizmor@" not in ci:
        return fail("pin actionlint and zizmor versions in taiki-e/install-action")
    if "needs.workflow-sanity.result" not in ci:
        return fail("aggregate ci job must fail when workflow-sanity fails")
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
    print("ok: workflow-sanity job, path filter, pinned linters, and lock target")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
