# Contributing to Patchloom

Thank you for considering contributing to Patchloom! This document covers the
practical steps for getting a change merged. For detailed architecture,
conventions, and command structure, see [AGENTS.md](AGENTS.md).

## Quick start

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
cargo build --all-features
make check          # must pass before every commit
```

**Requirements:** Rust 1.95+ (the project MSRV). Install via [rustup](https://rustup.rs/).

## Development workflow

1. Fork the repo and create a feature branch from `main`.
2. Make your changes. Run `make check-fast` during development for rapid feedback.
3. Run `make check` before committing. This is the full CI gate: formatting,
   clippy, unit tests, integration tests, and doc verification.
4. If you touched `#[cfg(...)]` attributes or MCP-only code, also run
   `cargo check --all-targets`. `make check` uses `--all-features`, so it
   does not exercise the default-feature build.
5. Commit with a [DCO sign-off](#dco-sign-off).
5. Open a pull request against `main`.

### Useful make targets

| Target | Purpose |
|--------|---------|
| `make check` | Full CI gate (run before every commit) |
| `make check-fast` | Skip doc verification for faster iteration |
| `make build` | Build all features (`cargo build --all-features`) |
| `make fmt` | Auto-format with `cargo fmt` |
| `make test` | Unit tests only |
| `make integration-test` | Integration tests only |
| `make clippy` | Lint check |
| `make update-readme` | Refresh generated test counts in README.md and CHANGELOG.md |
| `make sync-patchloom-md` | Regenerate PATCHLOOM.md from `patchloom agent-rules` |
| `make audit` | Run `cargo audit` for known vulnerabilities |
| `cargo check --all-targets` | Catch default-feature build regressions after `#[cfg(...)]` or MCP edits |

## Writing tests

- Unit tests go in `#[cfg(test)] mod tests` blocks at the bottom of each source file.
- Integration tests go in `tests/integration.rs`.
- Use `tempfile::TempDir` for filesystem fixtures.
- Test both success paths and error/edge-case exit codes.
- See [AGENTS.md](AGENTS.md#testing) for test conventions.

## Coding standards

- `cargo fmt` and `cargo clippy --all-targets --all-features -- -D warnings` must be clean.
- No `unwrap()` in non-test code. Use `anyhow::Context` or `expect()` with a message.
- No unsafe Rust (`unsafe_code = "deny"` in Cargo.toml).
- Prefer returning exit codes from `src/exit.rs` over panicking.
- Keep `main.rs` thin; all logic lives in `lib.rs` and submodules.

See [AGENTS.md](AGENTS.md#coding-conventions) for the full list.

## Adding a new command

See [AGENTS.md](AGENTS.md#adding-a-new-command) for the step-by-step guide to
adding a CLI command, and [AGENTS.md](AGENTS.md#adding-a-new-mcp-tool) for MCP tools.

## DCO sign-off

All commits must include a `Signed-off-by` line (Developer Certificate of
Origin). Use `git commit -s` to add it automatically:

```bash
git commit -s -m "feat: add frobnicate command"
```

This certifies that you wrote (or have the right to submit) the code under the
project's license terms. See [developercertificate.org](https://developercertificate.org/)
for details.

## Pull request guidelines

- One logical change per PR. Separate refactoring from feature work.
- Include tests for new functionality and bug fixes.
- Update documentation if your change affects user-facing behavior (README,
  command help text, reference docs).
- CI must pass. If it fails, fix it before requesting review.
- Squash merge is the default merge strategy.

## Reporting issues

Use [GitHub Issues](https://github.com/patchloom/patchloom/issues). Include:

- The patchloom version (`patchloom --version`).
- The command you ran and the full output.
- What you expected to happen vs. what actually happened.

For security vulnerabilities, see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the
same terms as the project: [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE),
at your option.
