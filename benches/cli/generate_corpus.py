#!/usr/bin/env python3
"""Generate synthetic benchmark corpora at different scales."""

import json
import random
import sys
from pathlib import Path

SCALES = {
    "small": {"files": 50, "lines_per_file": 100},
    "medium": {"files": 500, "lines_per_file": 100},
    "large": {"files": 5000, "lines_per_file": 100},
}

EXTENSIONS = [".py", ".rs", ".js", ".ts", ".go", ".md", ".txt"]

IMPORTS = [
    "import os", "import sys", "import json", "import pathlib",
    "use std::fs;", "use std::io;", "use anyhow::Result;",
    "const fs = require('fs');", "import express from 'express';",
    "package main", "import \"fmt\"",
]

FUNCTIONS = [
    "def process_data(items):", "def calculate_total(values):",
    "def validate_input(data):", "fn handle_request(req: &Request) ->",
    "function fetchData(url) {", "func processItems(items []Item)",
    "def get_user_name():", "def set_user_role(role):",
]

COMMENTS = [
    "# TODO: implement error handling",
    "# TODO: add logging here",
    "# FIXME: this is a workaround",
    "// TODO: refactor this function",
    "// NOTE: version 1.0.0 requirement",
    "# Version: v1.0.0",
]

BODY_LINES = [
    "    result = []",
    "    for item in items:",
    "        if item.is_valid():",
    "            result.append(item.process())",
    "    return result",
    "    let mut output = Vec::new();",
    "    config.version = \"v1.0.0\";",
    "    logger.info(\"processing complete\");",
    "    return {\"status\": \"ok\", \"version\": \"v1.0.0\"};",
    "    pass",
    "",
]


def generate_file(path: Path, lines: int):
    """Generate a single source-like file."""
    content = []
    content.append(random.choice(IMPORTS))
    content.append("")
    for i in range(lines - 2):
        r = random.random()
        if r < 0.05:
            content.append(random.choice(COMMENTS))
        elif r < 0.10:
            content.append(random.choice(FUNCTIONS))
        else:
            content.append(random.choice(BODY_LINES))
    content.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(content) + "\n")


def generate_corpus(base: Path, scale_name: str):
    """Generate a corpus at the given scale."""
    cfg = SCALES[scale_name]
    corpus_dir = base / scale_name
    if corpus_dir.exists():
        print(f"  {scale_name}: already exists, skipping")
        return
    corpus_dir.mkdir(parents=True)

    # Create structured config files for doc benchmarks
    (corpus_dir / "config.json").write_text(
        json.dumps({"name": "bench", "version": "v1.0.0", "debug": False}, indent=2)
        + "\n"
    )
    (corpus_dir / "config.yaml").write_text(
        "# Application configuration\n"
        "app:\n"
        "  name: bench  # project name\n"
        "  version: 'v1.0.0'  # current release\n"
        "  debug: false\n"
    )
    (corpus_dir / "config.toml").write_text(
        "# Application configuration\n"
        "[app]\n"
        'name = "bench"  # project name\n'
        'version = "v1.0.0"  # current release\n'
        "debug = false\n"
    )
    (corpus_dir / "README.md").write_text(
        "# Bench Project\n\n"
        "## Commands\n\n"
        "| Command | Description |\n"
        "|---------|-------------|\n"
        "| build | Build the project |\n"
        "| test | Run tests |\n\n"
        "## Changelog\n\n"
        "- v1.0.0 initial release\n"
    )
    (corpus_dir / "VERSION").write_text("v1.0.0\n")
    (corpus_dir / "package.json").write_text(
        json.dumps({"name": "bench", "version": "v1.0.0", "main": "index.js"}, indent=2)
        + "\n"
    )

    # Generate source files across subdirectories
    dirs = ["src", "lib", "tests", "docs", "scripts"]
    for i in range(cfg["files"]):
        subdir = random.choice(dirs)
        ext = random.choice(EXTENSIONS)
        name = f"file_{i:04d}{ext}"
        path = corpus_dir / subdir / name
        generate_file(path, cfg["lines_per_file"])

    total_files = cfg["files"] + 1  # +1 for config.json
    total_lines = cfg["files"] * cfg["lines_per_file"]
    print(f"  {scale_name}: {total_files} files, ~{total_lines} lines")


def main():
    base = Path(__file__).parent / "corpus"
    scales = sys.argv[1:] if len(sys.argv) > 1 else list(SCALES.keys())
    print("Generating benchmark corpora:")
    for scale in scales:
        if scale not in SCALES:
            print(f"  unknown scale: {scale}", file=sys.stderr)
            continue
        generate_corpus(base, scale)
    print("Done.")


if __name__ == "__main__":
    main()
