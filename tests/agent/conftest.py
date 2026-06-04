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


@pytest.fixture
def patchloom_shim(tmp_path, patchloom_bin):
    """Set up a patchloom shim that logs invocations, return (env_dict, log_path)."""
    shim_dir = tmp_path / "shim_bin"
    shim_dir.mkdir()
    log_file = tmp_path / "patchloom_calls.jsonl"

    shim_script = SHIM_TEMPLATE.replace("__REAL_PATH__", patchloom_bin)
    shim_script = shim_script.replace("__LOG_PATH__", str(log_file))
    shim_path = shim_dir / "patchloom"
    shim_path.write_text(shim_script)
    shim_path.chmod(shim_path.stat().st_mode | stat.S_IEXEC)

    env = {
        "PATH": f"{shim_dir}:{os.environ.get('PATH', '')}",
        "PATCHLOOM_SHIM_LOG": str(log_file),
    }
    return env, log_file


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
) -> AgentResult:
    """Run a prompt through the agent with patchloom shim capture."""
    shim_env, log_path = patchloom_shim

    result = agent.run_prompt(
        prompt,
        workspace,
        max_turns=max_turns,
        timeout_secs=timeout_secs,
        extra_env=shim_env,
    )
    # Re-parse shim log in case the driver didn't pick it up
    if not result.patchloom_calls:
        result.patchloom_calls = parse_shim_log(log_path)
    return result
