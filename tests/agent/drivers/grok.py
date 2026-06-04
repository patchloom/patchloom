"""Grok Build CLI agent driver."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from pathlib import Path

from .base import AgentDriver, AgentMetadata, AgentResult, load_shim_calls


class GrokDriver(AgentDriver):
    """Invokes Grok Build CLI in headless mode."""

    def __init__(self, model: str = "grok-build"):
        super().__init__(name="grok", model=model)

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
            "grok",
            "-p",
            prompt,
            "--yolo",
            "--output-format",
            "json",
            "--max-turns",
            str(max_turns),
            "--cwd",
            str(cwd),
            "-m",
            self.model,
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
                patchloom_calls=load_shim_calls(extra_env),
            )
        duration = time.monotonic() - start

        output_json = _try_parse_json(proc.stdout)

        return AgentResult(
            stdout=proc.stdout,
            stderr=proc.stderr,
            exit_code=proc.returncode,
            output_json=output_json,
            duration_secs=duration,
            patchloom_calls=load_shim_calls(extra_env),
        )

    def is_available(self) -> bool:
        return shutil.which("grok") is not None

    def get_metadata(self) -> AgentMetadata:
        # CLI version
        cli_version = "unknown"
        try:
            proc = subprocess.run(
                ["grok", "--version"],
                capture_output=True, text=True, timeout=5,
            )
            cli_version = proc.stdout.strip()
        except (subprocess.TimeoutExpired, FileNotFoundError):
            pass  # CLI not installed or too slow; use default

        # Model name (ask the model to self-identify)
        model_name = "unknown"
        try:
            proc = subprocess.run(
                ["grok", "-p",
                 "Reply with ONLY your model name and version, nothing else.",
                 "--output-format", "json", "-m", self.model,
                 "--max-turns", "1"],
                capture_output=True, text=True, timeout=30,
            )
            data = json.loads(proc.stdout)
            model_name = data.get("text", "unknown").strip()
        except (subprocess.TimeoutExpired, FileNotFoundError,
                json.JSONDecodeError, KeyError):
            pass  # model query failed; use default

        return AgentMetadata(
            agent_name=self.name,
            model_alias=self.model,
            model_name=model_name,
            cli_version=cli_version,
        )


def _try_parse_json(text: str) -> dict | None:
    try:
        return json.loads(text)
    except (json.JSONDecodeError, TypeError):
        return None

