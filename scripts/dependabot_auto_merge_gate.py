#!/usr/bin/env python3
"""Decide whether a Dependabot PR may arm auto-merge.

dependabot/fetch-metadata reports the highest update-type in a group.
Its calculateUpdateType splits versions on '.' and treats any difference
in the first piece as semver-major. Two 40-hex commit pins have no dots,
so a pin move is labeled major even when the floating tag did not change.
A real major still has a non-pin previous version.
"""

from __future__ import annotations

import json
import os
import re
import sys

MAJOR = "version-update:semver-major"
_SHA = re.compile(r"^[0-9a-fA-F]{40}$")


def _is_sha(version: str) -> bool:
    return _SHA.fullmatch(version) is not None


def _is_real_major(dependency: object) -> bool:
    if not isinstance(dependency, dict):
        return True
    if dependency.get("updateType") != MAJOR:
        return False
    prev = str(dependency.get("prevVersion") or "")
    new = str(dependency.get("newVersion") or "")
    if _is_sha(prev) and _is_sha(new):
        return False
    return True


def should_auto_merge(
    update_type: str,
    dependencies: list,
    previous_version: str,
) -> bool:
    """Return true when this bump is safe to auto-merge."""
    if update_type != MAJOR:
        return True
    if not dependencies:
        return _is_sha(previous_version)
    return not any(_is_real_major(dep) for dep in dependencies)


def decide(update_type: str, deps_json: str, previous_version: str) -> bool:
    raw = deps_json.strip()
    if not raw:
        return should_auto_merge(update_type, [], previous_version)
    try:
        parsed = json.loads(raw)
    except json.JSONDecodeError:
        return update_type != MAJOR
    if not isinstance(parsed, list):
        return update_type != MAJOR
    return should_auto_merge(update_type, parsed, previous_version)


def main() -> int:
    allow = decide(
        os.environ.get("UPDATE_TYPE", ""),
        os.environ.get("DEPS_JSON", ""),
        os.environ.get("PREVIOUS_VERSION", ""),
    )
    line = f"auto_merge={'true' if allow else 'false'}\n"
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as handle:
            handle.write(line)
    else:
        sys.stdout.write(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
