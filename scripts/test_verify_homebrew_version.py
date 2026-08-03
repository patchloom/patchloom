#!/usr/bin/env python3
"""Unit tests for scripts/verify-homebrew-version.sh helpers (#2134)."""

from __future__ import annotations

import os
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "verify-homebrew-version.sh"


def _run(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    e = os.environ.copy()
    if env:
        e.update(env)
    return subprocess.run(
        ["bash", str(SCRIPT), *args],
        cwd=ROOT,
        env=e,
        capture_output=True,
        text=True,
        check=False,
    )


class VerifyHomebrewVersionTests(unittest.TestCase):
    def test_normalize_strips_tag_prefixes(self) -> None:
        for raw, want in [
            ("0.26.0", "0.26.0"),
            ("v0.26.0", "0.26.0"),
            ("patchloom-v0.26.0", "0.26.0"),
            ("patchloom 0.26.0", "0.26.0"),
        ]:
            r = _run("--normalize-version", raw)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(r.stdout.strip(), want, raw)

    def test_check_version_line_accepts_cli_output(self) -> None:
        r = _run("--check-version-line", "patchloom 0.26.0", "0.26.0")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("OK", r.stdout)

    def test_check_version_line_rejects_mismatch(self) -> None:
        r = _run("--check-version-line", "patchloom 0.25.0", "0.26.0")
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("FAIL", r.stderr)

    def test_missing_expected_version_is_usage_error(self) -> None:
        env = {
            "EXPECTED_VERSION": "",
            "PLAN": "",
            "PATH": os.environ.get("PATH", ""),
        }
        r = _run(env=env)
        self.assertEqual(r.returncode, 2, r.stderr)


if __name__ == "__main__":
    unittest.main()
