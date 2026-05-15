# Installation

## From source (current)

Patchloom is not yet published to crates.io. Build from source:

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo build --release
# Binary is at target/release/patchloom
```

Optionally install it to your PATH:

```bash
cargo install --path .
```

## From crates.io (after public launch)

```bash
cargo install patchloom
```

## From GitHub releases (after public launch)

Pre-built binaries for Linux, macOS, and Windows will be available on the
[Releases](https://github.com/patchloom/patchloom/releases) page. Download
the archive for your platform, extract, and place `patchloom` on your PATH.

## From Homebrew (after public launch)

```bash
brew install patchloom/tap/patchloom
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
```

## Verify

```bash
patchloom --version
patchloom --help
```
