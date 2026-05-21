"""OpenAI Codex CLI agent driver."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path

from .base import AgentDriver, AgentMetadata, AgentResult, parse_shim_log


class CodexDriver(AgentDriver):
    """Invokes OpenAI Codex CLI in headless mode via `codex exec`."""

    def __init__(self, model: str = "o3"):
        super().__init__(name="codex", model=model)

    def run_prompt(
        self,
        prompt: str,
        cwd: Path,
        *,
        max_turns: int = 10,
        timeout_secs: int = 120,
        extra_env: dict | None = None,
    ) -> AgentResult:
        env = {**os.environ, **(extra_env or {})}
        cmd = [
            "codex",
            "exec",
            prompt,
            "--json",
            "-m",
            self.model,
            "-C",
            str(cwd),
            "--sandbox",
            "workspace-write",
            "--ask-for-approval",
            "never",
            "--ephemeral",
        ]

        start = time.monotonic()
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout_secs,
                env=env,
            )
        except subprocess.TimeoutExpired as exc:
            return AgentResult(
                stdout=exc.stdout or "",
                stderr=exc.stderr or "",
                exit_code=-1,
                output_json=None,
                duration_secs=time.monotonic() - start,
                patchloom_calls=_load_calls(extra_env),
            )
        duration = time.monotonic() - start

        # Codex --json emits JSONL; try to parse the last complete line
        output_json = _parse_last_json_line(proc.stdout)

        return AgentResult(
            stdout=proc.stdout,
            stderr=proc.stderr,
            exit_code=proc.returncode,
            output_json=output_json,
            duration_secs=duration,
            patchloom_calls=_load_calls(extra_env),
        )

    def is_available(self) -> bool:
        return shutil.which("codex") is not None

    def get_metadata(self) -> AgentMetadata:
        cli_version = "unknown"
        try:
            proc = subprocess.run(
                ["codex", "--version"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            cli_version = proc.stdout.strip()
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass

        return AgentMetadata(
            agent_name=self.name,
            model_alias=self.model,
            model_name=self.model,
            cli_version=cli_version,
        )


def _parse_last_json_line(text: str) -> dict | None:
    """Parse the last valid JSON line from JSONL output."""
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line:
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def _load_calls(extra_env: dict | None) -> list[dict]:
    if not extra_env:
        return []
    log_path = extra_env.get("PATCHLOOM_SHIM_LOG", "")
    if not log_path:
        return []
    return parse_shim_log(Path(log_path))
