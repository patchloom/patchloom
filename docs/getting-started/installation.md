# Installation

## From Homebrew (macOS and Linux)

```bash
brew install patchloom/tap/patchloom
```

This installs the core CLI from a pre-built binary. If you need
`mcp-server`, install from source with `--features mcp` (see below).

## From crates.io

Core CLI:

```bash
cargo install patchloom
```

With MCP support:

```bash
cargo install patchloom --features mcp
```

## From GitHub Releases

Pre-built binaries for Linux (x64, ARM64), macOS (x64, ARM64), and Windows
(x64) are available on the
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

Pre-built binaries include the core CLI only. If you need `mcp-server`,
install from source with `--features mcp`.

## From source

Install from source (requires Rust 1.95+):

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .
```

With MCP support:

```bash
cargo install --path . --features mcp
```

The `mcp-server` command is feature-gated. If you only run
`cargo install --path .`, you get the core CLI without MCP.

If you're contributing from a source checkout, use `make check-fast`
while iterating and `make check` before committing.

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
