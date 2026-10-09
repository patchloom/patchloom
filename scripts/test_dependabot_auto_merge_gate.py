#!/usr/bin/env python3
"""Lock Dependabot auto-merge: commit-pin moves are not semver majors."""

from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "dependabot_auto_merge_gate.py"
WORKFLOW = ROOT / ".github" / "workflows" / "dependabot-auto-merge.yml"

MAJOR = "version-update:semver-major"
MINOR = "version-update:semver-minor"
PATCH = "version-update:semver-patch"
OLD_PIN = "4ffc7888bffd451b357355dc214d43bb9f23917e"
NEW_PIN = "b7370d4d152efdce803a26cc6ca9eee58cb2ad54"


def load_gate():
    spec = importlib.util.spec_from_file_location("dependabot_auto_merge_gate", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class AutoMergeGateTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.gate = load_gate()

    def test_patch_and_minor_merge(self) -> None:
        deps = [
            {"updateType": MINOR, "prevVersion": "2.21.1", "newVersion": "2.22.0"},
            {"updateType": PATCH, "prevVersion": "1.4.3", "newVersion": "1.4.4"},
        ]
        self.assertTrue(self.gate.should_auto_merge(MINOR, deps, "2.21.1"))

    def test_grouped_commit_pin_is_not_a_major(self) -> None:
        # fetch-metadata calculateUpdateType compares SHA strings before
        # the first dot, so a pin move is labeled semver-major.
        deps = [
            {"updateType": MINOR, "prevVersion": "2.21.1", "newVersion": "2.22.0"},
            {"updateType": PATCH, "prevVersion": "2.87.22", "newVersion": "2.87.26"},
            {"updateType": PATCH, "prevVersion": "1.4.3", "newVersion": "1.4.4"},
            {
                "updateType": MAJOR,
                "prevVersion": OLD_PIN,
                "newVersion": NEW_PIN,
            },
        ]
        self.assertTrue(self.gate.should_auto_merge(MAJOR, deps, "2.21.1"))

    def test_real_major_stays_manual(self) -> None:
        deps = [
            {"updateType": MAJOR, "prevVersion": "1.4.3", "newVersion": "2.0.0"},
        ]
        self.assertFalse(self.gate.should_auto_merge(MAJOR, deps, "1.4.3"))

    def test_real_major_next_to_a_pin_stays_manual(self) -> None:
        deps = [
            {"updateType": MAJOR, "prevVersion": "1.4.3", "newVersion": "2.0.0"},
            {"updateType": MAJOR, "prevVersion": OLD_PIN, "newVersion": NEW_PIN},
        ]
        self.assertFalse(self.gate.should_auto_merge(MAJOR, deps, "1.4.3"))

    def test_sha_only_group_merges(self) -> None:
        deps = [
            {"updateType": MAJOR, "prevVersion": OLD_PIN, "newVersion": NEW_PIN},
        ]
        self.assertTrue(self.gate.should_auto_merge(MAJOR, deps, OLD_PIN))

    def test_empty_list_falls_back_to_previous_version(self) -> None:
        self.assertTrue(self.gate.should_auto_merge(MAJOR, [], OLD_PIN))
        self.assertFalse(self.gate.should_auto_merge(MAJOR, [], "1.2.3"))

    def test_missing_versions_on_a_major_stay_manual(self) -> None:
        deps = [{"updateType": MAJOR, "prevVersion": "", "newVersion": ""}]
        self.assertFalse(self.gate.should_auto_merge(MAJOR, deps, ""))

    def test_invalid_json_on_a_major_stays_manual(self) -> None:
        self.assertFalse(self.gate.decide(MAJOR, "{", "1.2.3"))
        self.assertTrue(self.gate.decide(PATCH, "{", "1.4.3"))

    def test_workflow_uses_the_gate(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("scripts/dependabot_auto_merge_gate.py", text)
        self.assertIn("steps.gate.outputs.auto_merge == 'true'", text)
        self.assertIn("github.event.repository.default_branch", text)
        self.assertIn("persist-credentials: false", text)
        self.assertNotIn(
            "steps.metadata.outputs.update-type != 'version-update:semver-major'",
            text,
        )


if __name__ == "__main__":
    unittest.main()
