#!/usr/bin/env python3
"""Lock release-event cli-bench ast rename argv and corpus fixture (#2566)."""

from __future__ import annotations

import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
RUN_SH = HERE / "run.sh"
PREFLIGHT = HERE / "preflight_expected_exit.sh"
GEN = HERE / "generate_corpus.py"
README = HERE.parent / "README.md"


def _load_generate_corpus():
    spec = importlib.util.spec_from_file_location("generate_corpus", GEN)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class RunShAstRenameTests(unittest.TestCase):
    def setUp(self) -> None:
        self.src = RUN_SH.read_text(encoding="utf-8")

    def test_uses_path_first_old_new(self) -> None:
        gen = _load_generate_corpus()
        ident = gen.AST_RENAME_IDENT
        self.assertIn(f'AST_RENAME_OLD="{ident}"', self.src)
        self.assertIn('ast rename "$DIR" --old "$AST_RENAME_OLD" --new', self.src)
        self.assertNotIn("ast rename foo --new bar $DIR", self.src)

    def test_preflights_before_hyperfine(self) -> None:
        rename_at = self.src.index("AST rename (directory, check)")
        preflight_at = self.src.index("preflight_expected_exit.sh", rename_at)
        hyperfine_at = self.src.index("hyperfine --warmup", rename_at)
        self.assertLess(preflight_at, hyperfine_at)

    def test_does_not_swallow_rename_hyperfine_stderr(self) -> None:
        start = self.src.index("AST rename (directory, check)")
        end = self.src.index("Doc set (JSON key)", start)
        block = self.src[start:end]
        self.assertNotIn("2>/dev/null", block)


class GenerateCorpusAstRenameTests(unittest.TestCase):
    def test_every_scale_contains_stable_ident(self) -> None:
        gen = _load_generate_corpus()
        dest = Path(tempfile.mkdtemp()) / "corpus"
        gen.generate_corpus(dest, "small")
        pinned = dest / "small" / "src" / "bench_rename_target.rs"
        self.assertTrue(pinned.is_file(), f"missing {pinned}")
        text = pinned.read_text(encoding="utf-8")
        self.assertIn(f"fn {gen.AST_RENAME_IDENT}", text)
        self.assertGreaterEqual(text.count(gen.AST_RENAME_IDENT), 2)


class PreflightExpectedExitTests(unittest.TestCase):
    def test_accepts_listed_nonzero(self) -> None:
        proc = subprocess.run(
            ["bash", str(PREFLIGHT), "0,2", "--", "bash", "-c", "exit 2"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_rejects_clap_style_exit_1_with_visible_stderr(self) -> None:
        proc = subprocess.run(
            [
                "bash",
                str(PREFLIGHT),
                "0,2",
                "--",
                "bash",
                "-c",
                "echo 'error: unexpected argument' >&2; exit 1",
            ],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 1)
        self.assertIn("error: unexpected argument", proc.stderr)
        self.assertIn("unexpected exit 1", proc.stderr)


class ReadmeAstRenameTests(unittest.TestCase):
    def test_documents_live_cli(self) -> None:
        text = README.read_text(encoding="utf-8")
        self.assertIn("ast rename PATH --old", text)
        self.assertNotRegex(text, r"`patchloom ast rename --check`")


if __name__ == "__main__":
    unittest.main()
