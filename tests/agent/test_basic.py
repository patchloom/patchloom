"""Basic agent scenarios: search, replace, read."""

from __future__ import annotations

import pytest

from conftest import run_scenario
from drivers.base import assert_patchloom_used, report_raw_tool_usage


@pytest.mark.timeout(180)
def test_search(agent, workspace, patchloom_shim):
    """Agent should use patchloom search to find lines matching a pattern."""
    # Setup: create files with TODO markers
    (workspace / "src").mkdir()
    (workspace / "src" / "main.py").write_text(
        "# TODO: implement auth\ndef main():\n    pass\n"
    )
    (workspace / "src" / "utils.py").write_text(
        "def helper():\n    # TODO: add logging\n    return 42\n"
    )
    (workspace / "README.md").write_text(
        "# Project\n\nA sample project.\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Find all lines containing 'TODO' across all source files in this "
        "project. Report which files and line numbers contain TODO comments.",
    )

    # Primary: patchloom search was used
    assert_patchloom_used(result, "search")

    # Secondary: output mentions both files
    response_text = (result.output_json or {}).get("text", result.stdout)
    assert "main.py" in response_text, "Agent should mention main.py"
    assert "utils.py" in response_text, "Agent should mention utils.py"

    raw = report_raw_tool_usage(result)
    if raw:
        print(f"WARNING: Agent also used raw tools: {raw}")


@pytest.mark.timeout(180)
def test_replace(agent, workspace, patchloom_shim):
    """Agent should use patchloom replace to rename a function across files."""
    (workspace / "src").mkdir()
    (workspace / "src" / "app.py").write_text(
        "def calculate_total(items):\n"
        "    return sum(i.price for i in items)\n"
        "\n"
        "result = calculate_total(order_items)\n"
    )
    (workspace / "src" / "test_app.py").write_text(
        "from app import calculate_total\n"
        "\n"
        "def test_it():\n"
        "    assert calculate_total([]) == 0\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Rename the function 'calculate_total' to 'compute_sum' in all "
        "files under src/. Make sure every reference is updated.",
    )

    # Primary: patchloom replace (or tx with replace op) was used
    assert_patchloom_used(result, "replace")

    # Secondary: files are updated
    app_content = (workspace / "src" / "app.py").read_text()
    test_content = (workspace / "src" / "test_app.py").read_text()
    assert "compute_sum" in app_content, "app.py should have new name"
    assert "calculate_total" not in app_content, "app.py should not have old name"
    assert "compute_sum" in test_content, "test_app.py should have new name"
    assert "calculate_total" not in test_content, "test_app.py should not have old name"


@pytest.mark.timeout(180)
def test_read(agent, workspace, patchloom_shim):
    """Agent reads a file. Native read_file is acceptable for standalone reads."""
    (workspace / "config.json").write_text(
        '{\n  "app_name": "MyApp",\n  "version": "1.0.0",\n  "debug": false\n}\n'
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Show me the contents of config.json. Report what you see.",
    )

    # No patchloom assertion: native read_file is fine for standalone reads.
    # patchloom read only adds value inside tx plans (pending-state reads).

    # Assert: agent saw the file content
    response_text = (result.output_json or {}).get("text", result.stdout)
    assert "MyApp" in response_text, "Agent should report the app_name value"


@pytest.mark.timeout(180)
def test_replace_regex(agent, workspace, patchloom_shim):
    """Agent should use patchloom replace --regex for pattern-based renames."""
    (workspace / "src").mkdir()
    (workspace / "src" / "utils.py").write_text(
        "def get_user_name():\n"
        "    pass\n"
        "\n"
        "def get_user_email():\n"
        "    pass\n"
        "\n"
        "def set_user_role():\n"
        "    pass\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "In src/utils.py, rename all functions that start with 'get_user_' "
        "to start with 'fetch_user_' instead. Use a regex replacement so you "
        "don't have to replace each one individually.",
    )

    # Primary: patchloom replace was used
    assert_patchloom_used(result, "replace")

    # Secondary: functions renamed
    content = (workspace / "src" / "utils.py").read_text()
    assert "fetch_user_name" in content, "get_user_name should become fetch_user_name"
    assert "fetch_user_email" in content, "get_user_email should become fetch_user_email"
    assert "set_user_role" in content, "set_user_role should NOT be renamed"
    assert "get_user_" not in content, "No get_user_ prefixes should remain"


@pytest.mark.timeout(180)
def test_replace_insert(agent, workspace, patchloom_shim):
    """Agent should use patchloom replace --insert-before to add a header."""
    (workspace / "src").mkdir()
    (workspace / "src" / "main.py").write_text(
        "import sys\n\ndef main():\n    print('Hello')\n"
    )
    (workspace / "src" / "utils.py").write_text(
        "import os\n\ndef helper():\n    return 42\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Add this copyright header as the very first line of every .py "
        "file under src/:\n"
        "# Copyright 2025 Example Corp. All rights reserved.\n"
        "Insert it before the existing first line, don't replace anything.",
    )

    # Primary: patchloom replace was used
    assert_patchloom_used(result, "replace")

    # Secondary: header added as first line
    for fname in ["main.py", "utils.py"]:
        content = (workspace / "src" / fname).read_text()
        assert content.startswith("# Copyright 2025 Example Corp"), (
            f"{fname} should start with copyright header"
        )
        # Original content preserved
        assert "import" in content, f"{fname} should still have imports"
