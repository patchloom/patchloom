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


@dataclass
class AgentMetadata:
    """Metadata about the agent and model used for a test run."""

    agent_name: str       # e.g. "grok"
    model_alias: str      # e.g. "grok-build"
    model_name: str       # e.g. "Grok 4.3"
    cli_version: str      # e.g. "grok 0.1.211 (2f2cd6d5c)"


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

    @abstractmethod
    def get_metadata(self) -> AgentMetadata:
        """Return metadata about the agent, model, and CLI version."""


def create_driver(agent_name: str, model: str) -> AgentDriver:
    """Factory: create the right driver for the given agent name."""
    if agent_name == "grok":
        from .grok import GrokDriver

        return GrokDriver(model=model)
    if agent_name == "claude":
        from .claude import ClaudeDriver

        return ClaudeDriver(model=model)
    if agent_name == "aider":
        from .aider import AiderDriver

        return AiderDriver(model=model)
    raise ValueError(f"Unknown agent: {agent_name!r}. Available: grok, claude, aider")


# ---------------------------------------------------------------------------
# Assertion helpers
# ---------------------------------------------------------------------------


_PATCHLOOM_SUBCOMMANDS = {
    "search", "replace", "patch", "md", "doc", "tidy",
    "create", "delete", "read", "status", "tx", "completions", "agent-rules",
    "batch", "mcp-server",
}


def _extract_subcommand(args: list[str]) -> str | None:
    """Find the patchloom subcommand in an arg list.

    Agents may place global flags (--cwd, --json) before the subcommand,
    so we scan all args instead of just checking args[0].
    """
    for arg in args:
        if arg in _PATCHLOOM_SUBCOMMANDS:
            return arg
    return None


def assert_patchloom_used(result: AgentResult, expected_command: str) -> None:
    """Assert patchloom was invoked with the expected subcommand."""
    matching = [
        c
        for c in result.patchloom_calls
        if c.get("args") and _extract_subcommand(c["args"]) == expected_command
    ]
    all_commands = [
        _extract_subcommand(c["args"])
        for c in result.patchloom_calls
        if c.get("args")
    ]
    assert matching, (
        f"Expected patchloom {expected_command} to be invoked, but "
        f"patchloom was called {len(result.patchloom_calls)} time(s) "
        f"with subcommands: {[c for c in all_commands if c] or 'none'}"
    )


def assert_patchloom_used_any(
    result: AgentResult, commands: list[str]
) -> None:
    """Assert patchloom was invoked with any of the expected subcommands."""
    used = {
        _extract_subcommand(c["args"])
        for c in result.patchloom_calls
        if c.get("args")
    }
    used.discard(None)
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
