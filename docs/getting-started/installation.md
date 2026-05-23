# Installation

## From crates.io

```bash
cargo install patchloom
```

With MCP server support:

```bash
cargo install patchloom --features mcp
```

## From Homebrew

```bash
brew install patchloom/tap/patchloom
```

## From GitHub Releases

Pre-built binaries for Linux (x64, ARM64), macOS (x64, ARM64), and Windows (x64) are available on the [Releases](https://github.com/patchloom/patchloom/releases) page. Download the archive for your platform, extract, and place `patchloom` on your PATH.

Release binaries include the core CLI. If you need `mcp-server`, install from source or crates.io with `--features mcp`.

## From source

Install from source (requires Rust 1.95+):

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .

# With MCP support
cargo install --path . --features mcp
```

## Shell completions

After installing, generate shell completions:

```bash
# bash
patchloom completions bash > /etc/bash_completion.d/patchloom

# zsh
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
