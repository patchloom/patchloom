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


@pytest.mark.timeout(180)
def test_doc_merge(agent, workspace, patchloom_shim):
    """Agent should use patchloom doc merge to add a new section to a config."""
    (workspace / "config.yaml").write_text(
        "app:\n"
        "  name: myapp\n"
        "  port: 8080\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Add a 'database' section to config.yaml with these values: "
        "host is 'localhost', port is 5432, and name is 'mydb'. "
        "Keep the existing 'app' section intact.",
    )

    # Primary: patchloom doc was used
    assert_patchloom_used(result, "doc")

    # Secondary: database section added, app preserved
    content = (workspace / "config.yaml").read_text()
    assert "database" in content, "config.yaml should have database section"
    assert "localhost" in content, "database host should be localhost"
    assert "5432" in content, "database port should be 5432"
    assert "mydb" in content, "database name should be mydb"
    assert "myapp" in content, "app name should be preserved"
    assert "8080" in content, "app port should be preserved"


@pytest.mark.timeout(180)
def test_md_replace_section(agent, workspace, patchloom_shim):
    """Agent should use patchloom md replace-section to rewrite a section."""
    (workspace / "README.md").write_text(
        "# My Project\n"
        "\n"
        "A cool project.\n"
        "\n"
        "## Installation\n"
        "\n"
        "Run `make install` to install.\n"
        "\n"
        "## Usage\n"
        "\n"
        "Run `./app` to start.\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Replace the Installation section in README.md with these new "
        "instructions:\n\n"
        "```\n"
        "pip install myproject\n"
        "```\n\n"
        "Keep all other sections unchanged.",
    )

    # Primary: patchloom md was used
    assert_patchloom_used(result, "md")

    # Secondary: Installation section replaced
    content = (workspace / "README.md").read_text()
    assert "pip install myproject" in content, "New installation instructions should be present"
    assert "make install" not in content, "Old installation instructions should be gone"
    # Other sections preserved
    assert "## Usage" in content, "Usage section should be preserved"
    assert "./app" in content, "Usage content should be preserved"


@pytest.mark.timeout(180)
def test_md_upsert_bullet(agent, workspace, patchloom_shim):
    """Agent should use patchloom md upsert-bullet to add a rule idempotently."""
    (workspace / "CONTRIBUTING.md").write_text(
        "# Contributing\n"
        "\n"
        "## Coding Standards\n"
        "\n"
        "- Use 4-space indentation\n"
        "- Write docstrings for all public functions\n"
        "\n"
        "## Testing\n"
        "\n"
        "Run `make test` before submitting.\n"
    )

    result = run_scenario(
        agent,
        workspace,
        patchloom_shim,
        "Add the following rule to the Coding Standards section in "
        "CONTRIBUTING.md if it is not already there: "
        "'All functions must have type annotations'. "
        "Do not duplicate it if it already exists.",
    )

    # Primary: patchloom md was used
    assert_patchloom_used(result, "md")

    # Secondary: bullet added
    content = (workspace / "CONTRIBUTING.md").read_text()
    assert "type annotations" in content, "New rule should be present"
    # Existing bullets preserved
    assert "4-space indentation" in content, "Existing rule should be preserved"
    assert "docstrings" in content, "Existing rule should be preserved"
    # Other sections preserved
    assert "## Testing" in content, "Testing section should be preserved"
