# Patchloom

Patchloom is a Rust CLI for agent-grade repo operations.

It is designed to give coding agents and power users one reliable binary for:

- safe search
- safe patch application
- parser-backed config edits
- Markdown section-aware edits
- multi-file transaction-oriented repo changes

## Status

Patchloom is in early bootstrap. The repo is currently private while the initial bootstrap, CI, and policy setup are being completed. The day 1 setup uses a self-hosted Linux CI workflow and a cargo-dist starter release pipeline for GitHub Releases, Homebrew, and crates.io. While the repo is private, external publishing jobs stay disabled. Winget, Scoop, and other packaging layers follow once asset names and checksums stabilize.

## Goals

- Unix-friendly, machine-readable CLI behavior
- Safe dry-run and diff-first workflows
- Strong support for Markdown, JSON, YAML, and TOML repo edits
- First-class newline and line-ending hygiene
- Clear, low-friction open-source contribution policy

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](./LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE))

at your option.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

All commits must be signed off with `git commit -s`.

## Security

For current security reporting guidance, see [SECURITY.md](./SECURITY.md).

GitHub private vulnerability reporting will be enabled after the repository becomes public.
