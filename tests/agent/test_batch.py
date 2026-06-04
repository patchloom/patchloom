"""Batch and transaction agent scenarios."""

from __future__ import annotations

import json

import pytest

from conftest import run_scenario
from drivers.base import assert_patchloom_used


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

    _result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Update the version from '1.0.0' to '2.0.0' in all config files "
        "(package.json, config.yaml, pyproject.toml).",
    )

    # No patchloom assertion: native tools are fine for multi-file string replace.

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
def test_tidy(agent, workspace, patchloom_shim):
    """Agent should use patchloom tidy to fix trailing whitespace."""
    (workspace / "dirty.txt").write_text("line one   \nline two\t\nline three  \n")
    (workspace / "clean.txt").write_text("already clean\n")

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Fix trailing whitespace in all text files in this project.",
    )

    # Primary: patchloom tidy was used
    assert_patchloom_used(result, "tidy")

    # Secondary: trailing whitespace removed
    content = (workspace / "dirty.txt").read_text()
    for line in content.splitlines():
        assert line == line.rstrip(), f"Line still has trailing whitespace: {line!r}"


@pytest.mark.timeout(240)
def test_tx_read_then_edit(agent, workspace, patchloom_shim):
    """Agent should use a patchloom tx plan with a read op to inspect then edit.

    This tests the unique value of patchloom read inside a transaction:
    the read operation sees pending in-plan state, enabling
    inspect-then-edit workflows in a single atomic call.
    """
    (workspace / "package.json").write_text(
        json.dumps(
            {"name": "myapp", "version": "1.0.0", "description": "A sample app"},
            indent=2,
        )
        + "\n"
    )
    (workspace / "README.md").write_text(
        "# myapp\n\nVersion: 1.0.0\n\nA sample app.\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "I need you to do the following atomically using a single patchloom "
        "transaction plan:\n"
        "1. Read package.json to discover the current version\n"
        "2. Update the version in package.json from whatever it is to '2.0.0'\n"
        "3. Update the version reference in README.md to match '2.0.0'\n"
        "Use patchloom tx with a read operation followed by replace operations "
        "so everything happens in one atomic plan.",
        max_turns=20,
    )

    # Primary: patchloom tx was used (the tx plan should include a read op)
    assert_patchloom_used(result, "tx")

    # Secondary: both files updated
    pkg = json.loads((workspace / "package.json").read_text())
    assert pkg["version"] == "2.0.0", "package.json version should be 2.0.0"

    readme = (workspace / "README.md").read_text()
    assert "2.0.0" in readme, "README.md should contain 2.0.0"
    assert "1.0.0" not in readme, "README.md should not contain old version"


@pytest.mark.timeout(240)
def test_tx_format_validate(agent, workspace, patchloom_shim):
    """Agent should use patchloom tx with format and validate lifecycle steps."""
    (workspace / "src").mkdir()
    (workspace / "src" / "config.json").write_text(
        json.dumps({"version": "1.0.0", "env": "dev"}, indent=2) + "\n"
    )
    (workspace / "src" / "settings.json").write_text(
        json.dumps({"version": "1.0.0", "debug": True}, indent=2) + "\n"
    )
    # Create a simple validation script
    (workspace / "check.sh").write_text(
        '#!/bin/bash\n'
        'grep -q "2.0.0" src/config.json && grep -q "2.0.0" src/settings.json\n'
    )
    import stat
    (workspace / "check.sh").chmod(
        (workspace / "check.sh").stat().st_mode | stat.S_IEXEC
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Update the version from '1.0.0' to '2.0.0' in both "
        "src/config.json and src/settings.json using a patchloom "
        "transaction plan. Include a validate step that runs "
        "'./check.sh' to verify both files were updated correctly.",
        max_turns=20,
    )

    # Primary: patchloom tx was used
    assert_patchloom_used(result, "tx")

    # Secondary: both files updated
    config = json.loads((workspace / "src" / "config.json").read_text())
    assert config["version"] == "2.0.0", "config.json version should be 2.0.0"

    settings = json.loads((workspace / "src" / "settings.json").read_text())
    assert settings["version"] == "2.0.0", "settings.json version should be 2.0.0"
