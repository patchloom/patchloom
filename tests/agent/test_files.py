"""File lifecycle agent scenarios: create, delete, status, patch."""

from __future__ import annotations

import subprocess

import pytest

from conftest import run_scenario
from drivers.base import assert_patchloom_used


@pytest.mark.timeout(180)
def test_create(agent, workspace, patchloom_shim):
    """Agent should use patchloom create to make a new file."""
    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Create a new file called LICENSE containing the text:\n"
        "MIT License\n\n"
        "Copyright (c) 2025 Example Corp\n\n"
        "Permission is hereby granted, free of charge, to any person.",
    )

    # Primary: patchloom create was used
    assert_patchloom_used(result, "create")

    # Secondary: file exists with correct content
    license_file = workspace / "LICENSE"
    assert license_file.exists(), "LICENSE should exist"
    content = license_file.read_text()
    assert "MIT License" in content, "LICENSE should contain MIT License"
    assert "Example Corp" in content, "LICENSE should contain Example Corp"


@pytest.mark.timeout(180)
def test_delete(agent, workspace, patchloom_shim):
    """Agent should use patchloom delete to remove a file."""
    (workspace / "config.old.json").write_text('{"deprecated": true}\n')
    (workspace / "config.json").write_text('{"active": true}\n')

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Delete the deprecated file config.old.json. "
        "Do not touch config.json.",
    )

    # Primary: patchloom delete was used
    assert_patchloom_used(result, "delete")

    # Secondary: correct file removed, other preserved
    assert not (workspace / "config.old.json").exists(), "config.old.json should be deleted"
    assert (workspace / "config.json").exists(), "config.json should still exist"


@pytest.mark.timeout(180)
def test_status(agent, workspace, patchloom_shim):
    """Agent checks uncommitted changes. Native git status is acceptable."""
    # Create a tracked file, commit it, then modify it
    tracked = workspace / "tracked.txt"
    tracked.write_text("original\n")
    subprocess.run(
        ["git", "add", "tracked.txt"],
        cwd=workspace, capture_output=True, check=True,
    )
    subprocess.run(
        ["git", "-c", "user.name=test", "-c", "user.email=test@test.com",
         "commit", "-m", "add tracked"],
        cwd=workspace, capture_output=True, check=True,
    )
    # Now modify it so it shows as changed
    tracked.write_text("modified\n")
    # Also create an untracked file
    (workspace / "untracked.txt").write_text("new\n")

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Which files have uncommitted changes in this project? "
        "Report the filenames and whether they are modified or new.",
    )

    # No patchloom assertion: patchloom status is a thin wrapper around
    # git status with no unique value. Native git status is fine.

    # Assert: agent found the changed files
    response_text = (result.output_json or {}).get("text", result.stdout)
    assert "tracked.txt" in response_text, "Agent should mention tracked.txt"


@pytest.mark.timeout(240)
def test_patch(agent, workspace, patchloom_shim):
    """Agent should use patchloom patch to apply a unified diff."""
    (workspace / "greeting.py").write_text(
        "def greet(name):\n"
        "    return 'Hello, ' + name\n"
        "\n"
        "print(greet('World'))\n"
    )

    diff_text = (
        "--- a/greeting.py\n"
        "+++ b/greeting.py\n"
        "@@ -1,4 +1,5 @@\n"
        " def greet(name):\n"
        "-    return 'Hello, ' + name\n"
        "+    greeting = f'Hello, {name}!'\n"
        "+    return greeting\n"
        " \n"
        " print(greet('World'))\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        f"Apply this unified diff to greeting.py:\n\n```diff\n{diff_text}```",
        max_turns=20,
    )

    # Primary: patchloom patch was used
    assert_patchloom_used(result, "patch")

    # Secondary: file was patched correctly
    content = (workspace / "greeting.py").read_text()
    assert "greeting = f'Hello, {name}!'" in content, "Patch should be applied"
    assert "return greeting" in content, "New return line should be present"
