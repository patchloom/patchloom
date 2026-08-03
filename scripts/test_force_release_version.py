#!/usr/bin/env python3
"""Tests for scripts/force_release_version.py preferred-path messaging."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "force_release_version.py"


def _load_module():
    spec = importlib.util.spec_from_file_location("force_release_version", SCRIPT)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class ForceReleaseVersionTests(unittest.TestCase):
    def test_refuses_without_ack_flag(self) -> None:
        r = subprocess.run(
            [sys.executable, str(SCRIPT), "0.26.0"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(r.returncode, 2)
        self.assertIn("Release-As", r.stderr)
        self.assertIn("--i-know-this-is-wiped", r.stderr)
        self.assertIn("PREFERRED", r.stderr)

    def test_help_mentions_preferred_path(self) -> None:
        r = subprocess.run(
            [sys.executable, str(SCRIPT), "--help"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(r.returncode, 2)
        self.assertIn("Release-As", r.stderr)
        self.assertIn("ci-build-release", r.stderr)

    def test_ack_rewrites_manifest_key_dot(self) -> None:
        mod = _load_module()
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / ".release-please-manifest.json").write_text(
                json.dumps({".": "0.25.1"}, indent=2) + "\n"
            )
            (root / "Cargo.toml").write_text(
                '[package]\nname = "patchloom"\nversion = "0.25.1"\n'
            )
            (root / "Cargo.lock").write_text(
                'version = 4\n\n[[package]]\nname = "patchloom"\nversion = "0.25.1"\n'
            )
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n## [0.25.1](https://github.com/patchloom/patchloom/"
                "compare/patchloom-v0.25.0...patchloom-v0.25.1)\n"
            )
            # Run in temp dir via chdir
            import os

            old = os.getcwd()
            try:
                os.chdir(root)
                code = mod.main(["--i-know-this-is-wiped", "0.26.0"])
            finally:
                os.chdir(old)
            self.assertEqual(code, 0)
            data = json.loads((root / ".release-please-manifest.json").read_text())
            self.assertEqual(data, {".": "0.26.0"})
            self.assertNotIn("0.26.0", list(data.keys()))
            cargo = (root / "Cargo.toml").read_text()
            self.assertIn('version = "0.26.0"', cargo)


if __name__ == "__main__":
    unittest.main()
