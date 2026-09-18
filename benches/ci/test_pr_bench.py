#!/usr/bin/env python3
"""Unit tests for PR-gate bench helpers (#2552)."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


class CheckThresholdTests(unittest.TestCase):
    def _write(self, mean: float) -> Path:
        path = Path(tempfile.mkdtemp()) / "bench-search.json"
        path.write_text(
            json.dumps({"results": [{"mean": mean}]}),
            encoding="utf-8",
        )
        return path

    def test_under_threshold_exits_0(self) -> None:
        path = self._write(0.01)
        proc = subprocess.run(
            [sys.executable, str(HERE / "check_threshold.py"), str(path), "search", "0.05"],
            check=False,
        )
        self.assertEqual(proc.returncode, 0)

    def test_over_threshold_exits_1(self) -> None:
        path = self._write(0.2)
        proc = subprocess.run(
            [sys.executable, str(HERE / "check_threshold.py"), str(path), "search", "0.05"],
            check=False,
        )
        self.assertEqual(proc.returncode, 1)


class MakePrFixtureTests(unittest.TestCase):
    def test_writes_text_rust_and_json(self) -> None:
        dest = Path(tempfile.mkdtemp()) / "corpus"
        subprocess.run(
            [sys.executable, str(HERE / "make_pr_fixture.py"), str(dest)],
            check=True,
        )
        txts = list((dest / "txt").glob("*.txt"))
        rusts = list((dest / "src").glob("*.rs"))
        self.assertGreaterEqual(len(txts), 100)
        self.assertGreaterEqual(len(rusts), 20)
        total = sum(p.stat().st_size for p in txts)
        self.assertGreater(total, 1_000_000, f"text corpus too small: {total}")
        pkg = (dest / "pkg.json").read_text(encoding="utf-8")
        self.assertIn("bench", pkg)
        self.assertIn("fn foo()", (dest / "src" / "lib_000.rs").read_text(encoding="utf-8"))


class CiYmlAstRenameTests(unittest.TestCase):
    def setUp(self) -> None:
        root = HERE.parents[1]
        self.ci = (root / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")

    def test_uses_path_first_old_new(self) -> None:
        self.assertIn(
            "ast rename $bench_src --old foo --new foo2 --check",
            self.ci,
        )
        self.assertNotIn(
            "ast rename foo --new foo2 $bench_src --check",
            self.ci,
        )

    def test_preflights_before_ignore_exit_status(self) -> None:
        start = self.ci.index("Benchmark ast rename dry-run")
        end = self.ci.index("Benchmark summary", start)
        block = self.ci[start:end]
        self.assertIn("preflight_expected_exit.sh", block)
        self.assertLess(
            block.index("preflight_expected_exit.sh"),
            block.index("hyperfine"),
        )


if __name__ == "__main__":
    unittest.main()

