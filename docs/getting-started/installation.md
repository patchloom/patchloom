# Installation

Patchloom is not yet published to crates.io or Homebrew. Install from source
for now. The sections below describe the planned post-launch channels.

## From source (current)

Install from source:

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
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

# elvish
patchloom completions elvish > ~/.config/elvish/rc.elv
```

## Verify

```bash
patchloom --version
patchloom --help
```
