"""Cline CLI agent driver."""

from __future__ import annotations

import os
import shutil
import subprocess
import time
from pathlib import Path

from .base import AgentDriver, AgentMetadata, AgentResult, load_shim_calls, parse_last_json_line


class ClineDriver(AgentDriver):
    """Invokes Cline CLI in headless mode."""

    def __init__(self, model: str = "claude-sonnet-4-20250514"):
        super().__init__(name="cline", model=model)

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
            "cline",
            prompt,
            "--json",
            "--model",
            self.model,
            "--auto-approve",
        ]

        start = time.monotonic()
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout_secs,
                env=env,
                cwd=str(cwd),
            )
        except subprocess.TimeoutExpired as exc:
            return AgentResult(
                stdout=exc.stdout or "",
                stderr=exc.stderr or "",
                exit_code=-1,
                output_json=None,
                duration_secs=time.monotonic() - start,
                patchloom_calls=load_shim_calls(extra_env),
            )
        duration = time.monotonic() - start

        # Cline --json emits NDJSON; try to parse the last complete line
        output_json = parse_last_json_line(proc.stdout)

        return AgentResult(
            stdout=proc.stdout,
            stderr=proc.stderr,
            exit_code=proc.returncode,
            output_json=output_json,
            duration_secs=duration,
            patchloom_calls=load_shim_calls(extra_env),
        )

    def is_available(self) -> bool:
        return shutil.which("cline") is not None

    def get_metadata(self) -> AgentMetadata:
        cli_version = "unknown"
        try:
            proc = subprocess.run(
                ["cline", "--version"],
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

