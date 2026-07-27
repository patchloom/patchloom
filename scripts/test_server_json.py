#!/usr/bin/env python3
"""Lock server.json constraints for MCP Registry publish."""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "server.json"
# Official MCP Registry rejects body.description longer than this (HTTP 422).
MAX_DESCRIPTION = 100


def main() -> int:
    data = json.loads(SERVER.read_text(encoding="utf-8"))
    desc = data.get("description")
    if not isinstance(desc, str) or not desc.strip():
        print("server.json description must be a non-empty string", file=sys.stderr)
        return 1
    if len(desc) > MAX_DESCRIPTION:
        print(
            f"server.json description is {len(desc)} chars; "
            f"MCP Registry max is {MAX_DESCRIPTION}",
            file=sys.stderr,
        )
        print(desc, file=sys.stderr)
        return 1
    name = data.get("name")
    if name != "io.github.patchloom/patchloom":
        print(f"unexpected server.json name: {name!r}", file=sys.stderr)
        return 1
    print(f"ok: description {len(desc)}/{MAX_DESCRIPTION} chars")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
