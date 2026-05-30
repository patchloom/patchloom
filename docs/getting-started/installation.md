# Installation

Patchloom is not yet published to crates.io or Homebrew. Install from source
for now. The sections below describe the planned post-launch channels.

## From source (current)

Install the core CLI from source (requires Rust 1.95+):

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path .
```

Install with MCP support:

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo install --path . --features mcp
```

The `mcp-server` command is feature-gated. If you only run `cargo install --path .`, you get the core CLI without MCP.

If you're contributing from a source checkout, use `make check-fast` while iterating and `make check` before committing. `make check` is the full local CI gate.

## From crates.io (after public launch)

Core CLI:

```bash
cargo install patchloom
```

MCP-capable install:

```bash
cargo install patchloom --features mcp
```

## From GitHub releases (after public launch)

Pre-built binaries for Linux, macOS, and Windows will be available on the
[Releases](https://github.com/patchloom/patchloom/releases) page. Download
the archive for your platform, extract, and place `patchloom` on your PATH.

The current planned release pipeline targets the core CLI build. If you need
`mcp-server`, install from source with `--features mcp`.

## From Homebrew (after public launch)

```bash
brew install patchloom/tap/patchloom
```

The planned Homebrew formula also targets the core CLI build. If you need
`mcp-server`, install from source with `--features mcp`.

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
