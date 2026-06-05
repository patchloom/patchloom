"""Multi-turn agent benchmarks: patchloom CLI vs MCP vs native tools.

Simulates realistic agent usage by running tasks in a single session.
AGENTS.md is read once at the start; subsequent tasks reuse the context.
This amortizes instruction processing overhead across multiple tasks,
matching real-world usage patterns.

Three modes:
- "patchloom": workspace has AGENTS.md with patchloom CLI instructions
- "mcp": workspace has MCP tools configured (patchloom mcp-server)
- "native": workspace has no AGENTS.md (agent uses native tools only)

Variance reduction:
- Use --runs N (default 1) to run each mode N times and take the median.
- With N>=3, the comparison table shows median and min-max range.

Run with:
    make bench-agent
    make bench-agent MODEL=sxs-gpt-5-4
    make bench-agent MODEL=sxs-gpt-5-4 RUNS=3
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import statistics
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

import pytest

from drivers import create_driver

SHIM_TEMPLATE = (Path(__file__).parent / "shim.sh").read_text()

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _find_patchloom_binary() -> str:
    repo_root = Path(__file__).resolve().parent.parent.parent
    for subdir in ["release", "debug"]:
        p = repo_root / "target" / subdir / "patchloom"
        if p.exists():
            return str(p)
    found = shutil.which("patchloom")
    if found:
        return found
    pytest.skip("patchloom binary not found")
    return ""  # unreachable; pytest.skip raises


def _has_mcp_support(binary: str) -> bool:
    """Check if the patchloom binary was built with --features mcp."""
    try:
        proc = subprocess.run(
            [binary, "mcp-server", "--help"],
            capture_output=True, timeout=5,
        )
        return proc.returncode == 0
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return False


def _make_shim(tmp_path, real_bin):
    shim_dir = tmp_path / "shim_bin"
    shim_dir.mkdir(exist_ok=True)
    log_file = tmp_path / "patchloom_calls.jsonl"
    shim_script = SHIM_TEMPLATE.replace("__REAL_PATH__", real_bin)
    shim_script = shim_script.replace("__LOG_PATH__", str(log_file))
    shim_path = shim_dir / "patchloom"
    shim_path.write_text(shim_script)
    shim_path.chmod(shim_path.stat().st_mode | stat.S_IEXEC)
    return {"PATH": f"{shim_dir}:{os.environ.get('PATH', '')}", "PATCHLOOM_SHIM_LOG": str(log_file)}, log_file


def _make_workspace(tmp_path, real_bin, mode):
    """Create an isolated workspace.

    mode="patchloom": AGENTS.md with CLI instructions
    mode="mcp":       .grok/config.toml with MCP server + minimal AGENTS.md
    mode="native":    no AGENTS.md
    """
    ws = tmp_path / "workspace"
    ws.mkdir(exist_ok=True)

    if mode == "patchloom":
        result = subprocess.run(
            [real_bin, "agent-rules", "--mode", "cli", "--platform", "linux"],
            capture_output=True, text=True, check=True,
        )
        (ws / "AGENTS.md").write_text(result.stdout)
    elif mode == "mcp":
        # Configure grok to discover patchloom as an MCP server
        grok_dir = ws / ".grok"
        grok_dir.mkdir(exist_ok=True)
        mcp_log = ws / "mcp_calls.jsonl"
        (grok_dir / "config.toml").write_text(
            f'[mcp_servers.patchloom]\n'
            f'command = "{real_bin}"\n'
            f'args = ["mcp-server"]\n'
            f'startup_timeout_sec = 10\n'
            f'tool_timeout_sec = 30\n'
            f'[mcp_servers.patchloom.env]\n'
            f'PATCHLOOM_MCP_LOG = "{mcp_log}"\n'
        )
        # Focused MCP AGENTS.md with tool list and usage guidance
        result = subprocess.run(
            [real_bin, "agent-rules", "--mode", "mcp"],
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
# Task definitions
# ---------------------------------------------------------------------------

TASKS = [
    {
        "name": "search",
        "prompt": "Find all lines containing 'TODO' across all source files. Report which files and line numbers.",
        "setup": lambda ws: [
            (ws / "src").mkdir(exist_ok=True),
            (ws / "src" / "main.py").write_text("# TODO: implement auth\ndef main():\n    pass\n"),
            (ws / "src" / "utils.py").write_text("def helper():\n    # TODO: add logging\n    return 42\n"),
        ],
        "check": lambda ws: True,  # search is read-only, just check it completes
    },
    {
        "name": "replace",
        "prompt": "Rename the function 'calculate_total' to 'compute_sum' in all files under src/.",
        "setup": lambda ws: [
            (ws / "src" / "app.py").write_text("def calculate_total(items):\n    return sum(i.price for i in items)\nresult = calculate_total(order_items)\n"),
            (ws / "src" / "test_app.py").write_text("from app import calculate_total\ndef test_it():\n    assert calculate_total([]) == 0\n"),
        ],
        "check": lambda ws: "compute_sum" in (ws / "src" / "app.py").read_text() and "calculate_total" not in (ws / "src" / "app.py").read_text(),
    },
    {
        "name": "doc_set",
        "prompt": "Set the 'debug' key to true in config.json.",
        "setup": lambda ws: (ws / "config.json").write_text(json.dumps({"app_name": "MyApp", "debug": False}, indent=2) + "\n"),
        "check": lambda ws: json.loads((ws / "config.json").read_text()).get("debug") is True,
    },
    {
        "name": "md_table",
        "prompt": "Add a new row to the Commands table in README.md: command is 'lint' and description is 'Run the linter'.",
        "setup": lambda ws: (ws / "README.md").write_text("# Project\n\n## Commands\n\n| Command | Description |\n|---------|-------------|\n| build | Build it |\n\n## License\n\nMIT\n"),
        "check": lambda ws: "lint" in (ws / "README.md").read_text(),
    },
    {
        "name": "tx_multi_file",
        "prompt": "Create hello.txt with content 'Hello, World!' and update version in package.json from '1.0.0' to '3.0.0'. Both changes must succeed together or neither should apply.",
        "prompt_mcp": (
            "Create hello.txt with content 'Hello, World!' and update version in package.json "
            "from '1.0.0' to '3.0.0'. Both changes must succeed together or neither should apply. "
            "Use the transaction tool with file.create and doc.set operations."
        ),
        "setup": lambda ws: (ws / "package.json").write_text(json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n"),
        "check": lambda ws: (ws / "hello.txt").exists() and json.loads((ws / "package.json").read_text()).get("version") == "3.0.0",
    },
    {
        "name": "batch_6_files",
        "prompt": (
            "Update the version from '1.0.0' to '2.0.0' in ALL of these files: "
            "package.json, pyproject.toml, config.yaml, config.json, version.txt, "
            "and the badge in README.md. Make sure every single file is updated."
        ),
        # In patchloom/mcp mode, hint that batch is the right tool
        "prompt_patchloom": (
            "Update the version from '1.0.0' to '2.0.0' in ALL of these files: "
            "package.json, pyproject.toml, config.yaml, config.json, version.txt, "
            "and the badge in README.md. Use patchloom batch to do all 6 in a "
            "single command. Make sure every single file is updated."
        ),
        "prompt_mcp": (
            "Update the version from '1.0.0' to '2.0.0' in ALL of these files: "
            "package.json, pyproject.toml, config.yaml, config.json, version.txt, "
            "and the badge in README.md. Use the batch tool to do all 6 in one call."
        ),
        "setup": lambda ws: [
            (ws / "package.json").write_text(json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n"),
            (ws / "pyproject.toml").write_text('[project]\nname = "myapp"\nversion = "1.0.0"\n'),
            (ws / "config.yaml").write_text("app:\n  name: myapp\n  version: '1.0.0'\n"),
            (ws / "config.json").write_text(json.dumps({"app_name": "myapp", "version": "1.0.0"}, indent=2) + "\n"),
            (ws / "version.txt").write_text("1.0.0\n"),
            (ws / "README.md").write_text("# MyApp\n\n![version](https://img.shields.io/badge/version-1.0.0-blue)\n\nA sample app.\n"),
        ],
        "check": lambda ws: all([
            json.loads((ws / "package.json").read_text()).get("version") == "2.0.0",
            "2.0.0" in (ws / "pyproject.toml").read_text(),
            "2.0.0" in (ws / "config.yaml").read_text(),
            json.loads((ws / "config.json").read_text()).get("version") == "2.0.0",
            (ws / "version.txt").read_text().strip() == "2.0.0",
            "2.0.0" in (ws / "README.md").read_text(),
        ]),
    },
    {
        "name": "batch_mixed_ops",
        "prompt": (
            "Make these changes atomically:\n"
            "1. Set the 'version' key to '4.0.0' in config.json\n"
            "2. Replace 'v3' with 'v4' in README.md\n"
            "3. Add bullet '- v4.0.0 released' under the '## Changelog' heading in CHANGELOG.md\n"
            "All three must succeed together."
        ),
        "prompt_mcp": (
            "Make these changes atomically using the transaction tool:\n"
            "1. Set the 'version' key to '4.0.0' in config.json (doc.set op)\n"
            "2. Replace 'v3' with 'v4' in README.md (replace op)\n"
            "3. Add bullet '- v4.0.0 released' under '## Changelog' in CHANGELOG.md (md.upsert_bullet op)\n"
            "All three in one transaction call."
        ),
        "setup": lambda ws: [
            (ws / "config.json").write_text(json.dumps({"name": "myapp", "version": "3.0.0"}, indent=2) + "\n"),
            (ws / "README.md").write_text("# MyApp v3\n\nA sample app running v3.\n"),
            (ws / "CHANGELOG.md").write_text("# Changelog\n\n## Changelog\n\n- v3.0.0 initial release\n"),
        ],
        "check": lambda ws: all([
            json.loads((ws / "config.json").read_text()).get("version") == "4.0.0",
            "v4" in (ws / "README.md").read_text(),
            "v4.0.0 released" in (ws / "CHANGELOG.md").read_text(),
        ]),
    },
    {
        "name": "yaml_comment_preserve",
        "prompt": (
            "Set the 'database.port' key to 5432 in config.yaml. "
            "The file has comments that MUST be preserved."
        ),
        "setup": lambda ws: (ws / "config.yaml").write_text(
            "# Database configuration\n"
            "database:\n"
            "  host: localhost  # primary host\n"
            "  port: 3306  # default MySQL port\n"
            "  name: myapp  # database name\n"
        ),
        "check": lambda ws: all([
            "5432" in (ws / "config.yaml").read_text(),
            "# primary host" in (ws / "config.yaml").read_text(),
            "# database name" in (ws / "config.yaml").read_text(),
        ]),
    },
    {
        "name": "md_insert",
        "prompt": (
            "Insert the text 'Released on 2026-01-15.' immediately after the '## v2.0.0' heading "
            "in CHANGELOG.md. Do NOT replace the existing content under that heading."
        ),
        "setup": lambda ws: (ws / "CHANGELOG.md").write_text(
            "# Changelog\n\n"
            "## v2.0.0\n\n"
            "- Added new feature\n"
            "- Fixed bug\n\n"
            "## v1.0.0\n\n"
            "- Initial release\n"
        ),
        "check": lambda ws: all([
            "Released on 2026-01-15" in (ws / "CHANGELOG.md").read_text(),
            "Added new feature" in (ws / "CHANGELOG.md").read_text(),
            "Fixed bug" in (ws / "CHANGELOG.md").read_text(),
        ]),
    },
    {
        "name": "file_ops",
        "prompt": (
            "Create a file called 'LICENSE' with the text 'MIT License\\n\\nCopyright 2026 MyApp'. "
            "Then rename 'old_settings.json' to 'settings.json'."
        ),
        "setup": lambda ws: [
            (ws / "old_settings.json").write_text(json.dumps({"theme": "dark"}, indent=2) + "\n"),
        ],
        "check": lambda ws: all([
            (ws / "LICENSE").exists(),
            "MIT License" in (ws / "LICENSE").read_text(),
            (ws / "settings.json").exists(),
            not (ws / "old_settings.json").exists(),
        ]),
    },
    {
        "name": "tidy",
        "prompt": (
            "Check all .txt files in the project for missing final newlines and trailing whitespace. "
            "Report which files have issues, then fix all the issues."
        ),
        "prompt_mcp": (
            "Fix whitespace issues in all .txt files: trim trailing whitespace and ensure "
            "each file ends with a newline. Use the patchloom fix_whitespace tool on each file. "
            "Don't modify files that are already clean."
        ),
        "setup": lambda ws: [
            (ws / "clean.txt").write_text("This file is clean\n"),
            (ws / "dirty1.txt").write_text("trailing spaces   \nmore trailing\t\n"),
            (ws / "dirty2.txt").write_text("no final newline"),
        ],
        "check": lambda ws: all([
            (ws / "dirty1.txt").read_text() == "trailing spaces\nmore trailing\n",
            (ws / "dirty2.txt").read_text().endswith("\n"),
            (ws / "clean.txt").read_text() == "This file is clean\n",
        ]),
    },
]


# ---------------------------------------------------------------------------
# Data classes
# ---------------------------------------------------------------------------


@dataclass
class TaskResult:
    name: str
    duration_secs: float
    patchloom_calls: int
    patchloom_duration_ms: int
    success: bool


@dataclass
class SessionResult:
    mode: str
    model: str
    tasks: list[TaskResult] = field(default_factory=list)
    total_secs: float = 0.0
    first_task_secs: float = 0.0
    subsequent_avg_secs: float = 0.0


# ---------------------------------------------------------------------------
# Session runner
# ---------------------------------------------------------------------------


def _get_prompt(task: dict, mode: str) -> str:
    """Return mode-specific prompt if available, else the default."""
    key = f"prompt_{mode}"
    return task.get(key, task["prompt"])


def _print_failure_diag(ws, task_name):
    """Print workspace state for failed tasks to diagnose why the check failed."""
    # Map task names to relevant files
    diag_files = {
        "replace": ["src/app.py", "src/test_app.py"],
        "doc_set": ["config.json"],
        "md_table": ["README.md"],
        "tx_multi_file": ["hello.txt", "package.json"],
        "batch_6_files": ["package.json", "pyproject.toml", "config.yaml", "config.json", "version.txt", "README.md"],
        "batch_mixed_ops": ["config.json", "README.md", "CHANGELOG.md"],
        "yaml_comment_preserve": ["config.yaml"],
        "md_insert": ["CHANGELOG.md"],
        "file_ops": ["LICENSE", "settings.json", "old_settings.json"],
        "tidy": ["dirty1.txt", "dirty2.txt", "clean.txt"],
    }
    files = diag_files.get(task_name, [])
    if not files:
        return
    print(f"      --- DIAG: {task_name} ---")
    for fname in files:
        fpath = ws / fname
        if fpath.exists():
            content = fpath.read_text()
            # Truncate long content, show repr to reveal whitespace
            if len(content) > 200:
                content = content[:200] + "..."
            print(f"      {fname}: {content!r}")
        else:
            print(f"      {fname}: MISSING")
    print(f"      --- END DIAG ---")


def _run_session(agent, real_bin, tmp_path, mode):
    """Run one full benchmark session. mode is 'patchloom', 'mcp', or 'native'."""
    ws = _make_workspace(tmp_path, real_bin, mode)
    shim_env, log_path = _make_shim(tmp_path, real_bin)
    session = SessionResult(mode=mode, model=agent.model)
    env = {**os.environ, **shim_env}
    mcp_log = ws / "mcp_calls.jsonl" if mode == "mcp" else tmp_path / "mcp_calls.jsonl"
    _run_session._prev_mcp_count = 0
    total_start = time.monotonic()
    session_id = None

    for task in TASKS:
        task["setup"](ws)
        prev_lines = log_path.read_text().splitlines() if log_path.exists() else []
        prev_count = len(prev_lines)

        prompt = _get_prompt(task, mode)
        cmd = ["grok", "-p", prompt, "--yolo", "--output-format", "json",
               "--max-turns", "15", "--cwd", str(ws), "-m", agent.model]
        if session_id:
            cmd += ["-r", session_id]
        start = time.monotonic()
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=240, env=env)
        except subprocess.TimeoutExpired:
            session.tasks.append(TaskResult(name=task["name"], duration_secs=240.0, patchloom_calls=0, patchloom_duration_ms=0, success=False))
            continue
        duration = time.monotonic() - start

        if not session_id:
            try:
                out = json.loads(proc.stdout)
                session_id = out.get("sessionId")
            except (json.JSONDecodeError, TypeError):
                pass  # first-run output may not be valid JSON

        # Parse shim log for CLI call count and per-call durations
        new_calls = 0
        pl_duration_ms = 0
        if log_path.exists():
            all_lines = log_path.read_text().splitlines()
            for line in all_lines[prev_count:]:
                line = line.strip()
                if not line:
                    continue
                new_calls += 1
                try:
                    entry = json.loads(line)
                    d = entry.get("duration_ms", 0)
                    if isinstance(d, (int, float)) and d > 0:
                        pl_duration_ms += int(d)
                except (json.JSONDecodeError, TypeError):
                    pass  # skip malformed shim log entries

        # For MCP mode, also parse MCP server log for tool calls
        mcp_calls_this_task = 0
        if mode == "mcp" and mcp_log.exists():
            mcp_lines = mcp_log.read_text().splitlines()
            # Count lines added since last task (use prev_mcp_count tracking)
            if not hasattr(_run_session, '_prev_mcp_count'):
                _run_session._prev_mcp_count = 0
            for line in mcp_lines[_run_session._prev_mcp_count:]:
                line = line.strip()
                if line:
                    mcp_calls_this_task += 1
            _run_session._prev_mcp_count = len(mcp_lines)

        try:
            success = task["check"](ws)
        except Exception:
            success = False

        session.tasks.append(TaskResult(
            name=task["name"],
            duration_secs=round(duration, 1),
            patchloom_calls=new_calls,
            patchloom_duration_ms=pl_duration_ms,
            success=bool(success),
        ))
        pl_info = f", {new_calls} pl calls ({pl_duration_ms}ms)" if new_calls else ""
        mcp_info = f", {mcp_calls_this_task} mcp calls" if mcp_calls_this_task else ""
        print(f"    [{mode}] {task['name']}: {duration:.1f}s{pl_info}{mcp_info}, {'OK' if success else 'FAIL'}")
        if not success:
            _print_failure_diag(ws, task["name"])
            # Dump CLI shim log entries for this task
            if new_calls > 0 and log_path.exists():
                print(f"      --- CLI SHIM LOG ({new_calls} calls) ---")
                all_log = log_path.read_text().splitlines()
                for line in all_log[prev_count:]:
                    line = line.strip()
                    if line:
                        print(f"      {line[:500]}")
                print(f"      --- END CLI SHIM LOG ---")
            # Dump MCP call log entries for this task
            if mcp_calls_this_task > 0 and mcp_log.exists():
                print(f"      --- MCP CALL LOG ({mcp_calls_this_task} calls) ---")
                mcp_all = mcp_log.read_text().splitlines()
                prev_mcp = _run_session._prev_mcp_count - mcp_calls_this_task
                for line in mcp_all[prev_mcp:_run_session._prev_mcp_count]:
                    line = line.strip()
                    if line:
                        print(f"      {line[:500]}")
                print(f"      --- END MCP CALL LOG ---")

    session.total_secs = round(time.monotonic() - total_start, 1)
    if session.tasks:
        session.first_task_secs = session.tasks[0].duration_secs
        if len(session.tasks) > 1:
            session.subsequent_avg_secs = round(
                sum(t.duration_secs for t in session.tasks[1:]) / (len(session.tasks) - 1), 1
            )
    return session


def _run_n_sessions(agent, real_bin, tmp_path, mode, n_runs):
    """Run a mode N times and return all SessionResults."""
    results = []
    for i in range(n_runs):
        run_dir = tmp_path / f"run_{i}"
        run_dir.mkdir(exist_ok=True)
        if n_runs > 1:
            print(f"\n  --- {mode.upper()} run {i + 1}/{n_runs} ---")
        session = _run_session(agent, real_bin, run_dir, mode)
        print(f"  Total: {session.total_secs}s")
        results.append(session)
    return results


# ---------------------------------------------------------------------------
# Statistics helpers
# ---------------------------------------------------------------------------


def _median(values: list[float]) -> float:
    return round(statistics.median(values), 1) if values else 0.0


def _aggregate_runs(runs: list[SessionResult]) -> dict:
    """Compute per-task and total statistics across N runs.

    Returns a dict with:
      median_total, min_total, max_total,
      tasks: {name: {median, min, max, pl_calls_median, pl_exec_median}}
    """
    totals = [r.total_secs for r in runs]
    task_names = [t.name for t in runs[0].tasks] if runs else []
    task_stats = {}
    for i, name in enumerate(task_names):
        durations = [r.tasks[i].duration_secs for r in runs if i < len(r.tasks)]
        pl_calls = [r.tasks[i].patchloom_calls for r in runs if i < len(r.tasks)]
        pl_execs = [r.tasks[i].patchloom_duration_ms for r in runs if i < len(r.tasks)]
        successes = [r.tasks[i].success for r in runs if i < len(r.tasks)]
        task_stats[name] = {
            "median": _median(durations),
            "min": round(min(durations), 1) if durations else 0.0,
            "max": round(max(durations), 1) if durations else 0.0,
            "pl_calls_median": round(statistics.median(pl_calls)) if pl_calls else 0,
            "pl_exec_median": round(statistics.median(pl_execs)) if pl_execs else 0,
            "success_rate": sum(successes) / len(successes) if successes else 0.0,
        }
    return {
        "n_runs": len(runs),
        "median_total": _median(totals),
        "min_total": round(min(totals), 1) if totals else 0.0,
        "max_total": round(max(totals), 1) if totals else 0.0,
        "tasks": task_stats,
    }


# ---------------------------------------------------------------------------
# Fixtures
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


@pytest.fixture(scope="module")
def n_runs(request):
    return request.config.getoption("--runs")


# ---------------------------------------------------------------------------
# Test functions
# ---------------------------------------------------------------------------


def test_dry_run_prompts(request):
    """Print all benchmark prompts without calling the LLM API."""
    if not request.config.getoption("--dry-run-prompts"):
        pytest.skip("Use --dry-run-prompts to run this test")

    modes = ["patchloom", "mcp", "native"]
    for task in TASKS:
        print(f"\n{'=' * 60}")
        print(f"Task: {task['name']}")
        print(f"{'=' * 60}")
        for mode in modes:
            prompt = _get_prompt(task, mode)
            label = f"[{mode}]"
            if prompt == task["prompt"]:
                if mode != "patchloom":
                    continue  # same as default, skip
                label = "[default]"
            print(f"\n  {label}:")
            for line in prompt.split("\n"):
                print(f"    {line}")
        # Show check function source for transparency
        print(f"\n  [check]: {task['check'].__code__.co_code!r:.60}...")
    print(f"\n{'=' * 60}")
    print(f"Total: {len(TASKS)} tasks x {len(modes)} modes")
    print(f"{'=' * 60}")


@pytest.mark.timeout(1200)
def test_multi_turn_patchloom(bench_agent, bench_patchloom_bin, tmp_path, n_runs, request):
    """Run tasks in one session WITH patchloom CLI AGENTS.md."""
    print(f"\n  === Multi-turn session: PATCHLOOM mode ({n_runs} run{'s' if n_runs > 1 else ''}) ===")
    runs = _run_n_sessions(bench_agent, bench_patchloom_bin, tmp_path, "patchloom", n_runs)
    if not hasattr(request.config, "_bench_runs"):
        request.config._bench_runs = {}
    request.config._bench_runs["patchloom"] = runs
    assert any(t.success for r in runs for t in r.tasks)


@pytest.mark.timeout(1200)
def test_multi_turn_mcp(bench_agent, bench_patchloom_bin, tmp_path, n_runs, request):
    """Run tasks in one session WITH patchloom MCP tools."""
    if not _has_mcp_support(bench_patchloom_bin):
        pytest.skip("patchloom binary lacks MCP support (build with --features mcp)")
    print(f"\n  === Multi-turn session: MCP mode ({n_runs} run{'s' if n_runs > 1 else ''}) ===")
    runs = _run_n_sessions(bench_agent, bench_patchloom_bin, tmp_path, "mcp", n_runs)
    if not hasattr(request.config, "_bench_runs"):
        request.config._bench_runs = {}
    request.config._bench_runs["mcp"] = runs
    assert any(t.success for r in runs for t in r.tasks)


@pytest.mark.timeout(1200)
def test_multi_turn_native(bench_agent, bench_patchloom_bin, tmp_path, n_runs, request):
    """Run tasks in one session WITHOUT patchloom (native tools only)."""
    print(f"\n  === Multi-turn session: NATIVE mode ({n_runs} run{'s' if n_runs > 1 else ''}) ===")
    runs = _run_n_sessions(bench_agent, bench_patchloom_bin, tmp_path, "native", n_runs)
    if not hasattr(request.config, "_bench_runs"):
        request.config._bench_runs = {}
    request.config._bench_runs["native"] = runs

    # Print comparison table after all modes complete
    all_runs = request.config._bench_runs
    modes = [m for m in ["patchloom", "mcp", "native"] if m in all_runs]
    if len(modes) >= 2:
        _print_comparison(all_runs, modes, n_runs)
        results_dir = Path(__file__).resolve().parent.parent.parent / "benches" / "agent" / "results"
        _save_results(all_runs, modes, results_dir, n_runs)

    assert any(t.success for r in runs for t in r.tasks)


# ---------------------------------------------------------------------------
# Comparison table
# ---------------------------------------------------------------------------

# Column labels for each mode (short names for table headers)
_MODE_LABELS = {"patchloom": "PL-CLI", "mcp": "MCP", "native": "Native"}


def _print_comparison(all_runs, modes, n_runs):
    """Print side-by-side comparison table for 2 or 3 modes."""
    aggs = {m: _aggregate_runs(all_runs[m]) for m in modes}
    model = all_runs[modes[0]][0].model

    # Build header
    n_modes = len(modes)
    col_w = 11  # width per mode column
    mode_headers = "".join(f"│ {_MODE_LABELS[m]:^{col_w}} " for m in modes)
    mode_dashes = "".join(f"┼{'─' * (col_w + 1)}" for _ in modes)
    mode_bottom = "".join(f"┴{'─' * (col_w + 1)}" for _ in modes)
    mode_top = "".join(f"┬{'─' * (col_w + 1)}" for _ in modes)

    total_w = 18 + (col_w + 2) * n_modes + 10 + 10 + 2  # approx
    title = f"Multi-Turn Benchmark: {' vs '.join(_MODE_LABELS[m] for m in modes)}"
    subtitle = f"Model: {model}" + (f"  (median of {n_runs} runs)" if n_runs > 1 else "")

    print(f"\n  ┌{'─' * (total_w)}┐")
    print(f"  │ {title:^{total_w - 2}} │")
    print(f"  │ {subtitle:^{total_w - 2}} │")
    print(f"  ├{'─' * 18}{mode_top}┬{'─' * 10}┬{'─' * 10}┤")

    header_line = f"  │ {'Task':<16} {mode_headers}│ {'Winner':^8} │ {'PL exec':^8} │"
    print(header_line)
    print(f"  ├{'─' * 18}{mode_dashes}┼{'─' * 10}┼{'─' * 10}┤")

    task_names = list(aggs[modes[0]]["tasks"].keys())
    for name in task_names:
        vals = {}
        for m in modes:
            ts = aggs[m]["tasks"].get(name, {})
            vals[m] = ts.get("median", 0.0)

        # Determine winner (lowest median wins, 1.5s threshold for ~same)
        sorted_modes = sorted(modes, key=lambda m: vals[m])
        best = vals[sorted_modes[0]]
        second = vals[sorted_modes[1]]
        winner = _MODE_LABELS[sorted_modes[0]] if (second - best) > 1.5 else "~same"

        # PL exec from patchloom CLI mode (MCP calls go through the server, not shim)
        pl_exec_ms = aggs.get("patchloom", {}).get("tasks", {}).get(name, {}).get("pl_exec_median", 0)
        pl_exec = f"{pl_exec_ms}ms" if pl_exec_ms > 0 else ""

        cols = ""
        for m in modes:
            ts = aggs[m]["tasks"].get(name, {})
            v = ts.get("median", 0.0)
            sr = ts.get("success_rate", 0.0)
            mark = "*" if sr < 1.0 else " "
            if n_runs > 1:
                lo = ts.get("min", v)
                hi = ts.get("max", v)
                rng = f" [{lo}-{hi}]" if hi - lo > 0.5 else ""
                cell = f"{v:>5.1f}s{mark}{rng}"
            else:
                cell = f"{v:>7.1f}s{mark}"
            cols += f"│ {cell:>{col_w}} "

        print(f"  │ {name:<16} {cols}│ {winner:>8} │ {pl_exec:>8} │")

    # Totals row
    print(f"  ├{'─' * 18}{mode_dashes}┼{'─' * 10}┼{'─' * 10}┤")
    cols = ""
    for m in modes:
        v = aggs[m]["median_total"]
        if n_runs > 1:
            lo = aggs[m]["min_total"]
            hi = aggs[m]["max_total"]
            rng = f" [{lo}-{hi}]" if hi - lo > 0.5 else ""
            cell = f"{v:>5.1f}s {rng}"
        else:
            cell = f"{v:>7.1f}s "
        cols += f"│ {cell:>{col_w}} "

    total_pl_ms = sum(
        aggs.get("patchloom", {}).get("tasks", {}).get(n, {}).get("pl_exec_median", 0)
        for n in task_names
    )
    total_pl = f"{total_pl_ms}ms" if total_pl_ms > 0 else ""
    print(f"  │ {'TOTAL':<16} {cols}│ {'':>8} │ {total_pl:>8} │")
    print(f"  └{'─' * 18}{mode_bottom}┴{'─' * 10}┴{'─' * 10}┘")
    print("  (* = task failed in at least one run)")

    # First-task latency analysis (use first run of each mode)
    if "patchloom" in modes and "native" in modes:
        pl_first = all_runs["patchloom"][0].first_task_secs
        nat_first = all_runs["native"][0].first_task_secs
        pl_subseq = all_runs["patchloom"][0].subsequent_avg_secs
        nat_subseq = all_runs["native"][0].subsequent_avg_secs
        ft_delta = pl_first - nat_first
        sa_delta = pl_subseq - nat_subseq
        print(f"\n  First-task latency:   PL-CLI {pl_first:.1f}s  vs  Native {nat_first:.1f}s  (delta: {ft_delta:+.1f}s)")
        print(f"  Subsequent avg:       PL-CLI {pl_subseq:.1f}s  vs  Native {nat_subseq:.1f}s  (delta: {sa_delta:+.1f}s)")
        if ft_delta > sa_delta + 1.0:
            overhead = ft_delta - sa_delta
            print(f"  Estimated AGENTS.md ingestion overhead: ~{overhead:.1f}s")


# ---------------------------------------------------------------------------
# JSON export
# ---------------------------------------------------------------------------


def _save_results(all_runs, modes, output_dir, n_runs):
    """Save benchmark results to a JSON file for historical comparison."""
    output_dir.mkdir(parents=True, exist_ok=True)
    model = all_runs[modes[0]][0].model
    timestamp = time.strftime("%Y%m%d_%H%M%S")
    filename = f"{model}_{timestamp}.json"

    def _session_dict(s):
        return {
            "mode": s.mode,
            "model": s.model,
            "total_secs": s.total_secs,
            "first_task_secs": s.first_task_secs,
            "subsequent_avg_secs": s.subsequent_avg_secs,
            "tasks": [
                {
                    "name": t.name,
                    "duration_secs": t.duration_secs,
                    "patchloom_calls": t.patchloom_calls,
                    "patchloom_duration_ms": t.patchloom_duration_ms,
                    "success": t.success,
                }
                for t in s.tasks
            ],
        }

    # Include prompts for reproducibility
    prompts = {}
    for task in TASKS:
        prompts[task["name"]] = {}
        for mode in modes:
            prompts[task["name"]][mode] = _get_prompt(task, mode)

    data = {
        "timestamp": timestamp,
        "model": model,
        "n_runs": n_runs,
        "prompts": prompts,
    }
    for mode in modes:
        runs = all_runs[mode]
        agg = _aggregate_runs(runs)
        data[mode] = {
            "aggregate": agg,
            "runs": [_session_dict(r) for r in runs],
        }

    filepath = output_dir / filename
    filepath.write_text(json.dumps(data, indent=2) + "\n")
    print(f"\n  Results saved to {filepath}")
