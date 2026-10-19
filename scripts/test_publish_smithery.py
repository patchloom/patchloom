#!/usr/bin/env python3
"""The Smithery upload must print the HTTP body when the API refuses it."""

from __future__ import annotations

import os
import stat
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts" / "publish-smithery.sh"


class PublishSmitheryTests(unittest.TestCase):
    def test_http_error_prints_status_and_body(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            curl = bin_dir / "curl"
            curl.write_text(
                textwrap.dedent(
                    """\
                    #!/usr/bin/env python3
                    import sys
                    args = sys.argv[1:]
                    url = next(a for a in args if a.startswith("https://"))
                    out = None
                    if "-o" in args:
                        out = args[args.index("-o") + 1]
                    if "/releases" in url:
                        if out:
                            open(out, "w", encoding="utf-8").write(
                                "Missing required permission: servers:write\\n"
                            )
                        if "-w" in args:
                            print("403")
                        raise SystemExit(0)
                    raise SystemExit(0)
                    """
                ),
                encoding="utf-8",
            )
            curl.chmod(curl.stat().st_mode | stat.S_IEXEC)
            bundle_dir = ROOT / "target" / "mcpb"
            bundle_dir.mkdir(parents=True, exist_ok=True)
            bundle = bundle_dir / "patchloom-9.9.9-smithery-test.mcpb"
            bundle.write_bytes(b"mcpb")
            env = os.environ.copy()
            env["PATH"] = str(bin_dir) + os.pathsep + env.get("PATH", "")
            env["SMITHERY_API_KEY"] = "test-key-not-a-real-secret"
            env["VERSION"] = "9.9.9-smithery-test"
            try:
                result = subprocess.run(
                    ["bash", str(SCRIPT)],
                    cwd=ROOT,
                    env=env,
                    capture_output=True,
                    text=True,
                    check=False,
                )
            finally:
                bundle.unlink(missing_ok=True)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Smithery release upload failed: HTTP 403", result.stderr)
        self.assertIn("Missing required permission: servers:write", result.stderr)
        self.assertNotIn("test-key-not-a-real-secret", result.stdout)
        self.assertNotIn("test-key-not-a-real-secret", result.stderr)


if __name__ == "__main__":
    unittest.main()
