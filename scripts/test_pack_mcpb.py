#!/usr/bin/env python3
"""Tests for scripts/pack-mcpb.sh version resolution and pack stamping."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "pack-mcpb.sh"


def _run_pack(env: dict[str, str], *extra_env: tuple[str, str]) -> subprocess.CompletedProcess[str]:
    e = os.environ.copy()
    e.update(env)
    for k, v in extra_env:
        e[k] = v
    return subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=ROOT,
        env=e,
        capture_output=True,
        text=True,
        check=False,
    )


class PackMcpbTests(unittest.TestCase):
    def test_print_version_from_cargo(self) -> None:
        r = _run_pack({"PACK_MCPB_PRINT_VERSION_ONLY": "1"})
        self.assertEqual(r.returncode, 0, r.stderr)
        cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        import re

        m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', cargo)
        self.assertIsNotNone(m)
        self.assertEqual(r.stdout.strip(), m.group(1))

    def test_print_version_honors_version_env(self) -> None:
        r = _run_pack(
            {"PACK_MCPB_PRINT_VERSION_ONLY": "1", "VERSION": "9.9.9-test"}
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.strip(), "9.9.9-test")

    def test_pack_stamps_manifest_and_package_when_mcpb_available(self) -> None:
        if shutil.which("mcpb") is None:
            self.skipTest("mcpb CLI not installed")
        if shutil.which("jq") is None:
            self.skipTest("jq not installed")

        ver = "0.0.0-packtest"
        out = ROOT / "target" / "mcpb" / f"patchloom-{ver}.mcpb"
        if out.exists():
            out.unlink()

        r = _run_pack({"VERSION": ver})
        self.assertEqual(r.returncode, 0, f"stdout={r.stdout}\nstderr={r.stderr}")
        self.assertTrue(out.is_file(), f"missing {out}")

        with zipfile.ZipFile(out) as zf:
            names = set(zf.namelist())
            self.assertIn("manifest.json", names)
            self.assertIn("package.json", names)
            self.assertIn("server/run.mjs", names)
            manifest = json.loads(zf.read("manifest.json"))
            package = json.loads(zf.read("package.json"))

        self.assertEqual(manifest["version"], ver)
        self.assertEqual(package["version"], ver)
        args = manifest["server"]["mcp_config"]["args"]
        self.assertEqual(args, ["-y", f"patchloom@{ver}", "mcp-server"])
        win_args = manifest["server"]["mcp_config"]["platform_overrides"]["win32"][
            "args"
        ]
        self.assertEqual(win_args, ["-y", f"patchloom@{ver}", "mcp-server"])

        # Launcher must load stamped package.json version for npx fallback.
        with zipfile.ZipFile(out) as zf:
            run_mjs = zf.read("server/run.mjs").decode("utf-8")
        self.assertIn("package.json", run_mjs)
        self.assertIn("binaryOnPath", run_mjs)


if __name__ == "__main__":
    unittest.main()
