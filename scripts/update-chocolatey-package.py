#!/usr/bin/env python3
"""Update Chocolatey package version + Windows zip checksums.

Release CI runs this after GitHub Release assets exist so chocolatey/ matches
the published zips. Then CI packs and (when CHOCOLATEY_API_KEY is set) pushes
to the Chocolatey Community Repository.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

TAG_PREFIX = "patchloom-v"
X64_ZIP = "patchloom-x86_64-pc-windows-msvc.zip"
ARM64_ZIP = "patchloom-aarch64-pc-windows-msvc.zip"

HASH_RE = re.compile(r"^([a-fA-F0-9]{64})\b")
VERSION_RE = re.compile(
    r"(<version>)([^<]+)(</version>)",
    re.IGNORECASE,
)
# Match assignment lines in chocolateyInstall.ps1
ASSIGN_RE = {
    "url64": re.compile(
        r"^(\$url64\s*=\s*')([^']*)(')\s*$", re.MULTILINE
    ),
    "checksum64": re.compile(
        r"^(\$checksum64\s*=\s*')([^']*)(')\s*$", re.MULTILINE
    ),
    "urlArm64": re.compile(
        r"^(\$urlArm64\s*=\s*')([^']*)(')\s*$", re.MULTILINE
    ),
    "checksumArm64": re.compile(
        r"^(\$checksumArm64\s*=\s*')([^']*)(')\s*$", re.MULTILINE
    ),
}


def parse_hash_file(path: Path) -> str:
    text = path.read_text(encoding="utf-8").strip()
    if not text:
        raise SystemExit(f"empty hash file: {path}")
    m = HASH_RE.match(text.splitlines()[0].strip())
    if not m:
        raise SystemExit(f"could not parse SHA256 from {path}: {text!r}")
    return m.group(1).lower()


def find_hash(artifacts_dir: Path, zip_name: str) -> str:
    candidates = list(artifacts_dir.rglob(f"{zip_name}.sha256"))
    if not candidates:
        raise SystemExit(
            f"missing {zip_name}.sha256 under {artifacts_dir} "
            f"(found {len(list(artifacts_dir.rglob('*')))} files)"
        )
    candidates.sort(key=lambda p: len(p.parts))
    return parse_hash_file(candidates[0])


def normalize_version(version: str) -> str:
    v = version.strip()
    if v.startswith(TAG_PREFIX):
        v = v[len(TAG_PREFIX) :]
    if v.startswith("v"):
        v = v[1:]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", v):
        raise SystemExit(f"invalid semver-ish version: {version!r} -> {v!r}")
    return v


def release_urls(version: str) -> tuple[str, str]:
    tag = f"{TAG_PREFIX}{version}"
    base = f"https://github.com/patchloom/patchloom/releases/download/{tag}"
    return f"{base}/{X64_ZIP}", f"{base}/{ARM64_ZIP}"


def update_nuspec(text: str, version: str) -> str:
    if not VERSION_RE.search(text):
        raise SystemExit("nuspec missing <version> element")
    return VERSION_RE.sub(rf"\g<1>{version}\g<3>", text, count=1)


def update_install_ps1(
    text: str, *, url64: str, hash64: str, url_arm64: str, hash_arm64: str
) -> str:
    # Preserve UTF-8 BOM if present when reading as str is fine; caller owns bytes.
    replacements = {
        "url64": url64,
        "checksum64": hash64.lower(),
        "urlArm64": url_arm64,
        "checksumArm64": hash_arm64.lower(),
    }
    out = text
    for key, value in replacements.items():
        pat = ASSIGN_RE[key]
        if not pat.search(out):
            raise SystemExit(f"chocolateyInstall.ps1 missing ${key} assignment")
        out = pat.sub(rf"\g<1>{value}\g<3>", out, count=1)
    return out


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--version",
        required=True,
        help="Semver without tag prefix, e.g. 0.13.0",
    )
    p.add_argument(
        "--artifacts-dir",
        type=Path,
        help="Directory with cargo-dist artifacts (searched for *.zip.sha256)",
    )
    p.add_argument("--hash-x64", help="SHA256 of the x86_64 Windows zip")
    p.add_argument("--hash-arm64", help="SHA256 of the aarch64 Windows zip")
    p.add_argument(
        "--package-dir",
        type=Path,
        default=Path("chocolatey"),
        help="Directory containing patchloom.nuspec and tools/",
    )
    p.add_argument(
        "--check",
        action="store_true",
        help="Exit 1 if files would change; do not write",
    )
    args = p.parse_args()

    version = normalize_version(args.version)
    if args.artifacts_dir:
        hash_x64 = find_hash(args.artifacts_dir, X64_ZIP)
        hash_arm64 = find_hash(args.artifacts_dir, ARM64_ZIP)
    elif args.hash_x64 and args.hash_arm64:
        hash_x64 = args.hash_x64.lower()
        hash_arm64 = args.hash_arm64.lower()
    else:
        raise SystemExit("provide --artifacts-dir or both --hash-x64 and --hash-arm64")

    url64, url_arm64 = release_urls(version)
    pkg = args.package_dir
    nuspec_path = pkg / "patchloom.nuspec"
    install_path = pkg / "tools" / "chocolateyInstall.ps1"
    if not nuspec_path.is_file():
        raise SystemExit(f"missing {nuspec_path}")
    if not install_path.is_file():
        raise SystemExit(f"missing {install_path}")

    raw_install = install_path.read_bytes()
    bom = raw_install.startswith(b"\xef\xbb\xbf")
    install_text = raw_install.decode("utf-8-sig")
    nuspec_text = nuspec_path.read_text(encoding="utf-8")

    new_nuspec = update_nuspec(nuspec_text, version)
    new_install = update_install_ps1(
        install_text,
        url64=url64,
        hash64=hash_x64,
        url_arm64=url_arm64,
        hash_arm64=hash_arm64,
    )

    changed = new_nuspec != nuspec_text or new_install != install_text
    if args.check:
        if changed:
            print("Chocolatey package is stale vs requested version/hashes", file=sys.stderr)
            return 1
        print(f"Chocolatey package already at {version}")
        return 0

    if not changed:
        print(f"Chocolatey package already at {version}; nothing to write")
        return 0

    nuspec_path.write_text(new_nuspec, encoding="utf-8")
    payload = new_install.encode("utf-8")
    if bom:
        payload = b"\xef\xbb\xbf" + payload
    install_path.write_bytes(payload)
    print(f"Updated Chocolatey package to {version}")
    print(f"  x64    {hash_x64}")
    print(f"  arm64  {hash_arm64}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
