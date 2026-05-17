"""Base agent driver abstraction for patchloom agent integration tests."""

from __future__ import annotations

import json
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class AgentResult:
    """Result of running an agent prompt."""

    stdout: str
    stderr: str
    exit_code: int
    output_json: dict | None
    duration_secs: float
    patchloom_calls: list[dict] = field(default_factory=list)


class AgentDriver(ABC):
    """Abstract interface for invoking an AI agent in headless mode."""

    def __init__(self, name: str, model: str):
        self.name = name
        self.model = model

    @abstractmethod
    def run_prompt(
        self,
        prompt: str,
        cwd: Path,
        *,
        max_turns: int = 10,
        timeout_secs: int = 120,
        extra_env: dict | None = None,
    ) -> AgentResult:
        """Run a single prompt and return the result."""

    @abstractmethod
    def is_available(self) -> bool:
        """Check if this agent and model are configured and reachable."""


def create_driver(agent_name: str, model: str) -> AgentDriver:
    """Factory: create the right driver for the given agent name."""
    if agent_name == "grok":
        from .grok import GrokDriver

        return GrokDriver(model=model)
    raise ValueError(f"Unknown agent: {agent_name!r}. Available: grok")


# ---------------------------------------------------------------------------
# Assertion helpers
# ---------------------------------------------------------------------------


def assert_patchloom_used(result: AgentResult, expected_command: str) -> None:
    """Assert patchloom was invoked with the expected subcommand."""
    matching = [
        c
        for c in result.patchloom_calls
        if c.get("args") and c["args"][0] == expected_command
    ]
    all_commands = [
        c["args"][0] for c in result.patchloom_calls if c.get("args")
    ]
    assert matching, (
        f"Expected patchloom {expected_command} to be invoked, but "
        f"patchloom was called {len(result.patchloom_calls)} time(s) "
        f"with commands: {all_commands or 'none'}"
    )


def assert_patchloom_used_any(
    result: AgentResult, commands: list[str]
) -> None:
    """Assert patchloom was invoked with any of the expected subcommands."""
    used = {c["args"][0] for c in result.patchloom_calls if c.get("args")}
    overlap = used & set(commands)
    assert overlap, (
        f"Expected patchloom to use one of {commands}, "
        f"but only saw: {sorted(used) or 'no patchloom calls'}"
    )


def report_raw_tool_usage(result: AgentResult) -> list[str]:
    """Return names of raw agent tools spotted in the response text."""
    raw_tools = []
    text = (result.stdout or "") + (result.output_json or {}).get("text", "")
    for tool in ["search_replace", "read_file", "grep", "list_dir"]:
        if tool in text:
            raw_tools.append(tool)
    return raw_tools


def parse_shim_log(path: Path) -> list[dict]:
    """Parse the JSONL log written by the patchloom shim."""
    if not path.exists():
        return []
    entries = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            try:
                entries.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return entries
