"""Structured editing agent scenarios: doc and md commands."""

from __future__ import annotations

import json

import pytest

from conftest import run_scenario
from drivers.base import assert_patchloom_used, report_raw_tool_usage


@pytest.mark.timeout(180)
def test_doc_set(agent, workspace, patchloom_shim):
    """Agent should use patchloom doc to set a key in a JSON file."""
    (workspace / "config.json").write_text(
        json.dumps(
            {"app_name": "MyApp", "debug": False, "log_level": "info"},
            indent=2,
        )
        + "\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Set the 'debug' key to true in config.json.",
    )

    # Primary: patchloom doc was used
    assert_patchloom_used(result, "doc")

    # Secondary: config.json updated
    config = json.loads((workspace / "config.json").read_text())
    assert config["debug"] is True, "debug should be true"
    # Other keys should be preserved
    assert config["app_name"] == "MyApp", "app_name should be preserved"
    assert config["log_level"] == "info", "log_level should be preserved"


@pytest.mark.timeout(180)
def test_md_table(agent, workspace, patchloom_shim):
    """Agent should use patchloom md to append a row to a markdown table."""
    (workspace / "README.md").write_text(
        "# My Project\n"
        "\n"
        "A sample project.\n"
        "\n"
        "## Commands\n"
        "\n"
        "| Command | Description |\n"
        "|---------|-------------|\n"
        "| `build` | Build the project |\n"
        "| `test` | Run tests |\n"
        "\n"
        "## License\n"
        "\n"
        "MIT\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Add a new row to the Commands table in README.md: "
        "command is `lint` and description is `Run the linter`.",
    )

    # Primary: patchloom md was used
    assert_patchloom_used(result, "md")

    # Secondary: table has the new row
    content = (workspace / "README.md").read_text()
    assert "lint" in content, "README should contain 'lint'"
    assert "Run the linter" in content, "README should contain 'Run the linter'"
    # Existing rows preserved
    assert "build" in content, "Existing 'build' row should be preserved"
    assert "test" in content, "Existing 'test' row should be preserved"
