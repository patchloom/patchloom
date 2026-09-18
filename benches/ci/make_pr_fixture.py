#!/usr/bin/env python3
"""Build a PR-gate corpus large enough that algorithm time dominates startup.

Usage: python3 make_pr_fixture.py DEST

Creates:
  DEST/txt/*.txt   ~2 MiB of searchable text
  DEST/src/*.rs    Rust sources for ast map / ast rename
  DEST/pkg.json    tiny JSON for doc get
"""

from __future__ import annotations

import argparse
from pathlib import Path

LINE = "hello world line with needle token\n"
RUST = "fn foo() { let x = 1; }\nfn bar() { foo(); }\n"


def write_fixture(root: Path) -> None:
    root.mkdir(parents=True, exist_ok=True)
    txt = root / "txt"
    txt.mkdir(exist_ok=True)
    blob = LINE * 400
    for i in range(160):
        (txt / f"f{i:03}.txt").write_text(blob, encoding="utf-8")
    src = root / "src"
    src.mkdir(exist_ok=True)
    rust = RUST * 80
    for i in range(40):
        (src / f"lib_{i:03}.rs").write_text(rust, encoding="utf-8")
    (root / "pkg.json").write_text(
        '{"name":"bench","version":"1.0.0"}\n', encoding="utf-8"
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("dest")
    args = parser.parse_args()
    write_fixture(Path(args.dest))


if __name__ == "__main__":
    main()
