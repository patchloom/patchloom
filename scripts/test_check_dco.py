#!/usr/bin/env python3
"""Lock the DCO check: sign-off required, Claude co-author trailers rejected."""

from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check-dco.sh"
WORKFLOW = ROOT / ".github" / "workflows" / "dco.yml"


def git(repo: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo), *args],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def commit(repo: Path, message: str, author: str = "Tester") -> None:
    env = os.environ.copy()
    env["GIT_AUTHOR_NAME"] = author
    env["GIT_AUTHOR_EMAIL"] = "tester@example.com"
    env["GIT_COMMITTER_NAME"] = author
    env["GIT_COMMITTER_EMAIL"] = "tester@example.com"
    subprocess.run(
        ["git", "-C", str(repo), "commit", "--allow-empty", "-m", message],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )


def run_check(repo: Path, base: str) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["BASE_SHA"] = base
    return subprocess.run(
        ["bash", str(SCRIPT)],
        cwd=repo,
        capture_output=True,
        text=True,
        env=env,
    )


class CheckDcoTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = Path(self.tmp.name)
        git(self.repo, "init", "-b", "main")
        git(self.repo, "config", "user.name", "Tester")
        git(self.repo, "config", "user.email", "tester@example.com")
        commit(self.repo, "base\n\nSigned-off-by: Tester <tester@example.com>")
        self.base = git(self.repo, "rev-parse", "HEAD")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_signed_off_commit_passes(self) -> None:
        commit(self.repo, "ok\n\nSigned-off-by: Tester <tester@example.com>")
        result = run_check(self.repo, self.base)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)
        self.assertIn("All commits have DCO sign-off", result.stdout)

    def test_missing_signoff_fails(self) -> None:
        commit(self.repo, "no signoff")
        result = run_check(self.repo, self.base)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing Signed-off-by", result.stdout)

    def test_anthropic_coauthor_fails(self) -> None:
        commit(
            self.repo,
            "\n".join(
                [
                    "tidy",
                    "",
                    "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>",
                    "Signed-off-by: Tester <tester@example.com>",
                ]
            ),
        )
        result = run_check(self.repo, self.base)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Anthropic co-author", result.stdout)

    def test_claude_session_fails(self) -> None:
        commit(
            self.repo,
            "\n".join(
                [
                    "tidy",
                    "",
                    "Claude-Session: https://claude.ai/code/session_example",
                    "Signed-off-by: Tester <tester@example.com>",
                ]
            ),
        )
        result = run_check(self.repo, self.base)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Claude-Session", result.stdout)

    def test_bot_commit_is_skipped(self) -> None:
        commit(
            self.repo,
            "bot\n\nCo-authored-by: Claude <noreply@anthropic.com>",
            author="dependabot[bot]",
        )
        result = run_check(self.repo, self.base)
        self.assertEqual(result.returncode, 0, result.stderr + result.stdout)

    def test_workflow_calls_the_script(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("bash scripts/check-dco.sh", text)
        self.assertIn("name: DCO Sign-off", text)


if __name__ == "__main__":
    unittest.main()
