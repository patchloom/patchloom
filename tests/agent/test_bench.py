"""Multi-turn agent benchmarks: patchloom instructions vs native tools.

Simulates realistic agent usage by running 5 tasks in a single session.
AGENTS.md is read once at the start; subsequent tasks reuse the context.
This amortizes instruction processing overhead across multiple tasks,
matching real-world usage patterns.

Two modes:
- "patchloom": workspace has AGENTS.md with patchloom instructions
- "native": workspace has no AGENTS.md (agent uses native tools only)

Run with:
    make bench-agent
    make bench-agent MODEL=sxs-gpt-5-4
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

import pytest

from drivers import create_driver

SHIM_TEMPLATE = (Path(__file__).parent / "shim.sh").read_text()


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


def _make_workspace(tmp_path, real_bin, with_agents_md):
    ws = tmp_path / "workspace"
    ws.mkdir(exist_ok=True)
    if with_agents_md:
        result = subprocess.run([real_bin, "agent-rules"], capture_output=True, text=True, check=True)
        (ws / "AGENTS.md").write_text(result.stdout)
    subprocess.run(["git", "init"], cwd=ws, capture_output=True, check=True)
    subprocess.run(["git", "add", "."], cwd=ws, capture_output=True, check=True)
    subprocess.run(["git", "-c", "user.name=test", "-c", "user.email=test@test.com", "commit", "--allow-empty", "-m", "init"], cwd=ws, capture_output=True, check=True)
    return ws


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
        "setup": lambda ws: (ws / "package.json").write_text(json.dumps({"name": "myapp", "version": "1.0.0"}, indent=2) + "\n"),
        "check": lambda ws: (ws / "hello.txt").exists() and json.loads((ws / "package.json").read_text()).get("version") == "3.0.0",
    },
    {
        "name": "batch_6_files",
        "prompt": "Update the version from '1.0.0' to '2.0.0' in ALL of these files: package.json, pyproject.toml, config.yaml, config.json, version.txt, and the badge in README.md. Make sure every single file is updated.",
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
]


@dataclass
class TaskResult:
    name: str
    duration_secs: float
    patchloom_calls: int
    success: bool


@dataclass
class SessionResult:
    mode: str
    model: str
    tasks: list[TaskResult] = field(default_factory=list)
    total_secs: float = 0.0


def _run_session(agent, real_bin, tmp_path, with_agents_md):
    mode = "patchloom" if with_agents_md else "native"
    ws = _make_workspace(tmp_path, real_bin, with_agents_md)
    shim_env, log_path = _make_shim(tmp_path, real_bin)
    session = SessionResult(mode=mode, model=agent.model)
    env = {**os.environ, **shim_env}
    total_start = time.monotonic()
    session_id = None  # captured from first task's JSON output

    for task in TASKS:
        task["setup"](ws)
        prev_count = len(log_path.read_text().splitlines()) if log_path.exists() else 0

        cmd = ["grok", "-p", task["prompt"], "--yolo", "--output-format", "json",
               "--max-turns", "15", "--cwd", str(ws), "-m", agent.model]
        if session_id:
            cmd += ["-r", session_id]
        start = time.monotonic()
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, timeout=240, env=env)
        except subprocess.TimeoutExpired:
            session.tasks.append(TaskResult(name=task["name"], duration_secs=240.0, patchloom_calls=0, success=False))
            continue
        duration = time.monotonic() - start

        # Extract session ID from first successful run for subsequent resumes
        if not session_id:
            try:
                out = json.loads(proc.stdout)
                session_id = out.get("sessionId")
            except (json.JSONDecodeError, TypeError):
                pass

        new_calls = (len(log_path.read_text().splitlines()) if log_path.exists() else 0) - prev_count
        try:
            success = task["check"](ws)
        except Exception:
            success = False

        session.tasks.append(TaskResult(name=task["name"], duration_secs=round(duration, 1), patchloom_calls=new_calls, success=bool(success)))
        print(f"    [{mode}] {task['name']}: {duration:.1f}s, {new_calls} pl calls, {'OK' if success else 'FAIL'}")

    session.total_secs = round(time.monotonic() - total_start, 1)
    return session


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


@pytest.mark.timeout(1200)
def test_multi_turn_patchloom(bench_agent, bench_patchloom_bin, tmp_path, request):
    """Run 5 tasks in one session WITH patchloom AGENTS.md."""
    print("\n  === Multi-turn session: PATCHLOOM mode ===")
    session = _run_session(bench_agent, bench_patchloom_bin, tmp_path, True)
    print(f"  Total: {session.total_secs}s")
    # Stash for comparison table
    if not hasattr(request.config, "_bench_sessions"):
        request.config._bench_sessions = {}
    request.config._bench_sessions["patchloom"] = session
    assert any(t.success for t in session.tasks)


@pytest.mark.timeout(1200)
def test_multi_turn_native(bench_agent, bench_patchloom_bin, tmp_path, request):
    """Run 5 tasks in one session WITHOUT patchloom AGENTS.md."""
    print("\n  === Multi-turn session: NATIVE mode ===")
    session = _run_session(bench_agent, bench_patchloom_bin, tmp_path, False)
    print(f"  Total: {session.total_secs}s")
    if not hasattr(request.config, "_bench_sessions"):
        request.config._bench_sessions = {}
    request.config._bench_sessions["native"] = session

    # Print comparison table after both sessions complete
    sessions = request.config._bench_sessions
    if "patchloom" in sessions and "native" in sessions:
        _print_comparison(sessions["patchloom"], sessions["native"])

    assert any(t.success for t in session.tasks)


def _print_comparison(pl, native):
    """Print side-by-side comparison table."""
    print("\n  ┌─────────────────────────────────────────────────────────────────────────────┐")
    print(f"  │ {'Multi-Turn Benchmark: patchloom vs native':^75} │")
    print(f"  │ {'Model: ' + pl.model:^75} │")
    print("  ├──────────────────┬────────────┬──────────────┬──────────┬────────┬──────────┤")
    print("  │ Task             │   PL time  │  Native time │    Delta │ PL cnt │   Winner │")
    print("  ├──────────────────┼────────────┼──────────────┼──────────┼────────┼──────────┤")

    for pt, nt in zip(pl.tasks, native.tasks):
        delta = pt.duration_secs - nt.duration_secs
        sign = "+" if delta > 0 else ""
        winner = "native" if delta > 1.5 else ("patchloom" if delta < -1.5 else "~same")
        p_mark = "*" if not pt.success else " "
        n_mark = "*" if not nt.success else " "
        print(
            f"  │ {pt.name:<16} │ {pt.duration_secs:>7.1f}s{p_mark}  "
            f"│ {nt.duration_secs:>9.1f}s{n_mark}  "
            f"│ {sign}{delta:>6.1f}s │ {pt.patchloom_calls:>6} │ {winner:>8} │"
        )

    print("  ├──────────────────┼────────────┼──────────────┼──────────┼────────┼──────────┤")
    d = pl.total_secs - native.total_secs
    sign = "+" if d > 0 else ""
    print(
        f"  │ {'TOTAL':<16} │ {pl.total_secs:>7.1f}s   "
        f"│ {native.total_secs:>9.1f}s   "
        f"│ {sign}{d:>6.1f}s │        │          │"
    )
    print("  └──────────────────┴────────────┴──────────────┴──────────┴────────┴──────────┘")
    print("  (* = task failed)")
