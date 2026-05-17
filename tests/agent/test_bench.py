"""A/B agent benchmarks: patchloom instructions vs native tools.

Runs each scenario twice (with and without AGENTS.md containing patchloom
instructions) and prints a comparison table of duration, tool calls, and
success rate. No hard assertions on timing; this is a diagnostic tool.

Run with:
    make bench-agent
    make bench-agent MODEL=sxs-gpt-5-4
"""

from __future__ import annotations

import json
import subprocess
import os
import stat
from dataclasses import dataclass
from pathlib import Path

import pytest

from drivers import create_driver
from drivers.base import (
    AgentResult,
    _extract_subcommand,
    parse_shim_log,
    report_raw_tool_usage,
)

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

SHIM_TEMPLATE = (Path(__file__).parent / "shim.sh").read_text()


def _find_patchloom_binary() -> str:
    repo_root = Path(__file__).resolve().parent.parent.parent
    for subdir in ["release", "debug"]:
        p = repo_root / "target" / subdir / "patchloom"
        if p.exists():
            return str(p)
    import shutil
    found = shutil.which("patchloom")
    if found:
        return found
    pytest.skip("patchloom binary not found")


def _make_shim(tmp_path: Path, real_bin: str):
    shim_dir = tmp_path / "shim_bin"
    shim_dir.mkdir()
    log_file = tmp_path / "patchloom_calls.jsonl"
    shim_script = SHIM_TEMPLATE.replace("__REAL_PATH__", real_bin)
    shim_script = shim_script.replace("__LOG_PATH__", str(log_file))
    shim_path = shim_dir / "patchloom"
    shim_path.write_text(shim_script)
    shim_path.chmod(shim_path.stat().st_mode | stat.S_IEXEC)
    env = {
        "PATH": f"{shim_dir}:{os.environ.get('PATH', '')}",
        "PATCHLOOM_SHIM_LOG": str(log_file),
    }
    return env, log_file


def _make_workspace(tmp_path: Path, real_bin: str, with_agents_md: bool):
    ws = tmp_path / "workspace"
    ws.mkdir()
    if with_agents_md:
        result = subprocess.run(
            [real_bin, "agent-rules"],
            capture_output=True, text=True, check=True,
        )
        (ws / "AGENTS.md").write_text(result.stdout)
    subprocess.run(["git", "init"], cwd=ws, capture_output=True, check=True)
    subprocess.run(["git", "add", "."], cwd=ws, capture_output=True, check=True)
    subprocess.run(
        ["git", "-c", "user.name=test", "-c", "user.email=test@test.com",
         "commit", "--allow-empty", "-m", "init"],
        cwd=ws, capture_output=True, check=True,
    )
    return ws


# ---------------------------------------------------------------------------
# Benchmark result collection
# ---------------------------------------------------------------------------

@dataclass
class BenchRow:
    scenario: str
    mode: str  # "patchloom" or "native"
    duration_secs: float
    patchloom_call_count: int
    patchloom_cmds: list[str]
    raw_tools: list[str]
    success: bool
    patchloom_time_ms: int  # total ms spent in patchloom calls


_bench_results: list[BenchRow] = []


def _run_bench(agent, real_bin, tmp_path, with_agents_md, prompt, setup_fn, check_fn, scenario_name):
    """Run one benchmark scenario and collect metrics."""
    ws = _make_workspace(tmp_path, real_bin, with_agents_md)
    shim_env, log_path = _make_shim(tmp_path, real_bin)
    setup_fn(ws)

    result = agent.run_prompt(
        prompt, ws, max_turns=15, timeout_secs=240, extra_env=shim_env,
    )
    if not result.patchloom_calls:
        result.patchloom_calls = parse_shim_log(log_path)

    success = True
    try:
        check_fn(ws, result)
    except (AssertionError, Exception):
        success = False

    cmds = [_extract_subcommand(c["args"]) for c in result.patchloom_calls if c.get("args")]
    cmds = [c for c in cmds if c]
    total_pl_ms = sum(c.get("duration_ms", 0) for c in result.patchloom_calls if c.get("duration_ms", 0) > 0)

    row = BenchRow(
        scenario=scenario_name,
        mode="patchloom" if with_agents_md else "native",
        duration_secs=round(result.duration_secs, 1),
        patchloom_call_count=len(result.patchloom_calls),
        patchloom_cmds=cmds,
        raw_tools=report_raw_tool_usage(result),
        success=success,
        patchloom_time_ms=total_pl_ms,
    )
    _bench_results.append(row)
    return row


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------

def _setup_search(ws):
    (ws / "src").mkdir(exist_ok=True)
    (ws / "src" / "main.py").write_text("# TODO: implement auth\ndef main():\n    pass\n")
    (ws / "src" / "utils.py").write_text("def helper():\n    # TODO: add logging\n    return 42\n")

def _check_search(ws, result):
    text = (result.output_json or {}).get("text", result.stdout)
    assert "main.py" in text

def _setup_replace(ws):
    (ws / "src").mkdir(exist_ok=True)
    (ws / "src" / "app.py").write_text("def calculate_total(items):\n    return sum(i.price for i in items)\nresult = calculate_total(order_items)\n")
    (ws / "src" / "test_app.py").write_text("from app import calculate_total\ndef test_it():\n    assert calculate_total([]) == 0\n")

def _check_replace(ws, result):
    content = (ws / "src" / "app.py").read_text()
    assert "compute_sum" in content
    assert "calculate_total" not in content

def _setup_batch(ws):
    (ws / "package.json").write_text(json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n")
    (ws / "config.yaml").write_text("app:\n  name: myapp\n  version: '1.0.0'\n")

def _check_batch(ws, result):
    pkg = json.loads((ws / "package.json").read_text())
    assert pkg["version"] == "2.0.0"
    assert "2.0.0" in (ws / "config.yaml").read_text()

def _setup_doc_set(ws):
    (ws / "config.json").write_text(json.dumps({"app_name": "MyApp", "debug": False}, indent=2) + "\n")

def _check_doc_set(ws, result):
    config = json.loads((ws / "config.json").read_text())
    assert config["debug"] is True

def _setup_tx(ws):
    (ws / "package.json").write_text(json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n")

def _check_tx(ws, result):
    assert (ws / "hello.txt").exists()
    pkg = json.loads((ws / "package.json").read_text())
    assert pkg["version"] == "3.0.0"


SCENARIOS = [
    ("search", "Find all lines containing 'TODO' across all source files. Report which files and line numbers.",
     _setup_search, _check_search),
    ("replace", "Rename the function 'calculate_total' to 'compute_sum' in all files under src/.",
     _setup_replace, _check_replace),
    ("batch_replace", "Update the version from '1.0.0' to '2.0.0' in all config files (package.json, config.yaml).",
     _setup_batch, _check_batch),
    ("doc_set", "Set the 'debug' key to true in config.json.",
     _setup_doc_set, _check_doc_set),
    ("tx_multi_file", "Do both atomically using a patchloom transaction: create hello.txt with 'Hello, World!' and update version in package.json to 3.0.0.",
     _setup_tx, _check_tx),
]


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

@pytest.fixture(scope="module")
def bench_agent(request):
    agent_name = request.config.getoption("--agent")
    model = request.config.getoption("--model")
    driver = create_driver(agent_name, model)
    if not driver.is_available():
        pytest.skip(f"Agent {agent_name!r} not available")
    return driver


@pytest.fixture(scope="module")
def bench_patchloom_bin():
    return _find_patchloom_binary()


@pytest.mark.timeout(300)
@pytest.mark.parametrize("scenario_name,prompt,setup_fn,check_fn", SCENARIOS,
                         ids=[s[0] for s in SCENARIOS])
@pytest.mark.parametrize("with_agents_md", [True, False],
                         ids=["patchloom", "native"])
def test_bench(bench_agent, bench_patchloom_bin, tmp_path,
               scenario_name, prompt, setup_fn, check_fn, with_agents_md):
    """Run a scenario in patchloom or native mode and collect metrics."""
    row = _run_bench(
        bench_agent, bench_patchloom_bin, tmp_path,
        with_agents_md, prompt, setup_fn, check_fn, scenario_name,
    )
    # Print inline result for -v output
    status = "OK" if row.success else "FAIL"
    print(
        f"  [{row.mode}] {row.scenario}: {row.duration_secs}s, "
        f"{row.patchloom_call_count} pl calls ({row.patchloom_time_ms}ms), "
        f"raw={row.raw_tools or 'none'}, {status}"
    )


def pytest_terminal_summary(terminalreporter, exitstatus, config):
    """Print the comparison table at the end of the test run."""
    if not _bench_results:
        return

    terminalreporter.write_sep("=", "Agent Benchmark Results")

    # Header
    header = f"{'Scenario':<20} {'Mode':<12} {'Duration':>10} {'PL calls':>10} {'PL time':>10} {'Raw tools':>15} {'OK?':>5}"
    terminalreporter.write_line(header)
    terminalreporter.write_line("-" * len(header))

    for row in sorted(_bench_results, key=lambda r: (r.scenario, r.mode)):
        status = "yes" if row.success else "FAIL"
        raw = ",".join(row.raw_tools) if row.raw_tools else "-"
        pl_time = f"{row.patchloom_time_ms}ms" if row.patchloom_time_ms > 0 else "-"
        terminalreporter.write_line(
            f"{row.scenario:<20} {row.mode:<12} {row.duration_secs:>9.1f}s "
            f"{row.patchloom_call_count:>10} {pl_time:>10} {raw:>15} {status:>5}"
        )

    terminalreporter.write_line("")
