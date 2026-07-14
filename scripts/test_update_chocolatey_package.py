#!/usr/bin/env python3
"""Unit tests for scripts/update-chocolatey-package.py"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SCRIPT = ROOT / "update-chocolatey-package.py"

SAMPLE_NUSPEC = """<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd">
  <metadata>
    <id>patchloom</id>
    <version>0.1.0</version>
    <title>Patchloom</title>
  </metadata>
  <files>
    <file src="tools\\**" target="tools" />
  </files>
</package>
"""

SAMPLE_INSTALL = """$ErrorActionPreference = 'Stop'
$url64 = 'https://example.com/old-x64.zip'
$checksum64 = '0000000000000000000000000000000000000000000000000000000000000000'
$urlArm64 = 'https://example.com/old-arm64.zip'
$checksumArm64 = '1111111111111111111111111111111111111111111111111111111111111111'
Install-ChocolateyZipPackage @packageArgs
"""


def run_script(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        capture_output=True,
        text=True,
        check=False,
    )


class UpdateChocolateyPackageTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.pkg = Path(self.tmp.name) / "chocolatey"
        (self.pkg / "tools").mkdir(parents=True)
        (self.pkg / "patchloom.nuspec").write_text(SAMPLE_NUSPEC, encoding="utf-8")
        (self.pkg / "tools" / "chocolateyInstall.ps1").write_bytes(
            b"\xef\xbb\xbf" + SAMPLE_INSTALL.encode("utf-8")
        )

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_update_from_hash_args(self) -> None:
        hx = "a" * 64
        ha = "b" * 64
        r = run_script(
            [
                "--version",
                "0.13.0",
                "--hash-x64",
                hx,
                "--hash-arm64",
                ha,
                "--package-dir",
                str(self.pkg),
            ]
        )
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        nuspec = (self.pkg / "patchloom.nuspec").read_text(encoding="utf-8")
        self.assertIn("<version>0.13.0</version>", nuspec)
        raw = (self.pkg / "tools" / "chocolateyInstall.ps1").read_bytes()
        self.assertTrue(raw.startswith(b"\xef\xbb\xbf"), "must keep UTF-8 BOM")
        text = raw.decode("utf-8-sig")
        self.assertIn(
            "patchloom-v0.13.0/patchloom-x86_64-pc-windows-msvc.zip",
            text,
        )
        self.assertIn(
            "patchloom-v0.13.0/patchloom-aarch64-pc-windows-msvc.zip",
            text,
        )
        self.assertIn(f"$checksum64 = '{hx}'", text)
        self.assertIn(f"$checksumArm64 = '{ha}'", text)

    def test_update_from_artifacts_dir_nested(self) -> None:
        artifacts = Path(self.tmp.name) / "artifacts" / "nested"
        artifacts.mkdir(parents=True)
        (artifacts / "patchloom-x86_64-pc-windows-msvc.zip.sha256").write_text(
            "c" * 64 + " *patchloom-x86_64-pc-windows-msvc.zip\n",
            encoding="utf-8",
        )
        (artifacts / "patchloom-aarch64-pc-windows-msvc.zip.sha256").write_text(
            "d" * 64 + " *patchloom-aarch64-pc-windows-msvc.zip\n",
            encoding="utf-8",
        )
        r = run_script(
            [
                "--version",
                "patchloom-v1.2.3",
                "--artifacts-dir",
                str(Path(self.tmp.name) / "artifacts"),
                "--package-dir",
                str(self.pkg),
            ]
        )
        self.assertEqual(r.returncode, 0, r.stderr + r.stdout)
        text = (self.pkg / "tools" / "chocolateyInstall.ps1").read_text(
            encoding="utf-8-sig"
        )
        self.assertIn("$checksum64 = '" + "c" * 64 + "'", text)
        self.assertIn("$checksumArm64 = '" + "d" * 64 + "'", text)
        self.assertIn("<version>1.2.3</version>", (self.pkg / "patchloom.nuspec").read_text())

    def test_check_stale_exits_1(self) -> None:
        r = run_script(
            [
                "--version",
                "9.9.9",
                "--hash-x64",
                "e" * 64,
                "--hash-arm64",
                "f" * 64,
                "--package-dir",
                str(self.pkg),
                "--check",
            ]
        )
        self.assertEqual(r.returncode, 1, r.stderr + r.stdout)

    def test_check_current_exits_0(self) -> None:
        # First write
        hx, ha = "1" * 64, "2" * 64
        r1 = run_script(
            [
                "--version",
                "0.2.0",
                "--hash-x64",
                hx,
                "--hash-arm64",
                ha,
                "--package-dir",
                str(self.pkg),
            ]
        )
        self.assertEqual(r1.returncode, 0, r1.stderr)
        r2 = run_script(
            [
                "--version",
                "0.2.0",
                "--hash-x64",
                hx,
                "--hash-arm64",
                ha,
                "--package-dir",
                str(self.pkg),
                "--check",
            ]
        )
        self.assertEqual(r2.returncode, 0, r2.stderr + r2.stdout)

    def test_missing_hash_fails(self) -> None:
        r = run_script(
            [
                "--version",
                "0.1.0",
                "--artifacts-dir",
                str(Path(self.tmp.name)),
                "--package-dir",
                str(self.pkg),
            ]
        )
        self.assertNotEqual(r.returncode, 0)


if __name__ == "__main__":
    unittest.main()
