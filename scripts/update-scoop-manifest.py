#!/usr/bin/env python3
"""Generate or update the Scoop bucket manifest for patchloom.

Release CI runs this after GitHub Release assets exist so patchloom/scoop-bucket
always pins the new version + SHA256. Client-side Scoop checkver/autoupdate is
kept as a fallback only; the committed JSON is the source of truth.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

# cargo-dist tag format for this project
TAG_PREFIX = "patchloom-v"

X64_ZIP = "patchloom-x86_64-pc-windows-msvc.zip"
ARM64_ZIP = "patchloom-aarch64-pc-windows-msvc.zip"

HASH_RE = re.compile(r"^([a-fA-F0-9]{64})\b")


def parse_hash_file(path: Path) -> str:
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        raise SystemExit(f"empty hash file: {path}")
    m = HASH_RE.match(text.splitlines()[0].strip())
    if not m:
        raise SystemExit(f"could not parse SHA256 from {path}: {text!r}")
    return m.group(1).lower()


def find_hash(artifacts_dir: Path, zip_name: str) -> str:
    """Locate zip.sha256 under a recursive artifacts download tree."""
    candidates = list(artifacts_dir.rglob(f"{zip_name}.sha256"))
    if not candidates:
        raise SystemExit(
            f"missing {zip_name}.sha256 under {artifacts_dir} "
            f"(found {len(list(artifacts_dir.rglob('*')))} files)"
        )
    if len(candidates) > 1:
        # prefer shallowest
        candidates.sort(key=lambda p: len(p.parts))
    return parse_hash_file(candidates[0])


def build_manifest(version: str, hash_x64: str, hash_arm64: str) -> dict:
    tag = f"{TAG_PREFIX}{version}"
    base = f"https://github.com/patchloom/patchloom/releases/download/{tag}"
    return {
        "version": version,
        "description": (
            "Structured file editing CLI for AI agents: search, replace, "
            "patch, doc, md, AST, and MCP."
        ),
        "homepage": "https://github.com/patchloom/patchloom",
        "license": "MIT|Apache-2.0",
        "architecture": {
            "64bit": {
                "url": f"{base}/{X64_ZIP}",
                "hash": hash_x64.lower(),
            },
            "arm64": {
                "url": f"{base}/{ARM64_ZIP}",
                "hash": hash_arm64.lower(),
            },
        },
        "bin": "patchloom.exe",
        # Fallback if the bucket JSON lags; release CI should keep version current.
        "checkver": {
            "url": "https://api.github.com/repos/patchloom/patchloom/releases/latest",
            "jsonpath": "$.tag_name",
            "regex": r"patchloom-v([\d.]+)",
        },
        "autoupdate": {
            "architecture": {
                "64bit": {
                    "url": (
                        "https://github.com/patchloom/patchloom/releases/download/"
                        "patchloom-v$version/" + X64_ZIP
                    ),
                    "hash": {
                        "url": "$url.sha256",
                        "regex": r"^([a-fA-F0-9]{64})",
                    },
                },
                "arm64": {
                    "url": (
                        "https://github.com/patchloom/patchloom/releases/download/"
                        "patchloom-v$version/" + ARM64_ZIP
                    ),
                    "hash": {
                        "url": "$url.sha256",
                        "regex": r"^([a-fA-F0-9]{64})",
                    },
                },
            }
        },
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--version", required=True, help="Semver without tag prefix, e.g. 0.13.0")
    p.add_argument(
        "--artifacts-dir",
        type=Path,
        help="Directory with cargo-dist artifacts (searched for *.zip.sha256)",
    )
    p.add_argument("--hash-x64", help="SHA256 of the x86_64 Windows zip")
    p.add_argument("--hash-arm64", help="SHA256 of the aarch64 Windows zip")
    p.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Path to write bucket/patchloom.json",
    )
    p.add_argument(
        "--check",
        action="store_true",
        help="Exit 0 only if existing --output matches generated content",
    )
    args = p.parse_args()

    version = args.version.lstrip("v")
    if version.startswith(TAG_PREFIX):
        version = version[len(TAG_PREFIX) :]

    if args.artifacts_dir is not None:
        hash_x64 = find_hash(args.artifacts_dir, X64_ZIP)
        hash_arm64 = find_hash(args.artifacts_dir, ARM64_ZIP)
    elif args.hash_x64 is not None and args.hash_arm64 is not None:
        hash_x64 = args.hash_x64
        hash_arm64 = args.hash_arm64
    else:
        raise SystemExit(
            "error: provide --artifacts-dir or both --hash-x64 and --hash-arm64"
        )

    manifest = build_manifest(version, hash_x64, hash_arm64)
    text = json.dumps(manifest, indent=4) + "\n"

    if args.check:
        if not args.output.is_file():
            print(f"missing {args.output}", file=sys.stderr)
            return 1
        existing = args.output.read_text(encoding="utf-8")
        if existing != text:
            print(f"{args.output} is stale for version {version}", file=sys.stderr)
            return 1
        print(f"ok: {args.output} matches {version}")
        return 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(text, encoding="utf-8")
    print(f"wrote {args.output} for version {version}")
    print(f"  64bit  {hash_x64}")
    print(f"  arm64  {hash_arm64}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
