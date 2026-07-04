# Contributing to Patchloom

Thank you for your interest in contributing to Patchloom!

## Prerequisites

- Rust 1.95+ ([rustup.rs](https://rustup.rs/); see `rust-version` in `Cargo.toml`)
- Git

## Getting started

```bash
git clone https://github.com/patchloom/patchloom.git
cd patchloom
make check
```

`make check` runs formatting, clippy, unit tests (all-features + no-default-features + ast-only + mcp-without-ast), integration tests, PTY tests, release notes verification, test hygiene audit, and generated-doc freshness checks. While iterating locally, `make check-fast` skips the doc freshness checks but adds library hygiene enforcement.

## Development workflow

1. Create a branch from `main`.
2. Make your changes.
3. Run `make check` to verify everything passes.
4. Commit with DCO sign-off: `git commit -s`.
5. Open a pull request.

### Useful make targets

This is a quick-reference subset. For the complete list, see [AGENTS.md](./AGENTS.md).

| Command | What it does |
|---------|-------------|
| `make fmt` | Run `cargo fmt --all` |
| `make build` | Build with all features |
| `make test` | Run unit tests |
| `make integration-test` | Run integration tests |
| `make pty-test` | Run PTY-based interactive terminal tests (serial) |
| `make clippy` | Run clippy with `-D warnings` |
| `make check` | Full CI gate (run before every commit) |
| `make check-fast` | Fast check (skips doc verification) |
| `make update-readme` | Update README.md rounded test count |
| `make sync-patchloom-md` | Regenerate PATCHLOOM.md from `patchloom agent-rules` output |

## Adding a new command

See the "Adding a new command" section in [AGENTS.md](./AGENTS.md) for the step-by-step template.

## Adding a new MCP tool

See the "Adding a new MCP tool" section in [AGENTS.md](./AGENTS.md).

## Coding conventions

- Run `cargo fmt` before every commit.
- `cargo clippy --all-targets --all-features -- -D warnings` must produce zero warnings.
- Never use `unwrap()` or `expect()` in non-test code; use `?` with `anyhow::Context` or `.ok_or_else(|| anyhow!("msg"))?`. Exception: `expect()` is acceptable on infallible internal invariants (e.g. `Mutex::lock`).
- `unsafe_code = "deny"` is enforced in `Cargo.toml`.
- All commits require a `Signed-off-by` line ([DCO](https://developercertificate.org/)). Use `git commit -s`.

## Troubleshooting `make check` failures

| Failure | Fix |
|---------|-----|
| `make fmt-check` | Run `make fmt` to auto-format, then re-run `make check`. |
| `make clippy` | Address the specific warning shown in the output. Clippy treats all warnings as errors (`-D warnings`). |
| `make check-patchloom-md` | The agent-rules output changed. Run `make sync-patchloom-md` to regenerate `PATCHLOOM.md`. |
| `make check-readme` | Test counts drifted. Run `make update-readme` to refresh `README.md`. |

## License

By contributing, you agree that your contributions will be licensed under the same dual license as the project: MIT or Apache-2.0, at the user's option.
