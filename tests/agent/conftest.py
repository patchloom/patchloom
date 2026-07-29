"""Pytest configuration and shared fixtures for agent integration tests."""

from __future__ import annotations

import os
import shutil
import stat
import subprocess
from pathlib import Path

import pytest

from drivers import AgentResult, create_driver
from drivers.base import parse_shim_log

# ---------------------------------------------------------------------------
# pytest CLI options
# ---------------------------------------------------------------------------


def pytest_addoption(parser):
    parser.addoption(
        "--agent",
        default=os.environ.get("AGENT_TEST_AGENT", "grok"),
        help="Agent to test (default: grok)",
    )
    parser.addoption(
        "--model",
        default=os.environ.get("AGENT_TEST_MODEL", "grok-build"),
        help="Model to use (default: grok-build)",
    )
    parser.addoption(
        "--runs",
        type=int,
        default=int(os.environ.get("BENCH_RUNS", "1")),
        help="Number of benchmark runs per mode for variance reduction (default: 1)",
    )
    parser.addoption(
        "--dry-run-prompts",
        action="store_true",
        default=False,
        help="Print benchmark prompts without calling the LLM API",
    )


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

SHIM_TEMPLATE = (Path(__file__).parent / "shim.sh").read_text()


def _find_patchloom_binary() -> str:
    """Locate the patchloom binary: prefer cargo target, fall back to PATH."""
    # Check for cargo-built binary in the repo
    repo_root = Path(__file__).resolve().parent.parent.parent
    cargo_bin = repo_root / "target" / "debug" / "patchloom"
    if cargo_bin.exists():
        return str(cargo_bin)
    cargo_bin_release = repo_root / "target" / "release" / "patchloom"
    if cargo_bin_release.exists():
        return str(cargo_bin_release)
    # Fall back to PATH
    found = shutil.which("patchloom")
    if found:
        return found
    pytest.skip("patchloom binary not found; run 'cargo build' first")
    return ""  # unreachable; pytest.skip raises


@pytest.fixture
def patchloom_bin() -> str:
    """Return the path to the real patchloom binary."""
    return _find_patchloom_binary()


# Appended to workspace AGENTS.md when require_patchloom=True so project rules
# reinforce the harness (soft "prefer" language alone is not enough).
_HARNESS_CONTRACT = """

## Agent harness contract (this test)

For **all file mutations** in this task you MUST use the project-local patchloom
CLI at **`./bin/patchloom`** (or `bin/patchloom`) via the shell. Do not use a
global install (`which patchloom` may point at an old Homebrew binary).

Examples:

- JSON/YAML/TOML: `./bin/patchloom doc set PATH SELECTOR VALUE --apply`
- Markdown table/section/bullet: `./bin/patchloom md … --apply`
- Multi-file atomic: `./bin/patchloom tx plan.json --apply`

Optional once per session: `export PATH="$PWD/bin:$PATH"` then bare `patchloom`.

Do **not** use native file-edit tools (`search_replace`, whole-file rewrite) or
`sed`/`jq`/`yq` for these edits. The harness fails if `./bin/patchloom` is not
invoked, even if the file ends up correct.
"""


@pytest.fixture
def workspace(tmp_path, patchloom_bin):
    """Create an isolated workspace with AGENTS.md from patchloom agent-rules.

    The workspace is a git repo so agents discover AGENTS.md via the standard
    project-rules mechanism.
    """
    # Generate focused CLI/Linux AGENTS.md from patchloom agent-rules
    result = subprocess.run(
        [patchloom_bin, "agent-rules", "--mode", "cli", "--platform", "linux"],
        capture_output=True,
        text=True,
        check=True,
    )
    (tmp_path / "AGENTS.md").write_text(result.stdout)

    # Initialize a git repo so the agent discovers AGENTS.md
    subprocess.run(
        ["git", "init"],
        cwd=tmp_path,
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "add", "."],
        cwd=tmp_path,
        capture_output=True,
        check=True,
    )
    subprocess.run(
        ["git", "-c", "user.name=test", "-c", "user.email=test@test.com",
         "commit", "-m", "init"],
        cwd=tmp_path,
        capture_output=True,
        check=True,
    )
    return tmp_path


@pytest.fixture
def patchloom_shim(workspace, patchloom_bin):
    """Install workspace ``bin/patchloom`` logging shim; return (env, log_path).

    Grok ``run_terminal_cmd`` often ignores process PATH and runs a global
    Homebrew ``patchloom`` (stale version). A project-local ``./bin/patchloom``
    is what agents must invoke so the harness can log calls.
    """
    log_file = workspace / "patchloom_calls.jsonl"
    bin_dir = workspace / "bin"
    bin_dir.mkdir(exist_ok=True)
    shim_script = SHIM_TEMPLATE.replace("__REAL_PATH__", patchloom_bin)
    shim_script = shim_script.replace("__LOG_PATH__", str(log_file))
    shim_path = bin_dir / "patchloom"
    shim_path.write_text(shim_script)
    shim_path.chmod(shim_path.stat().st_mode | stat.S_IEXEC)

    env = {
        "PATH": f"{bin_dir}:{os.environ.get('PATH', '')}",
        "PATCHLOOM_SHIM_LOG": str(log_file),
        "PATCHLOOM": str(shim_path),
    }
    return env, log_file


def pytest_report_header(config):
    """Print agent and model metadata at the top of every test run."""
    agent_name = config.getoption("--agent")
    model = config.getoption("--model")
    driver = create_driver(agent_name, model)
    if not driver.is_available():
        return [f"agent: {agent_name} (NOT AVAILABLE)"]
    meta = driver.get_metadata()
    # Stash for later use by fixtures
    config._agent_metadata = meta
    return [
        f"agent: {meta.agent_name}  "
        f"model: {meta.model_name} ({meta.model_alias})  "
        f"cli: {meta.cli_version}",
    ]


@pytest.fixture
def agent(request):
    """Create the agent driver from CLI flags."""
    agent_name = request.config.getoption("--agent")
    model = request.config.getoption("--model")
    driver = create_driver(agent_name, model)
    if not driver.is_available():
        pytest.skip(f"Agent {agent_name!r} is not available on this system")
    return driver


def run_scenario(
    agent,
    workspace: Path,
    patchloom_shim: tuple,
    prompt: str,
    *,
    max_turns: int = 15,
    timeout_secs: int = 180,
    require_patchloom: bool = False,
) -> AgentResult:
    """Run a prompt through the agent with patchloom shim capture.

    When ``require_patchloom`` is True (tests that assert the CLI was used):
    - Prepend a hard requirement to the prompt
    - Append a harness contract to workspace ``AGENTS.md``
    - Disable native ``search_replace`` so the agent cannot pass by rewriting
      files without hitting the PATH shim (Grok ``--disallowed-tools``)
    """
    shim_env, log_path = patchloom_shim

    disallowed_tools = None
    extra_rules = None
    final_prompt = prompt
    if require_patchloom:
        # Grok shell often ignores PATH and runs /opt/homebrew/bin/patchloom.
        # Force the workspace logging shim by absolute relative path.
        final_prompt = (
            "HARD REQUIREMENT: perform every file mutation with "
            "`./bin/patchloom` (project-local CLI) via the shell "
            "(e.g. `./bin/patchloom doc set … --apply`, "
            "`./bin/patchloom md … --apply`, `./bin/patchloom tx … --apply`). "
            "Do not call a global `patchloom` from Homebrew/PATH. "
            "Do not use search_replace or other native file-edit tools.\n\n"
            + prompt
        )
        agents_md = workspace / "AGENTS.md"
        if agents_md.exists():
            body = agents_md.read_text()
            if "Agent harness contract" not in body:
                agents_md.write_text(body + _HARNESS_CONTRACT)
        # Denying search_replace forces shell for edits (when the CLI honors it).
        disallowed_tools = ["search_replace"]
        extra_rules = (
            "All file mutations MUST use ./bin/patchloom via the shell "
            "(not a global patchloom install). Native search_replace is forbidden."
        )

    result = agent.run_prompt(
        final_prompt,
        workspace,
        max_turns=max_turns,
        timeout_secs=timeout_secs,
        extra_env=shim_env,
        disallowed_tools=disallowed_tools,
        extra_rules=extra_rules,
    )
    # Re-parse shim log in case the driver didn't pick it up
    if not result.patchloom_calls:
        result.patchloom_calls = parse_shim_log(log_path)
    return result
