"""Batch and transaction agent scenarios."""

from __future__ import annotations

import json

import pytest

from conftest import run_scenario
from drivers.base import (
    assert_patchloom_used,
    assert_patchloom_used_any,
    report_raw_tool_usage,
)


@pytest.mark.timeout(180)
def test_batch_replace(agent, workspace, patchloom_shim):
    """Agent should use patchloom to update a version string across multiple config files."""
    (workspace / "package.json").write_text(
        json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n"
    )
    (workspace / "config.yaml").write_text(
        "app:\n  name: myapp\n  version: '1.0.0'\n"
    )
    (workspace / "pyproject.toml").write_text(
        '[project]\nname = "myapp"\nversion = "1.0.0"\n'
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Update the version from '1.0.0' to '2.0.0' in all config files "
        "(package.json, config.yaml, pyproject.toml).",
    )

    # Primary: patchloom was used (replace, tx, or doc)
    assert_patchloom_used_any(result, ["replace", "tx", "doc"])

    # Secondary: all files updated
    pkg = json.loads((workspace / "package.json").read_text())
    assert pkg["version"] == "2.0.0", "package.json version should be 2.0.0"

    yaml_content = (workspace / "config.yaml").read_text()
    assert "2.0.0" in yaml_content, "config.yaml should contain 2.0.0"
    assert "1.0.0" not in yaml_content, "config.yaml should not contain 1.0.0"

    toml_content = (workspace / "pyproject.toml").read_text()
    assert "2.0.0" in toml_content, "pyproject.toml should contain 2.0.0"


@pytest.mark.timeout(240)
def test_tx_multi_file(agent, workspace, patchloom_shim):
    """Agent should use patchloom tx for atomic multi-file operations."""
    (workspace / "package.json").write_text(
        json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Do both of these atomically using a patchloom transaction:\n"
        "1. Create a new file called hello.txt containing 'Hello, World!'\n"
        "2. Update the version in package.json from '1.0.0' to '3.0.0'\n"
        "Use a single patchloom tx plan so both changes happen atomically.",
        max_turns=20,
    )

    # Primary: patchloom tx was used
    assert_patchloom_used(result, "tx")

    # Secondary: both changes applied
    hello = workspace / "hello.txt"
    assert hello.exists(), "hello.txt should exist"
    assert "Hello" in hello.read_text(), "hello.txt should contain Hello"

    pkg = json.loads((workspace / "package.json").read_text())
    assert pkg["version"] == "3.0.0", "package.json version should be 3.0.0"


@pytest.mark.timeout(180)
def test_hygiene(agent, workspace, patchloom_shim):
    """Agent should use patchloom hygiene to fix trailing whitespace."""
    (workspace / "dirty.txt").write_text("line one   \nline two\t\nline three  \n")
    (workspace / "clean.txt").write_text("already clean\n")

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Fix trailing whitespace in all text files in this project.",
    )

    # Primary: patchloom hygiene was used
    assert_patchloom_used(result, "hygiene")

    # Secondary: trailing whitespace removed
    content = (workspace / "dirty.txt").read_text()
    for line in content.splitlines():
        assert line == line.rstrip(), f"Line still has trailing whitespace: {line!r}"
