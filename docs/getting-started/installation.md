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

Pre-built binaries include all commands, including the MCP server.

## From source

Install from source (requires Rust 1.95+):

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .
```

This builds with MCP support by default. To build without MCP
(smaller binary), use `cargo install --path . --no-default-features`.

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
