#!/usr/bin/env python3
"""Force version fields on the release-please branch (make force-release-version).

Updates .release-please-manifest.json package key ".", Cargo.toml version,
Cargo.lock root package version, and the top versioned CHANGELOG section.

Intentionally does NOT use naive sed on the manifest: replacing the first
quoted string turns {".": "0.25.1"} into {"0.26.0": "0.25.1"} (bug hit when
forcing PR #2114 from 0.25.1 to 0.26.0 for SoftTextSkip non_exhaustive).
"""
from __future__ import annotations

import json
import pathlib
import re
import sys


def main() -> int:
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        print("usage: force_release_version.py X.Y.Z", file=sys.stderr)
        return 2
    v = sys.argv[1]
    root = pathlib.Path.cwd()

    manifest = root / ".release-please-manifest.json"
    data = json.loads(manifest.read_text())
    data["."] = v
    manifest.write_text(json.dumps(data, indent=2) + "\n")
    print(f"manifest . -> {v}")

    cargo = root / "Cargo.toml"
    cargo_text = cargo.read_text()
    cargo2, n = re.subn(
        r'(?m)^version = ".*"', f'version = "{v}"', cargo_text, count=1
    )
    if n != 1:
        print("error: Cargo.toml version line not updated", file=sys.stderr)
        return 1
    cargo.write_text(cargo2)
    print(f"Cargo.toml version -> {v}")

    lock = root / "Cargo.lock"
    lines = lock.read_text().splitlines(True)
    out: list[str] = []
    i = 0
    updated_lock = False
    while i < len(lines):
        line = lines[i]
        out.append(line)
        if (
            line.strip() == 'name = "patchloom"'
            and i + 1 < len(lines)
            and lines[i + 1].startswith("version =")
        ):
            i += 1
            out.append(f'version = "{v}"\n')
            updated_lock = True
            i += 1
            continue
        i += 1
    if not updated_lock:
        print("error: Cargo.lock root package version not found", file=sys.stderr)
        return 1
    lock.write_text("".join(out))
    print(f"Cargo.lock patchloom version -> {v}")

    cl = root / "CHANGELOG.md"
    t = cl.read_text()
    m = re.search(
        r"(## \[)([0-9]+\.[0-9]+\.[0-9]+)"
        r"(\]\(https://github.com/patchloom/patchloom/compare/patchloom-v)"
        r"([0-9.]+)(\.\.\.patchloom-v)([0-9.]+)(\))",
        t,
    )
    if m and m.group(2) != v:
        t = (
            t[: m.start()]
            + m.group(1)
            + v
            + m.group(3)
            + m.group(4)
            + m.group(5)
            + v
            + m.group(7)
            + t[m.end() :]
        )
        cl.write_text(t)
        print(f"CHANGELOG {m.group(2)} -> {v}")
    else:
        print("CHANGELOG ok or missing versioned section")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
