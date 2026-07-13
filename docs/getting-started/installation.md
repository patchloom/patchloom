# Installation

## From Homebrew (macOS and Linux)

```bash
brew install patchloom/tap/patchloom
```

This installs patchloom with all commands, including the MCP server.

## From crates.io

```bash
cargo install patchloom
```

## From GitHub Releases

Pre-built binaries for Linux (x64, ARM64, musl), macOS (x64, ARM64), and
Windows (x64, ARM64) are available on the
[Releases](https://github.com/patchloom/patchloom/releases/latest) page.
Download the archive for your platform, extract, and place `patchloom` on
your PATH.

Shell and PowerShell installer scripts are also available:

```bash
# Unix (Linux/macOS)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/patchloom/patchloom/releases/latest/download/patchloom-installer.sh | sh

# Windows (PowerShell)
powershell -ExecutionPolicy ByPass -c "irm https://github.com/patchloom/patchloom/releases/latest/download/patchloom-installer.ps1 | iex"
```

Pre-built binaries include all commands, including the MCP server.

## From source

Install from source (requires Rust 1.95+):

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .
```

This builds with all features by default (CLI + MCP server + AST operations).
To build a smaller binary without optional features (CLI is always included for the binary):

```bash
# CLI + AST only (no MCP server, no tokio/async)
cargo install --path . --no-default-features --features "cli,ast"

# CLI + MCP only (no AST grammars)
cargo install --path . --no-default-features --features "cli,mcp"

# CLI only (no MCP, no AST)
cargo install --path . --no-default-features --features cli
```

If you're contributing from a source checkout, use `make check-fast`
while iterating and `make check` before committing.

## As a Rust library

Add patchloom as a dependency to embed structured file editing in your own
Rust tools. Disable default features to omit CLI (clap), MCP server, and AST:

```toml
[dependencies]
patchloom = { default-features = false }
```

To add AST support without CLI/MCP (LLM agent embedders typically use
`ast` + `files` for plan execution and AST file mutators):

```toml
patchloom = { default-features = false, features = ["ast", "files"] }
```

See the [crate documentation](https://docs.rs/patchloom) for the full API
surface (`require_change`, `command_position`, `ast_rename_batch`,
`find_files_with_symbol`, `classify_error`, `restore_path_from_session`,
`run_post_write_validation`, `match_mode`) and the
[introduction](../introduction.md#as-a-rust-library) for a quick overview.
Embedder tables live under [Library API](../reference/README.md#library-api)
in the reference.

## Shell completions

After installing, generate shell completions:

```bash
# bash (system-wide; may require sudo and /etc/bash_completion.d in your setup)
patchloom completions bash > /etc/bash_completion.d/patchloom

# zsh (ensure ~/.zfunc is in $fpath, e.g. via oh-my-zsh custom or compinit)
patchloom completions zsh > ~/.zfunc/_patchloom

# fish
patchloom completions fish > ~/.config/fish/completions/patchloom.fish

# elvish
patchloom completions elvish > ~/.config/elvish/rc.elv

# PowerShell
patchloom completions powershell >> $PROFILE
```

## Verify

```bash
patchloom --version
patchloom --help
```
