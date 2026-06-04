#!/usr/bin/env python3
"""Tests for scripts/fossa-filter.py."""

import json
import os
import subprocess
import sys
import tempfile

FILTER = os.path.join(os.path.dirname(__file__), "fossa-filter.py")


def run_filter(issues):
    """Run the filter on a list of FOSSA issues and return (exit_code, stdout)."""
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(issues, f)
        f.flush()
        result = subprocess.run(
            [sys.executable, FILTER, f.name],
            capture_output=True, text=True,
        )
    os.unlink(f.name)
    return result.returncode, result.stdout


def test_empty_issues():
    rc, out = run_filter([])
    assert rc == 0, f"Expected 0, got {rc}: {out}"
    assert "0 issue(s)" in out


def test_llvm_exception_filtered():
    issues = [
        {"revisionId": "cargo+cc$1.0.106", "license": "apache-2.0 WITH llvm-exception", "type": "policy_conflict"},
        {"revisionId": "cargo+compiler_builtins$0.1.139", "license": "apache-2.0 WITH llvm-exception", "type": "policy_conflict"},
    ]
    rc, out = run_filter(issues)
    assert rc == 0, f"Expected 0, got {rc}: {out}"
    assert "2 issue(s)" in out


def test_r_efi_filtered():
    issues = [
        {"revisionId": "cargo+r-efi$4.5.0", "license": "LGPL-2.1-or-later", "type": "policy_flag"},
    ]
    rc, out = run_filter(issues)
    assert rc == 0, f"Expected 0, got {rc}: {out}"


def test_unknown_crate_llvm_exception_not_filtered():
    """An unknown crate with llvm-exception license should NOT be filtered."""
    issues = [
        {"revisionId": "cargo+mystery-crate$0.1.0", "license": "apache-2.0 WITH llvm-exception", "type": "policy_conflict"},
    ]
    rc, out = run_filter(issues)
    assert rc == 1, f"Expected 1, got {rc}: {out}"
    assert "mystery-crate" in out


def test_genuine_issue_not_filtered():
    issues = [
        {"revisionId": "cargo+shady-crate$0.1.0", "license": "GPL-3.0", "type": "policy_conflict"},
    ]
    rc, out = run_filter(issues)
    assert rc == 1, f"Expected 1, got {rc}: {out}"
    assert "1 genuine" in out


def test_mixed_issues():
    issues = [
        {"revisionId": "cargo+cc$1.0.106", "license": "apache-2.0 WITH llvm-exception", "type": "policy_conflict"},
        {"revisionId": "cargo+r-efi$4.5.0", "license": "LGPL-2.1-or-later", "type": "policy_flag"},
        {"revisionId": "cargo+bad-crate$1.0.0", "license": "AGPL-3.0", "type": "policy_conflict"},
    ]
    rc, out = run_filter(issues)
    assert rc == 1, f"Expected 1, got {rc}: {out}"
    assert "1 genuine" in out
    assert "bad-crate" in out


def test_object_wrapper():
    """FOSSA may wrap issues in an object."""
    issues_obj = {"issues": [
        {"revisionId": "cargo+cc$1.0.106", "license": "apache-2.0 WITH llvm-exception", "type": "policy_conflict"},
    ]}
    with tempfile.NamedTemporaryFile(mode="w", suffix=".json", delete=False) as f:
        json.dump(issues_obj, f)
        f.flush()
        result = subprocess.run(
            [sys.executable, FILTER, f.name],
            capture_output=True, text=True,
        )
    os.unlink(f.name)
    assert result.returncode == 0


def test_case_insensitive_license():
    """FOSSA may use different casing for the license ID."""
    issues = [
        {"revisionId": "cargo+cc$1.0.106", "license": "Apache-2.0 WITH LLVM-exception", "type": "policy_conflict"},
    ]
    rc, out = run_filter(issues)
    assert rc == 0, f"Expected 0, got {rc}: {out}"


if __name__ == "__main__":
    tests = [f for f in dir() if f.startswith("test_")]
    passed = 0
    failed = 0
    for t in sorted(tests):
        try:
            globals()[t]()
            print(f"  PASS  {t}")
            passed += 1
        except AssertionError as e:
            print(f"  FAIL  {t}: {e}")
            failed += 1
    print(f"\n{passed} passed, {failed} failed")
    sys.exit(1 if failed else 0)
