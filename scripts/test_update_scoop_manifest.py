#!/usr/bin/env python3
"""Tests for scripts/update-scoop-manifest.py"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "update-scoop-manifest.py"


class UpdateScoopManifestTests(unittest.TestCase):
    def test_write_from_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / "patchloom.json"
            r = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "0.13.0",
                    "--hash-x64",
                    "b194104e93904c82a9ffd30b5a6f9125b1ca41848b24c63353ec4bd775c332f1",
                    "--hash-arm64",
                    "1e96356a395d74c86eac9e1ef63617bd2e87f755cb6cd158e97cabc0f1a8dc5d",
                    "--output",
                    str(out),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertIn("wrote", r.stdout)
            data = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(data["version"], "0.13.0")
            self.assertEqual(data["bin"], "patchloom.exe")
            self.assertIn("patchloom-v0.13.0", data["architecture"]["64bit"]["url"])
            self.assertIn(
                "patchloom-x86_64-pc-windows-msvc.zip",
                data["architecture"]["64bit"]["url"],
            )
            self.assertEqual(
                data["architecture"]["64bit"]["hash"],
                "b194104e93904c82a9ffd30b5a6f9125b1ca41848b24c63353ec4bd775c332f1",
            )
            self.assertIn("patchloom-v$version", data["autoupdate"]["architecture"]["64bit"]["url"])

    def test_artifacts_dir_and_check(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            art = root / "artifacts" / "nested"
            art.mkdir(parents=True)
            (art / "patchloom-x86_64-pc-windows-msvc.zip.sha256").write_text(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa "
                "*patchloom-x86_64-pc-windows-msvc.zip\n",
                encoding="utf-8",
            )
            (art / "patchloom-aarch64-pc-windows-msvc.zip.sha256").write_text(
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb "
                "*patchloom-aarch64-pc-windows-msvc.zip\n",
                encoding="utf-8",
            )
            out = root / "bucket" / "patchloom.json"
            subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "patchloom-v1.2.3",
                    "--artifacts-dir",
                    str(root / "artifacts"),
                    "--output",
                    str(out),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            data = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(data["version"], "1.2.3")
            self.assertEqual(
                data["architecture"]["64bit"]["hash"],
                "a" * 64,
            )
            self.assertEqual(
                data["architecture"]["arm64"]["hash"],
                "b" * 64,
            )
            # --check must pass for same content
            r = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "1.2.3",
                    "--hash-x64",
                    "a" * 64,
                    "--hash-arm64",
                    "b" * 64,
                    "--output",
                    str(out),
                    "--check",
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r.returncode, 0, r.stderr)

    def test_missing_hash_file_fails(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            out = root / "out.json"
            r = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--version",
                    "0.1.0",
                    "--artifacts-dir",
                    str(root),
                    "--output",
                    str(out),
                ],
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("missing", r.stderr)


if __name__ == "__main__":
    unittest.main()
