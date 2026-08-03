#!/usr/bin/env python3
"""EMERGENCY: force version fields on the release-please branch.

**Preferred (durable) path: do this on main instead:**

    git commit --allow-empty -s \\
      -m "chore: release X.Y.Z" \\
      -m "Release-As: X.Y.Z"

Or put ``Release-As: X.Y.Z`` in the body of a normal main PR (e.g. curated
``RELEASE_NOTES.md``). On squash-merge, keep the footer in the squash body.
For public API breaks, prefer ``fix!:`` / ``feat!:`` or ``BREAKING CHANGE``.

See ``~/.grok/skills/ci-build-release/SKILL.md`` section
"Force next release version (Release-As)". Project notes:
``~/.grok/skills/patchloom-contrib/SKILL.md`` (same section name).

Edits to the synthetic release-please branch are **wiped** when main
advances. This script only rewrites branch files for a short-lived CI
unblock; land ``Release-As`` on main immediately after.

Updates .release-please-manifest.json package key ".", Cargo.toml version,
Cargo.lock root package version, and the top versioned CHANGELOG section.

Does NOT use naive sed on the manifest: replacing the first quoted string
turns {".": "0.25.1"} into {"0.26.0": "0.25.1"} (bug hit on #2114).
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ACK_FLAG = "--i-know-this-is-wiped"
VERSION_RE = re.compile(r"\d+\.\d+\.\d+")


def preferred_path_message(version: str = "X.Y.Z") -> str:
    return f"""\
PREFERRED (durable): land on **main**, do not rely on this script:

  git commit --allow-empty -s \\
    -m "chore: release {version}" \\
    -m "Release-As: {version}"

Or: Release-As footer on a normal main PR (keep footer on squash-merge).
API break: use fix!: / feat!: / BREAKING CHANGE (bump-minor-pre-major -> 0.x minor).

Docs: ci-build-release skill -> "Force next release version (Release-As)"

This tool only rewrites the release-please **branch** and is wiped on the
next main merge. After any emergency force, land Release-As on main ASAP.
"""


def usage() -> None:
    print(preferred_path_message(), file=sys.stderr)
    print(
        f"EMERGENCY usage (branch only):\n"
        f"  force_release_version.py {ACK_FLAG} X.Y.Z\n"
        f"  make force-release-version VERSION=X.Y.Z\n",
        file=sys.stderr,
    )


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)

    if not args or args in (["-h"], ["--help"], ["help"]):
        usage()
        return 2

    if ACK_FLAG not in args:
        print(
            f"error: refusing to run without {ACK_FLAG}\n"
            f"(this is the emergency branch path; prefer Release-As on main)\n",
            file=sys.stderr,
        )
        usage()
        return 2

    args = [a for a in args if a != ACK_FLAG]
    if len(args) != 1 or not VERSION_RE.fullmatch(args[0]):
        usage()
        return 2

    v = args[0]
    # Always restate the durable path so agents/logs see it.
    print(preferred_path_message(v), file=sys.stderr)
    print(
        f"EMERGENCY: rewriting release-please branch files to {v} "
        f"(will not survive the next main merge)\n",
        file=sys.stderr,
    )

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

    print(
        f"\nDone. Next: land Release-As: {v} on main so the next release-please "
        f"rebuild keeps this version.",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
